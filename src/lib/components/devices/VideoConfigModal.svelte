<script lang="ts">
    import type {
        VideoDevice,
        VideoDeviceConfig,
        VideoCodec,
        HardwareEncoderType,
        CodecCapability,
        EncoderAvailability,
    } from "$lib/api";
    import {
        getCodecDisplayName,
        getResolutionLabel,
        getTargetResolutions,
        getTargetFramerates,
        formatFps,
        computeDefaultConfig,
        validateVideoDeviceConfig,
        getEncoderAvailability,
        autoSelectEncoderPreset,
        PASSTHROUGH_ONLY_CODECS,
        ENCODE_ONLY_CODECS,
    } from "$lib/api";
    interface Props {
        device: VideoDevice;
        currentConfig: VideoDeviceConfig | null;
        onSave: (config: VideoDeviceConfig) => void;
        onClose: () => void;
    }

    let { device, currentConfig, onSave, onClose }: Props = $props();

    // Compute effective config: saved config or smart defaults
    const effectiveConfig = currentConfig ?? computeDefaultConfig(device);

    // Available codecs for this device
    const availableCodecs = $derived(device.supported_codecs);

    // State for selections
    let selectedCodec = $state<VideoCodec>(
        effectiveConfig?.source_codec ?? availableCodecs[0] ?? "raw",
    );
    let selectedWidth = $state<number>(effectiveConfig?.source_width ?? 0);
    let selectedHeight = $state<number>(effectiveConfig?.source_height ?? 0);
    let selectedFps = $state<number>(effectiveConfig?.source_fps ?? 0);
    // 0 = "Match Source" sentinel
    let selectedTargetWidth = $state<number>(
        effectiveConfig?.target_width ?? 0,
    );
    let selectedTargetHeight = $state<number>(
        effectiveConfig?.target_height ?? 0,
    );
    let selectedTargetFps = $state<number>(effectiveConfig?.target_fps ?? 0);

    // Encoding settings (per-device)
    let passthrough = $state<boolean>(effectiveConfig?.passthrough ?? true);
    let encodingCodec = $state<VideoCodec | null>(
        effectiveConfig?.encoding_codec ?? null,
    );
    let encoderType = $state<HardwareEncoderType | null>(
        effectiveConfig?.encoder_type ?? null,
    );
    let presetLevel = $state<number>(effectiveConfig?.preset_level ?? 3);
    let encoderAvailability = $state<EncoderAvailability | null>(null);

    // Auto-select state
    let autoSelectRunning = $state(false);
    let autoSelectProgress = $state("");
    let autoSelectError = $state("");

    // More encoding options revealer
    let showMoreEncoding = $state(false);

    // Stream source help tooltip
    let showStreamSourceHelp = $state(false);

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
                    const info =
                        a[
                            encodingCodec as keyof Pick<
                                EncoderAvailability,
                                "av1" | "vp9" | "vp8" | "ffv1"
                            >
                        ];
                    if (info?.recommended) {
                        encoderType = info.recommended as HardwareEncoderType;
                    }
                }
            })
            .catch(() => {});
    });

    // Get capabilities for the selected codec
    const codecCaps = $derived<CodecCapability[]>(
        device.capabilities[selectedCodec] ?? [],
    );

    // Available resolutions for selected codec (sorted highest first)
    const availableResolutions = $derived(
        codecCaps.map((c) => ({
            width: c.width,
            height: c.height,
            label: getResolutionLabel(c.width, c.height),
        })),
    );

    // Available FPS for selected codec + resolution
    const availableFps = $derived(
        codecCaps.find(
            (c) => c.width === selectedWidth && c.height === selectedHeight,
        )?.framerates ?? [],
    );

    // Whether this codec is passthrough-only (no encoding/decoding available)
    const isPassthroughOnly = $derived(
        PASSTHROUGH_ONLY_CODECS.includes(selectedCodec),
    );

    // Whether this codec requires encoding (cannot passthrough, e.g. raw → MKV)
    const isEncodeOnly = $derived(
        ENCODE_ONLY_CODECS.includes(selectedCodec),
    );

    // Whether encoding settings are active (not passthrough)
    const isEncoding = $derived(!passthrough);

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
        if (encoderAvailability.ffv1.available)
            codecs.push({
                codec: "ffv1",
                label: "FFV1 (Lossless, huge files)",
            });
        return codecs;
    });

    // Available encoder backends for the selected encoding codec
    const availableEncoders = $derived.by(() => {
        if (!encoderAvailability || !encodingCodec) return [];
        const info =
            encoderAvailability[
                encodingCodec as keyof Pick<
                    EncoderAvailability,
                    "av1" | "vp9" | "vp8" | "ffv1"
                >
            ];
        return info?.encoders ?? [];
    });

    // When encoding codec changes, always select the recommended encoder for that codec.
    // Force reactivity via null intermediate in case the recommended value is the same
    // (e.g., both codecs only have Software).
    let lastCodecForEncoder = encodingCodec;
    $effect(() => {
        const codec = encodingCodec;
        if (!codec || !encoderAvailability) return;
        if (codec !== lastCodecForEncoder) {
            lastCodecForEncoder = codec;
            const info =
                encoderAvailability[
                    codec as keyof Pick<
                        EncoderAvailability,
                        "av1" | "vp9" | "vp8" | "ffv1"
                    >
                ];
            if (info?.recommended) {
                const rec = info.recommended as HardwareEncoderType;
                encoderType = null;
                // Use microtask to ensure Svelte processes the null before the real value
                queueMicrotask(() => {
                    encoderType = rec;
                });
            }
        }
    });

    // Track the backend-recommended values for labeling
    const recommendedCodec = $derived<VideoCodec | null>(
        encoderAvailability
            ? (encoderAvailability.recommended_codec as VideoCodec)
            : null,
    );
    const recommendedEncoder = $derived.by(() => {
        if (!encoderAvailability || !encodingCodec) return null;
        const info =
            encoderAvailability[
                encodingCodec as keyof Pick<
                    EncoderAvailability,
                    "av1" | "vp9" | "vp8" | "ffv1"
                >
            ];
        return info?.recommended ?? null;
    });

    const presetLabels: Record<number, string> = {
        1: "Lightest",
        2: "Light",
        3: "Balanced",
        4: "Heavy",
        5: "Heaviest",
    };

    // When codec changes, set passthrough default: passthrough-only codecs are forced,
    // raw/mjpeg = encode, all others = passthrough
    let lastCodecForPassthrough = selectedCodec;
    $effect(() => {
        if (selectedCodec !== lastCodecForPassthrough) {
            lastCodecForPassthrough = selectedCodec;
            if (PASSTHROUGH_ONLY_CODECS.includes(selectedCodec)) {
                passthrough = true;
            } else if (ENCODE_ONLY_CODECS.includes(selectedCodec)) {
                passthrough = false;
            } else {
                passthrough = selectedCodec !== "mjpeg";
            }
        }
    });

    // Initialize defaults when codec changes
    $effect(() => {
        const caps = device.capabilities[selectedCodec] ?? [];
        if (caps.length > 0) {
            const hasMatch = caps.some(
                (c) => c.width === selectedWidth && c.height === selectedHeight,
            );
            if (!hasMatch) {
                selectedWidth = caps[0].width;
                selectedHeight = caps[0].height;
            }
        }
    });

    // Update FPS when resolution changes
    $effect(() => {
        const cap = codecCaps.find(
            (c) => c.width === selectedWidth && c.height === selectedHeight,
        );
        if (cap) {
            const fpsOptions = cap.framerates;
            // Check if current selection is close to any available option
            const hasClose = fpsOptions.some(
                (f) => Math.abs(f - selectedFps) < 0.01,
            );
            if (fpsOptions.length > 0 && !hasClose) {
                selectedFps = fpsOptions[0]; // Pick highest available
            }
        }
    });

    // When source changes while "Match Source" is active, keep it as "Match Source"
    // When source changes with a specific target, validate it still makes sense
    $effect(() => {
        if (passthrough) {
            // Passthrough: target is always ignored, keep sentinels
            selectedTargetWidth = 0;
            selectedTargetHeight = 0;
            selectedTargetFps = 0;
        }
        // For encoding: if target is non-zero and exceeds source, reset to Match Source
        if (isEncoding && selectedTargetWidth !== 0) {
            if (
                selectedTargetWidth > selectedWidth ||
                selectedTargetHeight > selectedHeight
            ) {
                selectedTargetWidth = 0;
                selectedTargetHeight = 0;
            }
        }
        if (isEncoding && selectedTargetFps !== 0) {
            if (selectedTargetFps > selectedFps + 0.5) {
                selectedTargetFps = 0;
            }
        }
    });

    function handleResolutionChange(value: string) {
        const [w, h] = value.split("x").map(Number);
        selectedWidth = w;
        selectedHeight = h;
    }

    function handleTargetResolutionChange(value: string) {
        if (value === "match") {
            selectedTargetWidth = 0;
            selectedTargetHeight = 0;
        } else {
            const [w, h] = value.split("x").map(Number);
            selectedTargetWidth = w;
            selectedTargetHeight = h;
        }
    }

    function handleTargetFpsChange(value: string) {
        if (value === "match") {
            selectedTargetFps = 0;
        } else {
            selectedTargetFps = Number(value);
        }
    }

    let validationError = $state<string | null>(null);

    /** Build the current config from UI state */
    function buildConfig(): VideoDeviceConfig {
        return {
            source_codec: selectedCodec,
            source_width: selectedWidth,
            source_height: selectedHeight,
            source_fps: selectedFps,
            passthrough,
            encoding_codec: encodingCodec,
            encoder_type: encoderType,
            preset_level: presetLevel,
            target_width: isEncoding ? selectedTargetWidth : 0,
            target_height: isEncoding ? selectedTargetHeight : 0,
            target_fps: isEncoding ? selectedTargetFps : 0,
        };
    }

    /** Check if the config has changed from what was loaded */
    function hasChanges(): boolean {
        if (!effectiveConfig) return true;
        const current = buildConfig();
        return (
            current.source_codec !== effectiveConfig.source_codec ||
            current.source_width !== effectiveConfig.source_width ||
            current.source_height !== effectiveConfig.source_height ||
            Math.abs(current.source_fps - effectiveConfig.source_fps) > 0.01 ||
            current.passthrough !== effectiveConfig.passthrough ||
            current.encoding_codec !== effectiveConfig.encoding_codec ||
            current.encoder_type !== effectiveConfig.encoder_type ||
            current.preset_level !== effectiveConfig.preset_level ||
            current.target_width !== effectiveConfig.target_width ||
            current.target_height !== effectiveConfig.target_height ||
            Math.abs(current.target_fps - effectiveConfig.target_fps) > 0.01
        );
    }

    /** Save (if changed) and close the modal */
    async function saveAndClose() {
        if (hasChanges()) {
            validationError = null;
            const valid = await validateVideoDeviceConfig(
                device.id,
                selectedCodec,
                selectedWidth,
                selectedHeight,
                selectedFps,
            );
            if (!valid) {
                validationError = `This device does not support ${selectedWidth}x${selectedHeight} @ ${formatFps(selectedFps)}fps with ${getCodecDisplayName(selectedCodec)}. Try a different combination.`;
                return; // Don't close — let the user fix the config
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
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-content" onclick={(e) => e.stopPropagation()}>
        <div class="modal-header">
            <div class="header-left">
                <h3>Configure Video Source</h3>
                <span class="device-name-label">{device.name}</span>
            </div>
        </div>

        <div class="modal-body">
            <!-- Source Codec -->
            <div class="field">
                <label for="codec-select">
                    Source Codec
                    <span class="help-wrapper">
                        <button
                            class="help-btn"
                            onclick={(e) => {
                                e.stopPropagation();
                                showStreamSourceHelp = !showStreamSourceHelp;
                            }}
                            onblur={() => (showStreamSourceHelp = false)}
                        >
                            ?
                        </button>
                        {#if showStreamSourceHelp}
                            <span class="help-tooltip">
                                We detect that your video device can deliver its
                                video stream pre-encoded with the codecs in the
                                dropdown menu. <br /><br />RAW streams are most
                                reliable over high-speed connections.
                            </span>
                        {/if}
                    </span>
                </label>
                <select id="codec-select" bind:value={selectedCodec}>
                    {#each availableCodecs as codec}
                        <option value={codec}
                            >{getCodecDisplayName(codec)}</option
                        >
                    {/each}
                </select>
            </div>

            <!-- Source Resolution -->
            <div class="field">
                <label for="resolution-select">Source Resolution</label>
                <select
                    id="resolution-select"
                    value="{selectedWidth}x{selectedHeight}"
                    onchange={(e) =>
                        handleResolutionChange(
                            (e.target as HTMLSelectElement).value,
                        )}
                >
                    {#each availableResolutions as res}
                        <option value="{res.width}x{res.height}"
                            >{res.label}</option
                        >
                    {/each}
                </select>
            </div>

            <!-- Source FPS -->
            <div class="field">
                <label for="fps-select">Source Framerate</label>
                <select id="fps-select" bind:value={selectedFps}>
                    {#each availableFps as fps}
                        <option value={fps}>{formatFps(fps)} fps</option>
                    {/each}
                </select>
            </div>

            <div class="divider"></div>

            <!-- Encode / Passthrough -->
            <div class="field">
                <div class="radio-group">
                    <label
                        class="radio-label"
                        class:radio-disabled={isPassthroughOnly}
                    >
                        <input
                            type="radio"
                            name="passthrough"
                            value="encode"
                            checked={!passthrough}
                            disabled={isPassthroughOnly}
                            onchange={() => (passthrough = false)}
                        />
                        {selectedCodec === "raw"
                            ? "Encode (Recommended)"
                            : selectedCodec === "mjpeg"
                              ? "Re-encode (Recommended)"
                              : "Re-encode"}
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
                        Passthrough{selectedCodec !== "raw" &&
                        selectedCodec !== "mjpeg" &&
                        !isPassthroughOnly
                            ? " (Recommended)"
                            : ""}
                    </label>
                </div>
                {#if isPassthroughOnly}
                    <span class="field-hint">
                        <span class="warning-icon">&#9888;</span>
                        {getCodecDisplayName(selectedCodec)} is a proprietary codec
                        and will be recorded as passthrough only. In-app playback
                        is not supported.
                    </span>
                {:else if isEncodeOnly}
                    <span class="field-hint"
                        >RAW video cannot be stored as passthrough and must be
                        encoded.</span
                    >
                {:else if passthrough && selectedCodec === "mjpeg"}
                    <span class="field-hint"
                        ><span class="warning-icon">&#9888;</span> MJPEG is recorded
                        directly from the source. MJPEG uses significant disk space,
                        which can be saved by re-encoding.</span
                    >
                {:else if passthrough}
                    <span class="field-hint"
                        >Video is recorded directly from the source without
                        re-encoding.</span
                    >
                {:else if !passthrough && selectedCodec === "mjpeg"}
                    <span class="field-hint"
                        >MJPEG source will be re-encoded using the settings
                        below.</span
                    >
                {:else if !passthrough && selectedCodec !== "raw"}
                    <span class="field-hint warning"
                        >Re-encoding an already-compressed stream causes quality
                        loss. Use this only if you need to change resolution or
                        frame rate.</span
                    >
                {:else}
                    <span class="field-hint"
                        >RAW source will be encoded using the settings below.</span
                    >
                {/if}
            </div>

            {#if isEncoding}
                <!-- Encoding Codec -->
                <div class="field">
                    <label for="encoding-codec-select">Video Codec</label>
                    <select
                        id="encoding-codec-select"
                        bind:value={encodingCodec}
                    >
                        {#each availableEncodingCodecs as ec}
                            <option value={ec.codec}
                                >{ec.label}{ec.codec === recommendedCodec
                                    ? " (Recommended)"
                                    : ""}</option
                            >
                        {/each}
                    </select>
                </div>

                <!-- Encoder Backend -->
                {#if availableEncoders.length > 0}
                    <div class="field">
                        <label for="encoder-type-select">Encoder</label>
                        <select
                            id="encoder-type-select"
                            bind:value={encoderType}
                        >
                            {#each availableEncoders as enc}
                                <option value={enc.id}
                                    >{enc.display_name}{enc.id ===
                                        recommendedEncoder && enc.is_hardware
                                        ? " (Recommended)"
                                        : ""}</option
                                >
                            {/each}
                        </select>
                    </div>
                {/if}

                {#if encoderAvailability}
                    <p class="encoder-info">
                        {#if encoderAvailability.av1.has_hardware || encoderAvailability.vp9.has_hardware || encoderAvailability.vp8.has_hardware}
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
                            <strong
                                >{recommendedCodec
                                    ? getCodecDisplayName(recommendedCodec)
                                    : "the default"}</strong
                            > for the best experience.
                        {:else}
                            Your system does not support hardware acceleration
                            for the available codecs. If you experience
                            choppiness, try reducing resolution or framerate.
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
                            value={selectedTargetWidth === 0
                                ? "match"
                                : `${selectedTargetWidth}x${selectedTargetHeight}`}
                            onchange={(e) =>
                                handleTargetResolutionChange(
                                    (e.target as HTMLSelectElement).value,
                                )}
                        >
                            <option value="match">Match Source</option>
                            {#each targetResolutions as res}
                                <option value="{res.width}x{res.height}"
                                    >{res.label}</option
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
                            value={selectedTargetFps === 0
                                ? "match"
                                : String(selectedTargetFps)}
                            onchange={(e) =>
                                handleTargetFpsChange(
                                    (e.target as HTMLSelectElement).value,
                                )}
                        >
                            <option value="match">Match Source</option>
                            {#each targetFramerates as fps}
                                <option value={String(fps)}>{fps} fps</option>
                            {/each}
                        </select>
                    </div>

                    <!-- Preset Level -->
                    <div class="field">
                        <label for="preset-slider">
                            Quality Preset: {presetLabels[presetLevel] ??
                                presetLevel}
                        </label>
                        <input
                            id="preset-slider"
                            type="range"
                            min="1"
                            max="5"
                            step="1"
                            bind:value={presetLevel}
                        />
                        <div class="preset-range-labels">
                            <span>Lightest</span>
                            <span>Heaviest</span>
                        </div>
                        <span class="field-hint">
                            {#if encodingCodec === "ffv1"}
                                {#if presetLevel <= 3}
                                    Faster encoding, larger files. FFV1 quality
                                    is always lossless.
                                {:else}
                                    Slower encoding, smaller files. FFV1 quality
                                    is always lossless.
                                {/if}
                            {:else if presetLevel <= 3}
                                Smaller files. Smoother recordings on less
                                powerful systems.
                            {:else}
                                Larger files. Higher quality video. Works best
                                on more powerful systems.
                            {/if}
                        </span>
                    </div>
                {/if}
            {/if}
        </div>

        {#if validationError}
            <div class="validation-error">{validationError}</div>
        {/if}

        <div class="modal-footer">
            <button class="btn-close" onclick={saveAndClose}> Close </button>
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
        font-family: "Bebas Neue", Impact, "Arial Narrow", sans-serif;
        font-size: 1.125rem;
        font-weight: 400;
        color: #e8e6e3;
        letter-spacing: 0.04em;
        margin: 0 0 0.25rem;
    }

    .device-name-label {
        font-size: 0.75rem;
        color: #6b6b6b;
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
        position: absolute;
        top: 100%;
        left: 50%;
        transform: translateX(-50%);
        margin-top: 0.5rem;
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
        z-index: 10;
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

    .field label {
        font-size: 0.6875rem;
        font-weight: 400;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: #8a8a8a;
        display: flex;
        align-items: center;
        gap: 0.5rem;
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

    .warning-icon {
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

    .preset-range-labels {
        display: flex;
        justify-content: space-between;
        font-size: 0.625rem;
        color: #5a5a5a;
    }

    .field-disabled {
        opacity: 0.5;
    }

    .badge {
        display: inline-block;
        padding: 0.0625rem 0.375rem;
        background: rgba(201, 169, 98, 0.12);
        border-radius: 0.125rem;
        font-size: 0.5625rem;
        color: #c9a962;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        font-weight: 500;
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

    :global(body.light-mode) .field label {
        color: #5a5a5a;
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

    :global(body.light-mode) .checkbox-label input {
        accent-color: #a08030;
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

    :global(body.light-mode) .preset-range-labels {
        color: #7a7a7a;
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

    :global(body.light-mode) .badge {
        background: rgba(160, 128, 48, 0.12);
        color: #8a6a20;
    }
</style>
