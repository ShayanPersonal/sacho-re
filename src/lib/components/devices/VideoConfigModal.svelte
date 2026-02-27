<script lang="ts">
    import type {
        VideoDevice,
        VideoDeviceConfig,
        VideoCodec,
        HardwareEncoderType,
        CodecCapability,
        EncoderAvailability,
        EncoderTestResult,
    } from "$lib/api";
    import {
        isRawFormat,
        is10BitFormat,
        getCodecDisplayName,
        getResolutionLabel,
        getTargetResolutions,
        getTargetFramerates,
        formatFps,
        computeDefaultConfig,
        validateVideoDeviceConfig,
        getEncoderAvailability,
        testEncoderPreset,
        autoSelectEncoderPreset,
        sortFormatsByPriority,
        defaultPassthrough,
        supportsPassthrough,
        getCodecInfo,
        formatDisplayName,
    } from "$lib/api";
    interface Props {
        device: VideoDevice;
        currentConfig: VideoDeviceConfig | null;
        onSave: (config: VideoDeviceConfig) => void;
        onClose: () => void;
    }

    let { device, currentConfig, onSave, onClose }: Props = $props();

    // Compute effective config: saved config or smart defaults
    // svelte-ignore state_referenced_locally
    const effectiveConfig = currentConfig ?? computeDefaultConfig(device);

    // State for selections — cascade: Resolution → Framerate → Format
    let selectedWidth = $state<number>(effectiveConfig?.source_width ?? 0);
    let selectedHeight = $state<number>(effectiveConfig?.source_height ?? 0);
    let selectedFps = $state<number>(effectiveConfig?.source_fps ?? 0);
    // svelte-ignore state_referenced_locally
    let selectedFormat = $state<string>(
        effectiveConfig?.source_format ??
            Object.keys(device.capabilities)[0] ??
            "",
    );
    // Resolve legacy 0 sentinel ("Match Source") to source values for backward compat
    let selectedTargetWidth = $state<number>(
        effectiveConfig?.target_width || effectiveConfig?.source_width || 0,
    );
    let selectedTargetHeight = $state<number>(
        effectiveConfig?.target_height || effectiveConfig?.source_height || 0,
    );
    let selectedTargetFps = $state<number>(
        effectiveConfig?.target_fps || effectiveConfig?.source_fps || 0,
    );

    // Encoding settings (per-device)
    let passthrough = $state<boolean>(effectiveConfig?.passthrough ?? true);
    let encodingCodec = $state<VideoCodec | null>(
        effectiveConfig?.encoding_codec ?? null,
    );
    let encoderType = $state<HardwareEncoderType | null>(
        effectiveConfig?.encoder_type ?? null,
    );
    let presetLevel = $state<number>(effectiveConfig?.preset_level ?? 3);
    let effortLevel = $state<number>(effectiveConfig?.effort_level ?? 3);
    let videoBitDepth = $state<number | null>(
        effectiveConfig?.video_bit_depth ?? null,
    );
    let encoderAvailability = $state<EncoderAvailability | null>(null);

    // Auto-select state
    let autoSelectRunning = $state(false);
    let autoSelectProgress = $state("");
    let autoSelectError = $state("");

    // More encoding options revealer
    let showMoreEncoding = $state(false);

    // Help tooltips (only one open at a time, shared position style)
    let showResolutionHelp = $state(false);
    let showFpsHelp = $state(false);
    let showStreamSourceHelp = $state(false);
    let showCodecHelp = $state(false);
    let showEncoderHelp = $state(false);
    let tooltipStyle = $state("");

    function positionTooltip(e: MouseEvent) {
        const btn = e.currentTarget as HTMLElement;
        const rect = btn.getBoundingClientRect();
        const tooltipWidth = 240;
        let left = rect.left + rect.width / 2 - tooltipWidth / 2;
        left = Math.max(8, Math.min(left, window.innerWidth - tooltipWidth - 8));
        let top = rect.bottom + 6;
        if (top + 200 > window.innerHeight) {
            top = rect.top - 6;
            tooltipStyle = `left:${left}px;bottom:${window.innerHeight - top}px;`;
        } else {
            tooltipStyle = `left:${left}px;top:${top}px;`;
        }
    }

    // Encoder test state
    let testRunning = $state(false);
    let testResult = $state<EncoderTestResult | null>(null);
    let testError = $state("");

    // Load encoder availability on mount, resolve null codec/encoder to recommended
    $effect(() => {
        getEncoderAvailability()
            .then((a) => {
                encoderAvailability = a;
                // Resolve null encoding codec to the backend's recommended
                if (encodingCodec === null) {
                    encodingCodec = a.recommended_codec as VideoCodec;
                }
                // Resolve null encoder type to the recommended for current codec
                if (encoderType === null && encodingCodec) {
                    const info = getCodecInfo(a, encodingCodec);
                    if (info?.recommended) {
                        encoderType = info.recommended as HardwareEncoderType;
                    }
                }
            })
            .catch(() => {});
    });

    // ── Cascade: Resolution → Framerate → Format ──────────────────────

    // All resolutions: union across ALL formats
    const allResolutions = $derived.by(() => {
        const seen = new Set<string>();
        const result: { width: number; height: number; label: string }[] = [];
        for (const caps of Object.values(device.capabilities)) {
            for (const c of caps) {
                const key = `${c.width}x${c.height}`;
                if (!seen.has(key)) {
                    seen.add(key);
                    result.push({
                        width: c.width,
                        height: c.height,
                        label: getResolutionLabel(c.width, c.height),
                    });
                }
            }
        }
        // Sort by pixel count descending
        result.sort((a, b) => b.width * b.height - a.width * a.height);
        return result;
    });

    // Available FPS at the selected resolution (union across all formats)
    const availableFps = $derived.by(() => {
        const fpsSet = new Set<number>();
        for (const caps of Object.values(device.capabilities)) {
            const cap = caps.find(
                (c) => c.width === selectedWidth && c.height === selectedHeight,
            );
            if (cap) {
                for (const f of cap.framerates) fpsSet.add(f);
            }
        }
        const result = [...fpsSet];
        result.sort((a, b) => b - a); // Descending
        return result;
    });

    // Available formats at the selected resolution + framerate
    const availableFormats = $derived.by(() => {
        const result: string[] = [];
        for (const [format, caps] of Object.entries(device.capabilities)) {
            const cap = caps.find(
                (c) => c.width === selectedWidth && c.height === selectedHeight,
            );
            if (
                cap &&
                cap.framerates.some((f) => Math.abs(f - selectedFps) < 0.01)
            ) {
                result.push(format);
            }
        }
        return sortFormatsByPriority(result);
    });

    // Whether the selected format is a raw pixel format (requires encoding)
    const isSelectedRaw = $derived(isRawFormat(selectedFormat));

    // Whether this format requires encoding (raw pixels can't be stored as-is)
    const isEncodeOnly = $derived(isSelectedRaw);

    // Whether encoding settings are active (not passthrough)
    const isEncoding = $derived(!passthrough);

    // Whether the source format is 10-bit or higher
    const sourceIs10Bit = $derived(is10BitFormat(selectedFormat));

    // Target resolutions (only relevant for encoding): "Match Source" + common resolutions
    const targetResolutions = $derived(
        isEncoding ? getTargetResolutions(selectedWidth, selectedHeight) : [],
    );

    // Target framerates (only relevant for encoding): common values ≤ source fps
    const targetFramerates = $derived(
        isEncoding ? getTargetFramerates(selectedFps) : [],
    );

    // Available encoding codecs from encoder availability
    const availableEncodingCodecs = $derived.by(() => {
        if (!encoderAvailability)
            return [] as { codec: VideoCodec; label: string }[];
        const codecs: { codec: VideoCodec; label: string }[] = [];
        if (encoderAvailability.av1.available)
            codecs.push({ codec: "av1", label: "AV1" });
        if (encoderAvailability.vp9.available)
            codecs.push({ codec: "vp9", label: "VP9" });
        if (encoderAvailability.vp8.available)
            codecs.push({ codec: "vp8", label: "VP8" });
        if (encoderAvailability.h264.available)
            codecs.push({ codec: "h264", label: "H.264" });
        if (encoderAvailability.ffv1.available)
            codecs.push({
                codec: "ffv1",
                label: "FFV1 (huge, lossless files)",
            });
        return codecs;
    });

    // Available encoder backends for the selected encoding codec
    const availableEncoders = $derived.by(() => {
        if (!encoderAvailability || !encodingCodec) return [];
        const info = getCodecInfo(encoderAvailability, encodingCodec);
        return info?.encoders ?? [];
    });

    // When encoding codec changes, always select the recommended encoder for that codec.
    // svelte-ignore state_referenced_locally
    let lastCodecForEncoder = encodingCodec;
    $effect(() => {
        const codec = encodingCodec;
        if (!codec || !encoderAvailability) return;
        if (codec !== lastCodecForEncoder) {
            lastCodecForEncoder = codec;
            const info = getCodecInfo(encoderAvailability, codec);
            if (info?.recommended) {
                const rec = info.recommended as HardwareEncoderType;
                encoderType = null;
                queueMicrotask(() => {
                    encoderType = rec;
                });
            }
        }
    });

    // "(Recommended)" label: only for AV1 or VP9 when they have hardware accel
    const displayRecommendedCodec = $derived.by((): VideoCodec | null => {
        if (!encoderAvailability) return null;
        if (encoderAvailability.av1.has_hardware) return "av1";
        if (encoderAvailability.vp9.has_hardware) return "vp9";
        return null;
    });
    // "(Default)" label: the backend's auto-selected codec, only when it's not already "(Recommended)"
    const autoSelectedCodec = $derived<VideoCodec | null>(
        encoderAvailability
            ? (encoderAvailability.recommended_codec as VideoCodec)
            : null,
    );
    const recommendedEncoder = $derived.by(() => {
        if (!encoderAvailability || !encodingCodec) return null;
        const info = getCodecInfo(encoderAvailability, encodingCodec);
        return info?.recommended ?? null;
    });

    const levelLabels: Record<number, string> = {
        1: "Lowest",
        2: "Low",
        3: "Balanced",
        4: "High",
        5: "Highest",
    };

    // ── Cascade effects ──────────────────────────────────────────────

    // When resolution changes: recompute available fps; if current fps unavailable, pick highest
    $effect(() => {
        // Touch selectedWidth/selectedHeight to trigger
        const _w = selectedWidth,
            _h = selectedHeight;
        const fpsOptions = availableFps;
        if (fpsOptions.length > 0) {
            const hasClose = fpsOptions.some(
                (f) => Math.abs(f - selectedFps) < 0.01,
            );
            if (!hasClose) {
                selectedFps = fpsOptions[0]; // Pick highest available
            }
        }
    });

    // When resolution or framerate changes: always auto-select the best format
    // (H264 first, then MJPEG, then raw — per the availableFormats sort order)
    $effect(() => {
        // Touch selectedWidth/selectedHeight/selectedFps to trigger
        const _w = selectedWidth,
            _h = selectedHeight,
            _fps = selectedFps;
        const formats = availableFormats;
        if (formats.length > 0) {
            selectedFormat = formats[0];
        }
    });

    // When format changes: update passthrough/encode defaults
    // svelte-ignore state_referenced_locally
    let lastFormatForPassthrough = selectedFormat;
    $effect(() => {
        if (selectedFormat !== lastFormatForPassthrough) {
            lastFormatForPassthrough = selectedFormat;
            passthrough = defaultPassthrough(selectedFormat);
        }
    });

    // Auto-select 10-bit when source is 10-bit and codec is FFV1
    $effect(() => {
        if (encodingCodec === "ffv1" && sourceIs10Bit) {
            videoBitDepth = 10;
        }
    });

    // Keep target in sync: default to source values, clamp when source shrinks,
    // and reset to source if current target isn't in the available options list
    $effect(() => {
        if (passthrough) {
            selectedTargetWidth = selectedWidth;
            selectedTargetHeight = selectedHeight;
            selectedTargetFps = selectedFps;
        }
        if (isEncoding) {
            if (
                selectedTargetWidth > selectedWidth ||
                selectedTargetHeight > selectedHeight
            ) {
                selectedTargetWidth = selectedWidth;
                selectedTargetHeight = selectedHeight;
            }
            if (selectedTargetFps > selectedFps + 0.5) {
                selectedTargetFps = selectedFps;
            }
            // Reset to source if target isn't in the available options
            const resInList = targetResolutions.some(
                (r) => r.width === selectedTargetWidth && r.height === selectedTargetHeight,
            );
            if (!resInList) {
                selectedTargetWidth = selectedWidth;
                selectedTargetHeight = selectedHeight;
            }
            const fpsInList = targetFramerates.some(
                (f) => Math.abs(f - selectedTargetFps) < 0.01,
            );
            if (!fpsInList) {
                selectedTargetFps = selectedFps;
            }
        }
    });

    function handleResolutionChange(value: string) {
        const [w, h] = value.split("x").map(Number);
        selectedWidth = w;
        selectedHeight = h;
    }

    function handleTargetResolutionChange(value: string) {
        const [w, h] = value.split("x").map(Number);
        selectedTargetWidth = w;
        selectedTargetHeight = h;
    }

    function handleTargetFpsChange(value: string) {
        selectedTargetFps = Number(value);
    }

    let validationError = $state<string | null>(null);

    /** Build the current config from UI state */
    function buildConfig(): VideoDeviceConfig {
        return {
            source_format: selectedFormat,
            source_width: selectedWidth,
            source_height: selectedHeight,
            source_fps: selectedFps,
            passthrough,
            encoding_codec: encodingCodec,
            encoder_type: encoderType,
            preset_level: presetLevel,
            effort_level: effortLevel,
            video_bit_depth: encodingCodec === "ffv1" ? videoBitDepth : null,
            target_width: selectedTargetWidth,
            target_height: selectedTargetHeight,
            target_fps: selectedTargetFps,
        };
    }

    /** Check if the config has changed from what was loaded */
    function hasChanges(): boolean {
        if (!effectiveConfig) return true;
        const current = buildConfig();
        return (
            current.source_format !== effectiveConfig.source_format ||
            current.source_width !== effectiveConfig.source_width ||
            current.source_height !== effectiveConfig.source_height ||
            Math.abs(current.source_fps - effectiveConfig.source_fps) > 0.01 ||
            current.passthrough !== effectiveConfig.passthrough ||
            current.encoding_codec !== effectiveConfig.encoding_codec ||
            current.encoder_type !== effectiveConfig.encoder_type ||
            current.preset_level !== effectiveConfig.preset_level ||
            current.effort_level !== effectiveConfig.effort_level ||
            current.video_bit_depth !== effectiveConfig.video_bit_depth ||
            current.target_width !== effectiveConfig.target_width ||
            current.target_height !== effectiveConfig.target_height ||
            Math.abs(current.target_fps - effectiveConfig.target_fps) > 0.01
        );
    }

    /** Save (if changed) and close the modal */
    async function saveAndClose() {
        if (testRunning) return;
        if (hasChanges()) {
            validationError = null;
            const valid = await validateVideoDeviceConfig(
                device.id,
                selectedFormat,
                selectedWidth,
                selectedHeight,
                selectedFps,
            );
            if (!valid) {
                validationError = `This device does not support ${selectedWidth}x${selectedHeight} @ ${formatFps(selectedFps)}fps with ${selectedFormat}. Try a different combination.`;
                return;
            }
            onSave(buildConfig());
        }
        onClose();
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
    class="modal-overlay"
    onclick={saveAndClose}
    onkeydown={(e) => e.key === "Escape" && saveAndClose()}
>
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="modal-content" onclick={(e) => e.stopPropagation()}>
        <div class="modal-header">
            <div class="header-left">
                <h3>Configure Video Source</h3>
                <span class="device-name-label">{device.name}</span>
            </div>
        </div>

        <div class="modal-inner">
            {#if testRunning}
                <div class="test-lock-overlay">
                    <div class="lock-message">
                        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                            <circle cx="12" cy="12" r="10" />
                            <polyline points="12 6 12 12 16 14" />
                        </svg>
                        <span>Testing configuration...</span>
                    </div>
                </div>
            {/if}
        <div class="modal-body">
            <!-- Source Resolution -->
            <div class="field">
                <label for="resolution-select">
                    Source Resolution
                    <span class="help-wrapper">
                        <button
                            class="help-btn"
                            onclick={(e) => {
                                e.stopPropagation();
                                showResolutionHelp = !showResolutionHelp;
                                showFpsHelp = false;
                                showStreamSourceHelp = false;
                                showCodecHelp = false;
                                showEncoderHelp = false;
                                if (showResolutionHelp) positionTooltip(e);
                            }}
                            onblur={() => (showResolutionHelp = false)}
                        >
                            ?
                        </button>
                        {#if showResolutionHelp}
                            <span class="help-tooltip" style={tooltipStyle}>
                                Higher resolution improves video quality. But it increases file size and is more demanding on your system.
                            </span>
                        {/if}
                    </span>
                </label>
                <div class="select-wrapper">
                    <select
                        id="resolution-select"
                        value="{selectedWidth}x{selectedHeight}"
                        onchange={(e) =>
                            handleResolutionChange(
                                (e.target as HTMLSelectElement).value,
                            )}
                    >
                        {#each allResolutions as res}
                            <option value="{res.width}x{res.height}"
                                >{res.label}</option
                            >
                        {/each}
                    </select>
                    <span class="select-count"
                        >{allResolutions.length}
                        {allResolutions.length === 1
                            ? "option"
                            : "options"}</span
                    >
                </div>
            </div>

            <!-- Source FPS -->
            <div class="field">
                <label for="fps-select">
                    Source Framerate
                    <span class="help-wrapper">
                        <button
                            class="help-btn"
                            onclick={(e) => {
                                e.stopPropagation();
                                showFpsHelp = !showFpsHelp;
                                showResolutionHelp = false;
                                showStreamSourceHelp = false;
                                showCodecHelp = false;
                                showEncoderHelp = false;
                                if (showFpsHelp) positionTooltip(e);
                            }}
                            onblur={() => (showFpsHelp = false)}
                        >
                            ?
                        </button>
                        {#if showFpsHelp}
                            <span class="help-tooltip" style={tooltipStyle}>
                                Higher framerate produces smoother video. But it
                                increases file size and is more demanding on your system.
                            </span>
                        {/if}
                    </span>
                </label>
                <div class="select-wrapper">
                    <select id="fps-select" bind:value={selectedFps}>
                        {#each availableFps as fps}
                            <option value={fps}>{formatFps(fps)} fps</option>
                        {/each}
                    </select>
                    <span class="select-count"
                        >{availableFps.length}
                        {availableFps.length === 1 ? "option" : "options"}</span
                    >
                </div>
            </div>

            <!-- Source Format -->
            <div class="field">
                <label for="format-select">
                    Source Format
                    <span class="help-wrapper">
                        <button
                            class="help-btn"
                            onclick={(e) => {
                                e.stopPropagation();
                                showStreamSourceHelp = !showStreamSourceHelp;
                                showResolutionHelp = false;
                                showFpsHelp = false;
                                showCodecHelp = false;
                                showEncoderHelp = false;
                                if (showStreamSourceHelp) positionTooltip(e);
                            }}
                            onblur={() => (showStreamSourceHelp = false)}
                        >
                            ?
                        </button>
                        {#if showStreamSourceHelp}
                            <span class="help-tooltip" style={tooltipStyle}>
                                If not sure what to pick,
                                choose the first item in the list.
                                If there's an issue, try the next item and repeat.<br /><br />
                                Formats marked as (raw) tend to be higher quality, but may need a good connection (for example, use USB 3, not
                                USB 2).
                            </span>
                        {/if}
                    </span>
                </label>
                <div class="select-wrapper">
                    <select id="format-select" bind:value={selectedFormat}>
                        {#each availableFormats as fmt}
                            <option value={fmt}
                                >{formatDisplayName(fmt)}{isRawFormat(fmt)
                                    ? " (raw)"
                                    : defaultPassthrough(fmt)
                                      ? " (supports passthrough)"
                                      : ""}</option
                            >
                        {/each}
                    </select>
                    <span class="select-count"
                        >{availableFormats.length}
                        {availableFormats.length === 1
                            ? "option"
                            : "options"}</span
                    >
                </div>
            </div>

            <div class="divider"></div>

            <!-- Encode / Passthrough -->
            <div class="field">
                <div class="radio-group">
                    <label class="radio-label">
                        <input
                            type="radio"
                            name="passthrough"
                            value="encode"
                            checked={!passthrough}
                            disabled={false}
                            onchange={() => (passthrough = false)}
                        />
                        {isSelectedRaw
                            ? "Encode"
                            : "Re-encode"}{!defaultPassthrough(selectedFormat)
                            ? " (Recommended)"
                            : ""}
                    </label>
                    <label
                        class="radio-label"
                        class:radio-disabled={isEncodeOnly}
                    >
                        <input
                            type="radio"
                            name="passthrough"
                            value="passthrough"
                            checked={passthrough}
                            disabled={isEncodeOnly}
                            onchange={() => (passthrough = true)}
                        />
                        Passthrough{defaultPassthrough(selectedFormat) && selectedFormat !== "H264" && selectedFormat !== "VP8"
                            ? " (Recommended)"
                            : ""}
                    </label>
                </div>
                {#if isEncodeOnly}
                    <span class="field-hint"
                        >{selectedFormat} video must be encoded</span
                    >
                {:else if passthrough && selectedFormat === "MJPEG"}
                    <span class="field-hint"
                        ><span class="field-hint warning"
                            >&#9888; MJPEG has high disk usage, which can be
                            saved by re-encoding.</span
                        ></span
                    >
                {:else if passthrough}
                    <span class="field-hint"
                        >Video is recorded to disk directly from the source.</span
                    >
                {:else if !passthrough && selectedFormat === "MJPEG"}
                    <span class="field-hint"
                        >MJPEG source will be re-encoded using the settings
                        below.</span
                    >
                {:else if !passthrough && !isSelectedRaw}
                    <span class="field-hint warning"
                        >Re-encoding may
                        cause quality loss, but may reduce file size.</span
                    >
                {:else}
                    <span class="field-hint"
                        >{selectedFormat} source will be encoded using the settings
                        below.</span
                    >
                {/if}
            </div>

            {#if isEncoding}
                <!-- Encoding Codec -->
                <div class="field">
                    <label for="encoding-codec-select">
                        Video Codec
                        <span class="help-wrapper">
                            <button
                                class="help-btn"
                                onclick={(e) => {
                                    e.stopPropagation();
                                    showCodecHelp = !showCodecHelp;
                                    showResolutionHelp = false;
                                    showFpsHelp = false;
                                    showStreamSourceHelp = false;
                                    showEncoderHelp = false;
                                    if (showCodecHelp) positionTooltip(e);
                                }}
                                onblur={() => (showCodecHelp = false)}
                            >
                                ?
                            </button>
                            {#if showCodecHelp}
                                <span class="help-tooltip" style={tooltipStyle}>
                                    The video codec affects quality, file size, and compatibility with other programs.<br /><br /> Newer codecs (higher in the list) produce smaller files and better quality video, if your system can handle them.
                                </span>
                            {/if}
                        </span>
                    </label>
                    <select
                        id="encoding-codec-select"
                        bind:value={encodingCodec}
                    >
                        {#each availableEncodingCodecs as ec}
                            <option value={ec.codec}
                                >{ec.label}{ec.codec === displayRecommendedCodec
                                    ? " (Recommended)"
                                    : ec.codec === autoSelectedCodec && ec.codec !== displayRecommendedCodec
                                      ? " (Default)"
                                      : ""}</option
                            >
                        {/each}
                    </select>
                </div>

                <!-- Encoder Backend -->
                {#if availableEncoders.length > 0}
                    <div class="field">
                        <label for="encoder-type-select">
                            Encoder
                            <span class="help-wrapper">
                                <button
                                    class="help-btn"
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        showEncoderHelp = !showEncoderHelp;
                                        showResolutionHelp = false;
                                        showFpsHelp = false;
                                        showStreamSourceHelp = false;
                                        showCodecHelp = false;
                                        if (showEncoderHelp) positionTooltip(e);
                                    }}
                                    onblur={() => (showEncoderHelp = false)}
                                >
                                    ?
                                </button>
                                {#if showEncoderHelp}
                                    <span class="help-tooltip" style={tooltipStyle}>
                                        We recommend whatever is auto-selected. Changing this can be more demanding on your system.
                                    </span>
                                {/if}
                            </span>
                        </label>
                        <select
                            id="encoder-type-select"
                            bind:value={encoderType}
                        >
                            {#each availableEncoders as enc}
                                <option value={enc.id}
                                    >{enc.display_name}{enc.id ===
                                        recommendedEncoder &&
                                    enc.is_hardware &&
                                    availableEncoders.length > 1
                                        ? " (Recommended)"
                                        : ""}</option
                                >
                            {/each}
                        </select>
                    </div>
                {/if}

                {#if encoderAvailability}
                    <p class="encoder-info">
                        {#if encoderAvailability.av1.has_hardware || encoderAvailability.vp9.has_hardware}
                            Your device supports hardware acceleration for {[
                                encoderAvailability.av1.has_hardware
                                    ? "AV1"
                                    : null,
                                encoderAvailability.vp9.has_hardware
                                    ? "VP9"
                                    : null,
                                encoderAvailability.vp8.has_hardware
                                    ? "VP8"
                                    : null,
                            ]
                                .filter(Boolean)
                                .join(", ")
                                .replace(/, ([^,]*)$/, " and $1")}. We recommend
                            selecting
                            <strong
                                >{displayRecommendedCodec
                                    ? getCodecDisplayName(displayRecommendedCodec)
                                    : "the default"}</strong
                            >.
                        {:else}
                            AV1 and VP9 produce better video and smaller files
                            if your system can handle it.
                        {/if}
                    </p>
                {/if}

                <button
                    class="advanced-toggle"
                    onclick={() => (showMoreEncoding = !showMoreEncoding)}
                >
                    More
                    <svg
                        class="toggle-chevron"
                        class:open={showMoreEncoding}
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <polyline points="6 9 12 15 18 9"></polyline>
                    </svg>
                </button>
                {#if showMoreEncoding}
                    <!-- Target Resolution -->
                    <div class="field">
                        <label for="target-resolution-select"
                            >Encoding Resolution</label
                        >
                        <select
                            id="target-resolution-select"
                            value="{selectedTargetWidth}x{selectedTargetHeight}"
                            onchange={(e) =>
                                handleTargetResolutionChange(
                                    (e.target as HTMLSelectElement).value,
                                )}
                        >
                            {#each targetResolutions as res}
                                <option value="{res.width}x{res.height}"
                                    >{res.label}{res.width === selectedWidth && res.height === selectedHeight ? ' (Default)' : ''}</option
                                >
                            {/each}
                        </select>
                    </div>

                    <!-- Target FPS -->
                    <div class="field">
                        <label for="target-fps-select">Encoding Framerate</label
                        >
                        <select
                            id="target-fps-select"
                            value={String(selectedTargetFps)}
                            onchange={(e) =>
                                handleTargetFpsChange(
                                    (e.target as HTMLSelectElement).value,
                                )}
                        >
                            {#each targetFramerates as fps}
                                <option value={String(fps)}>{formatFps(fps)} fps{Math.abs(fps - selectedFps) < 0.01 ? ' (Default)' : ''}</option>
                            {/each}
                        </select>
                    </div>

                    <!-- Quality & Effort -->
                    <div class="field">
                        <label for="preset-slider">
                            Encoding Preset: {levelLabels[presetLevel] ?? presetLevel}
                        </label>
                        <input
                            id="preset-slider"
                            type="range"
                            min="1"
                            max="5"
                            step="1"
                            bind:value={presetLevel}
                        />
                        {#if encoderType === "software" && encodingCodec !== "ffv1"}
                            <label for="effort-slider" class="effort-label">
                                Encoding Effort: {levelLabels[effortLevel] ?? effortLevel}
                            </label>
                            <input
                                id="effort-slider"
                                type="range"
                                min="1"
                                max="5"
                                step="1"
                                bind:value={effortLevel}
                            />
                        {/if}
                        <span class="field-hint">
                            {#if encodingCodec === "ffv1"}
                                FFV1 is always lossless, but higher values improve compression.
                            {/if}
                        </span>
                    </div>
                    {#if encodingCodec === "ffv1"}
                        <div class="field">
                            <span class="field-label">Bit Depth</span>
                            <div class="radio-group">
                                <label class="radio-label">
                                    <input
                                        type="radio"
                                        name="bitdepth"
                                        value={8}
                                        checked={videoBitDepth !== 10}
                                        onchange={() => (videoBitDepth = null)}
                                    />
                                    8-bit
                                </label>
                                <label
                                    class="radio-label"
                                    class:radio-disabled={!sourceIs10Bit}
                                    title={!sourceIs10Bit
                                        ? "10-bit requires a 10-bit source format"
                                        : ""}
                                >
                                    <input
                                        type="radio"
                                        name="bitdepth"
                                        value={10}
                                        checked={videoBitDepth === 10}
                                        disabled={!sourceIs10Bit}
                                        onchange={() => (videoBitDepth = 10)}
                                    />
                                    10-bit
                                </label>
                            </div>
                        </div>
                    {/if}
                {/if}
            {/if}
        </div>

        {#if validationError}
            <div class="validation-error">{validationError}</div>
        {/if}

        {#if testResult}
            <div
                class="test-result"
                class:test-pass={testResult.success}
                class:test-warn={testResult.warning}
                class:test-fail={!testResult.success && !testResult.warning}
            >
                {testResult.message}
            </div>
        {/if}
        {#if testError}
            <div class="test-result test-fail">{testError}</div>
        {/if}

        <div class="modal-footer">
            <button
                class="btn-test"
                disabled={testRunning}
                onclick={async () => {
                    testRunning = true;
                    testResult = null;
                    testError = "";
                    try {
                        // Save config first if changed
                        const current = buildConfig();
                        if (current && hasChanges()) {
                            onSave(current);
                            // Brief delay for config to propagate
                            await new Promise((r) => setTimeout(r, 100));
                        }
                        testResult = await testEncoderPreset(device.id);
                    } catch (e: any) {
                        testError =
                            e?.message ?? e?.toString() ?? "Test failed";
                    } finally {
                        testRunning = false;
                    }
                }}
            >
                {testRunning ? "Testing..." : "Test"}
            </button>
            <button class="btn-close" disabled={testRunning} onclick={saveAndClose}> Close </button>
        </div>
        </div>
    </div>
</div>

<style>
    .modal-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.6);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 1000;
        backdrop-filter: blur(2px);
    }

    .modal-content {
        background: #1a1a1a;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 0.5rem;
        width: 100%;
        max-width: 420px;
        box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
    }

    .modal-header {
        padding: 1.25rem 1.5rem 0.75rem;
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
    }

    .header-left {
        display: flex;
        flex-direction: column;
    }

    .modal-header h3 {
        font-family: "Roboto", -apple-system, BlinkMacSystemFont, sans-serif;
        font-size: 1rem;
        font-weight: 500;
        color: #e8e6e3;
        letter-spacing: 0.04em;
        margin: 0 0 0.25rem;
    }

    .device-name-label {
        font-size: 0.75rem;
        color: #6b6b6b;
    }

    .modal-inner {
        position: relative;
    }

    .test-lock-overlay {
        position: absolute;
        inset: 0;
        background: rgba(14, 14, 12, 0.55);
        backdrop-filter: blur(3px);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 50;
    }

    .lock-message {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.75rem;
        color: #8a8a8a;
        font-size: 0.9rem;
        font-weight: 500;
        letter-spacing: 0.03em;
        text-transform: uppercase;
    }

    .lock-message svg {
        color: rgba(219, 187, 116, 0.6);
    }

    .modal-body {
        padding: 1rem 1.5rem;
        display: flex;
        flex-direction: column;
        max-height: 60vh;
        overflow-y: auto;
        gap: 0.875rem;
    }

    .divider {
        height: 1px;
        background: rgba(255, 255, 255, 0.04);
        margin: 0.25rem 0;
    }

    .help-wrapper {
        position: relative;
        display: inline-flex;
        align-items: center;
    }

    .help-btn {
        width: 13px;
        height: 13px;
        padding: 0;
        background: rgba(255, 255, 255, 0.06);
        border: none;
        border-radius: 50%;
        color: #5a5a5a;
        font-size: 0.5625rem;
        font-weight: 500;
        cursor: pointer;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
    }

    .help-btn:hover {
        background: rgba(255, 255, 255, 0.2);
        color: #8a8a8a;
    }

    .help-tooltip {
        position: fixed;
        padding: 0.625rem 0.75rem;
        background: #1a1a1a;
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: 0.5rem;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
        font-size: 0.6875rem;
        color: #a8a8a8;
        line-height: 1.5;
        white-space: normal;
        width: 240px;
        z-index: 1000;
        text-transform: none;
        letter-spacing: normal;
        font-weight: normal;
    }

    .encoder-info {
        margin: 0;
        padding: 0.5rem 0.75rem;
        background: rgba(255, 255, 255, 0.02);
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
        color: #6b6b6b;
        font-size: 0.6875rem;
        line-height: 1.5;
    }

    .encoder-info strong {
        color: #a8a8a8;
    }

    .advanced-toggle {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        background: none;
        border: none;
        color: #6a6a6a;
        font-family: inherit;
        font-size: 0.75rem;
        cursor: pointer;
        padding: 0.25rem 0;
        transition: color 0.15s ease;
        justify-content: center;
        width: 100%;
    }

    .advanced-toggle:hover {
        color: #a8a8a8;
    }

    .toggle-chevron {
        width: 12px;
        height: 12px;
        transition: transform 0.2s ease;
    }

    .toggle-chevron.open {
        transform: rotate(180deg);
    }

    .field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .field label,
    .field-label {
        font-size: 0.6875rem;
        font-weight: 400;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: #8a8a8a;
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .select-wrapper {
        position: relative;
        display: flex;
        align-items: center;
    }

    .select-wrapper select {
        flex: 1;
    }

    .select-count {
        position: absolute;
        right: 1.5rem;
        font-size: 0.6875rem;
        color: #4a4a4a;
        pointer-events: none;
    }

    .field select {
        padding: 0.5rem 0.625rem;
        background: rgba(0, 0, 0, 0.3);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 0.25rem;
        color: #e8e6e3;
        font-family: inherit;
        font-size: 0.8125rem;
        cursor: pointer;
        transition: border-color 0.15s ease;
    }

    .field select:focus {
        outline: none;
        border-color: rgba(201, 169, 98, 0.4);
    }

    .field select:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .field-hint {
        font-size: 0.6875rem;
        color: #5a5a5a;
        line-height: 1.4;
    }

    .field-hint.warning {
        color: #c9a962;
    }

    .radio-group {
        display: flex;
        gap: 1rem;
    }

    .radio-label {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        cursor: pointer;
        font-size: 0.8125rem;
        color: #a8a8a8;
    }

    .radio-disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .radio-label input {
        accent-color: #c9a962;
        width: 14px;
        height: 14px;
        margin: 0;
    }

    .field input[type="range"] {
        -webkit-appearance: none;
        appearance: none;
        width: 100%;
        height: 4px;
        background: rgba(255, 255, 255, 0.08);
        border-radius: 2px;
        outline: none;
        cursor: pointer;
    }

    .field input[type="range"]::-webkit-slider-thumb {
        -webkit-appearance: none;
        appearance: none;
        width: 16px;
        height: 16px;
        border-radius: 50%;
        background: #c9a962;
        cursor: pointer;
        border: 2px solid rgba(0, 0, 0, 0.3);
        transition: transform 0.1s ease;
    }

    .field input[type="range"]::-webkit-slider-thumb:hover {
        transform: scale(1.15);
    }

    .field input[type="range"]::-moz-range-thumb {
        width: 16px;
        height: 16px;
        border-radius: 50%;
        background: #c9a962;
        cursor: pointer;
        border: 2px solid rgba(0, 0, 0, 0.3);
    }

    .effort-label {
        margin-top: 0.5rem;
    }

    .test-result {
        margin: 0 1.5rem;
        padding: 0.5rem 0.75rem;
        border-radius: 6px;
        font-size: 0.8rem;
        line-height: 1.4;
    }

    .test-pass {
        background: rgba(34, 197, 94, 0.12);
        color: #4ade80;
    }

    .test-warn {
        background: rgba(234, 179, 8, 0.12);
        color: #facc15;
    }

    .test-fail {
        background: rgba(220, 38, 38, 0.12);
        color: #f87171;
    }

    .btn-test {
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.1);
        color: #b0b0b0;
        padding: 0.5rem 1rem;
        border-radius: 0.375rem;
        font-family: inherit;
        font-size: 0.8125rem;
        cursor: pointer;
        transition: all 0.15s;
    }

    .btn-test:hover:not(:disabled) {
        background: rgba(255, 255, 255, 0.1);
        color: #e0e0e0;
    }

    .btn-test:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .validation-error {
        margin: 0 1.5rem;
        padding: 0.5rem 0.75rem;
        border-radius: 6px;
        background: rgba(220, 38, 38, 0.12);
        color: #f87171;
        font-size: 0.8rem;
        line-height: 1.4;
    }

    .modal-footer {
        padding: 0.75rem 1.5rem 1.25rem;
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        border-top: 1px solid rgba(255, 255, 255, 0.04);
    }

    .btn-close {
        padding: 0.5rem 1rem;
        border-radius: 0.25rem;
        font-family: inherit;
        font-size: 0.75rem;
        letter-spacing: 0.02em;
        cursor: pointer;
        transition: all 0.15s ease;
        background: transparent;
        border: 1px solid rgba(255, 255, 255, 0.08);
        color: #8a8a8a;
    }

    .btn-close:hover {
        border-color: rgba(255, 255, 255, 0.15);
        color: #a8a8a8;
    }

    /* Light mode */
    :global(body.light-mode) .modal-content {
        background: #f5f5f5;
        border-color: rgba(0, 0, 0, 0.12);
    }

    :global(body.light-mode) .modal-header {
        border-bottom-color: rgba(0, 0, 0, 0.06);
    }

    :global(body.light-mode) .modal-header h3 {
        color: #2a2a2a;
    }

    :global(body.light-mode) .device-name-label {
        color: #7a7a7a;
    }

    :global(body.light-mode) .field label,
    :global(body.light-mode) .field-label {
        color: #5a5a5a;
    }

    :global(body.light-mode) .select-count {
        color: #b0b0b0;
    }

    :global(body.light-mode) .field select {
        background: rgba(255, 255, 255, 0.9);
        border-color: rgba(0, 0, 0, 0.12);
        color: #2a2a2a;
    }

    :global(body.light-mode) .field select:focus {
        border-color: rgba(160, 128, 48, 0.5);
    }

    :global(body.light-mode) .field-hint {
        color: #8a8a8a;
    }

    :global(body.light-mode) .field-hint.warning {
        color: #8a6a20;
    }

    :global(body.light-mode) .field input[type="range"] {
        background: rgba(0, 0, 0, 0.1);
    }

    :global(body.light-mode) .field input[type="range"]::-webkit-slider-thumb {
        background: #a08030;
        border-color: rgba(255, 255, 255, 0.5);
    }

    :global(body.light-mode) .field input[type="range"]::-moz-range-thumb {
        background: #a08030;
        border-color: rgba(255, 255, 255, 0.5);
    }

    :global(body.light-mode) .test-pass {
        background: rgba(34, 197, 94, 0.1);
        color: #16a34a;
    }

    :global(body.light-mode) .test-warn {
        background: rgba(234, 179, 8, 0.1);
        color: #a16207;
    }

    :global(body.light-mode) .test-fail {
        background: rgba(220, 38, 38, 0.08);
        color: #dc2626;
    }

    :global(body.light-mode) .btn-test {
        background: rgba(0, 0, 0, 0.04);
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode) .btn-test:hover:not(:disabled) {
        background: rgba(0, 0, 0, 0.08);
        color: #3a3a3a;
    }

    :global(body.light-mode) .divider {
        background: rgba(0, 0, 0, 0.06);
    }

    :global(body.light-mode) .validation-error {
        background: rgba(220, 38, 38, 0.08);
        color: #dc2626;
    }

    :global(body.light-mode) .modal-footer {
        border-top-color: rgba(0, 0, 0, 0.06);
    }

    :global(body.light-mode) .test-lock-overlay {
        background: rgba(245, 245, 243, 0.6);
    }

    :global(body.light-mode) .lock-message {
        color: #6a6a6a;
    }

    :global(body.light-mode) .lock-message svg {
        color: rgba(160, 128, 48, 0.6);
    }

    :global(body.light-mode) .encoder-info {
        background: rgba(0, 0, 0, 0.03);
        border-color: rgba(0, 0, 0, 0.1);
        color: #5a5a5a;
    }

    :global(body.light-mode) .encoder-info strong {
        color: #3a3a3a;
    }

    :global(body.light-mode) .help-btn {
        background: rgba(0, 0, 0, 0.08);
        color: #7a7a7a;
    }

    :global(body.light-mode) .help-btn:hover {
        background: rgba(0, 0, 0, 0.12);
        color: #4a4a4a;
    }

    :global(body.light-mode) .help-tooltip {
        background: #f5f5f5;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
        color: #5a5a5a;
    }

    :global(body.light-mode) .advanced-toggle {
        color: #7a7a7a;
    }

    :global(body.light-mode) .advanced-toggle:hover {
        color: #4a4a4a;
    }

    :global(body.light-mode) .btn-close {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }
</style>
