<script lang="ts">
  import type { VideoDevice, VideoDeviceConfig, VideoCodec, CodecCapability } from '$lib/api';
  import { getCodecDisplayName, getResolutionLabel, getTargetResolutions, getTargetFramerates, formatFps, computeDefaultConfig } from '$lib/api';
  import { settings } from '$lib/stores/settings';

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
  let selectedCodec = $state<VideoCodec>(effectiveConfig?.source_codec ?? availableCodecs[0] ?? 'raw');
  let selectedWidth = $state<number>(effectiveConfig?.source_width ?? 0);
  let selectedHeight = $state<number>(effectiveConfig?.source_height ?? 0);
  let selectedFps = $state<number>(effectiveConfig?.source_fps ?? 0);
  // 0 = "Match Source" sentinel
  let selectedTargetWidth = $state<number>(effectiveConfig?.target_width ?? 0);
  let selectedTargetHeight = $state<number>(effectiveConfig?.target_height ?? 0);
  let selectedTargetFps = $state<number>(effectiveConfig?.target_fps ?? 0);

  // Get capabilities for the selected codec
  const codecCaps = $derived<CodecCapability[]>(device.capabilities[selectedCodec] ?? []);

  // Available resolutions for selected codec (sorted highest first)
  const availableResolutions = $derived(
    codecCaps.map(c => ({ width: c.width, height: c.height, label: getResolutionLabel(c.width, c.height) }))
  );

  // Available FPS for selected codec + resolution
  const availableFps = $derived(
    (codecCaps.find(c => c.width === selectedWidth && c.height === selectedHeight))?.framerates ?? []
  );

  // Whether this is a raw (encoding needed) codec
  const isRawCodec = $derived(selectedCodec === 'raw');

  // Target resolutions (only relevant for raw): "Match Source" + common resolutions
  const targetResolutions = $derived(
    isRawCodec ? getTargetResolutions(selectedWidth, selectedHeight) : []
  );

  // Target framerates (only relevant for raw): common values ≤ source fps
  const targetFramerates = $derived(
    isRawCodec ? getTargetFramerates(selectedFps) : []
  );

  // Encoding mode label for display
  const encodingModeLabel = $derived.by(() => {
    const mode = $settings?.video_encoding_mode;
    switch (mode) {
      case 'av1': return 'AV1';
      case 'vp9': return 'VP9';
      case 'vp8': return 'VP8';
      default: return 'VP8';
    }
  });

  // Initialize defaults when codec changes
  $effect(() => {
    const caps = device.capabilities[selectedCodec] ?? [];
    if (caps.length > 0) {
      const hasMatch = caps.some(c => c.width === selectedWidth && c.height === selectedHeight);
      if (!hasMatch) {
        selectedWidth = caps[0].width;
        selectedHeight = caps[0].height;
      }
    }
  });

  // Update FPS when resolution changes
  $effect(() => {
    const cap = codecCaps.find(c => c.width === selectedWidth && c.height === selectedHeight);
    if (cap) {
      const fpsOptions = cap.framerates;
      // Check if current selection is close to any available option
      const hasClose = fpsOptions.some(f => Math.abs(f - selectedFps) < 0.01);
      if (fpsOptions.length > 0 && !hasClose) {
        selectedFps = fpsOptions[0]; // Pick highest available
      }
    }
  });

  // When source changes while "Match Source" is active, keep it as "Match Source"
  // When source changes with a specific target, validate it still makes sense
  $effect(() => {
    if (!isRawCodec) {
      // Passthrough: target is always ignored, keep sentinels
      selectedTargetWidth = 0;
      selectedTargetHeight = 0;
      selectedTargetFps = 0;
    }
    // For raw: if target is non-zero and exceeds source, reset to Match Source
    if (isRawCodec && selectedTargetWidth !== 0) {
      if (selectedTargetWidth > selectedWidth || selectedTargetHeight > selectedHeight) {
        selectedTargetWidth = 0;
        selectedTargetHeight = 0;
      }
    }
    if (isRawCodec && selectedTargetFps !== 0) {
      if (selectedTargetFps > selectedFps + 0.5) {
        selectedTargetFps = 0;
      }
    }
  });

  function handleResolutionChange(value: string) {
    const [w, h] = value.split('x').map(Number);
    selectedWidth = w;
    selectedHeight = h;
  }

  function handleTargetResolutionChange(value: string) {
    if (value === 'match') {
      selectedTargetWidth = 0;
      selectedTargetHeight = 0;
    } else {
      const [w, h] = value.split('x').map(Number);
      selectedTargetWidth = w;
      selectedTargetHeight = h;
    }
  }

  function handleTargetFpsChange(value: string) {
    if (value === 'match') {
      selectedTargetFps = 0;
    } else {
      selectedTargetFps = Number(value);
    }
  }

  function handleSave() {
    const config: VideoDeviceConfig = {
      source_codec: selectedCodec,
      source_width: selectedWidth,
      source_height: selectedHeight,
      source_fps: selectedFps,
      target_width: isRawCodec ? selectedTargetWidth : 0,
      target_height: isRawCodec ? selectedTargetHeight : 0,
      target_fps: isRawCodec ? selectedTargetFps : 0,
    };
    onSave(config);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="modal-overlay" onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal-content" onclick={(e) => e.stopPropagation()}>
    <div class="modal-header">
      <h3>Configure Video Source</h3>
      <span class="device-name-label">{device.name}</span>
    </div>

    <div class="modal-body">
      <!-- Source Codec -->
      <div class="field">
        <label for="codec-select">Stream Type</label>
        <select id="codec-select" bind:value={selectedCodec}>
          {#each availableCodecs as codec}
            <option value={codec}>{getCodecDisplayName(codec)}</option>
          {/each}
        </select>
        {#if !isRawCodec}
          <span class="field-hint">Passthrough — video is recorded directly from the source without re-encoding. If the stream type is MJPEG, the quality may not be as good and files may be very large.</span>
        {:else}
          <span class="field-hint">Raw video will be encoded using {encodingModeLabel} (configured in Settings).</span>
        {/if}
      </div>

      <!-- Source Resolution -->
      <div class="field">
        <label for="resolution-select">Source Resolution</label>
        <select 
          id="resolution-select" 
          value="{selectedWidth}x{selectedHeight}"
          onchange={(e) => handleResolutionChange((e.target as HTMLSelectElement).value)}
        >
          {#each availableResolutions as res}
            <option value="{res.width}x{res.height}">{res.label}</option>
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

      <!-- Target Resolution -->
      <div class="field" class:field-disabled={!isRawCodec}>
        <label for="target-resolution-select">
          Encoding Resolution
          {#if !isRawCodec}
            <span class="badge">Passthrough</span>
          {/if}
        </label>
        <select 
          id="target-resolution-select" 
          disabled={!isRawCodec}
          value={selectedTargetWidth === 0 ? 'match' : `${selectedTargetWidth}x${selectedTargetHeight}`}
          onchange={(e) => handleTargetResolutionChange((e.target as HTMLSelectElement).value)}
        >
          {#if isRawCodec}
            <option value="match">Match Source</option>
            {#each targetResolutions as res}
              <option value="{res.width}x{res.height}">{res.label}</option>
            {/each}
          {:else}
            <option value="match">Match Source</option>
          {/if}
        </select>
        {#if !isRawCodec}
          <span class="field-hint">Not available in passthrough mode — video is recorded as-is from the source.</span>
        {/if}
      </div>

      <!-- Target FPS -->
      <div class="field" class:field-disabled={!isRawCodec}>
        <label for="target-fps-select">
          Encoding Framerate
          {#if !isRawCodec}
            <span class="badge">Passthrough</span>
          {/if}
        </label>
        <select 
          id="target-fps-select" 
          disabled={!isRawCodec}
          value={selectedTargetFps === 0 ? 'match' : String(selectedTargetFps)}
          onchange={(e) => handleTargetFpsChange((e.target as HTMLSelectElement).value)}
        >
          {#if isRawCodec}
            <option value="match">Match Source</option>
            {#each targetFramerates as fps}
              <option value={String(fps)}>{fps} fps</option>
            {/each}
          {:else}
            <option value="match">Match Source</option>
          {/if}
        </select>
      </div>
    </div>

    <div class="modal-footer">
      <button class="btn-secondary" onclick={onClose}>Cancel</button>
      <button class="btn-primary" onclick={handleSave}>Save</button>
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
  }

  .modal-header h3 {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
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
    gap: 0.875rem;
  }

  .divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.04);
    margin: 0.25rem 0;
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

  .modal-footer {
    padding: 0.75rem 1.5rem 1.25rem;
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
  }

  .btn-secondary,
  .btn-primary {
    padding: 0.5rem 1rem;
    border-radius: 0.25rem;
    font-family: inherit;
    font-size: 0.75rem;
    letter-spacing: 0.02em;
    cursor: pointer;
    transition: all 0.15s ease;
    border: none;
  }

  .btn-secondary {
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: #8a8a8a;
  }

  .btn-secondary:hover {
    border-color: rgba(255, 255, 255, 0.15);
    color: #a8a8a8;
  }

  .btn-primary {
    background: rgba(201, 169, 98, 0.15);
    color: #c9a962;
    border: 1px solid rgba(201, 169, 98, 0.3);
  }

  .btn-primary:hover {
    background: rgba(201, 169, 98, 0.25);
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

  :global(body.light-mode) .divider {
    background: rgba(0, 0, 0, 0.06);
  }

  :global(body.light-mode) .modal-footer {
    border-top-color: rgba(0, 0, 0, 0.06);
  }

  :global(body.light-mode) .btn-secondary {
    border-color: rgba(0, 0, 0, 0.12);
    color: #5a5a5a;
  }

  :global(body.light-mode) .btn-primary {
    background: rgba(160, 128, 48, 0.15);
    color: #8a6a20;
    border-color: rgba(160, 128, 48, 0.3);
  }

  :global(body.light-mode) .badge {
    background: rgba(160, 128, 48, 0.12);
    color: #8a6a20;
  }
</style>
