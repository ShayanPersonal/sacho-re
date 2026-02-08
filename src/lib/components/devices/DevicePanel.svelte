<script lang="ts">
  import {
    audioDevices,
    midiDevices,
    videoDevices,
    selectedAudioDevices,
    selectedMidiDevices,
    triggerMidiDevices,
    selectedVideoDevices,
    videoDeviceCodecs,
    audioDeviceCount,
    midiDeviceCount,
    videoDeviceCount,
    refreshDevices,
    deviceSaveStatus,
    toggleAudioDevice,
    toggleMidiDevice,
    toggleMidiTrigger,
    toggleVideoDevice,
    setVideoDeviceCodec
  } from '$lib/stores/devices';
  import { settings } from '$lib/stores/settings';
  import type { VideoCodec, VideoEncodingMode, EncoderAvailability } from '$lib/api';
  import { getEncoderAvailability } from '$lib/api';
  
  let encoderAvailability = $state<EncoderAvailability | null>(null);
  
  // Load encoder availability on mount
  $effect(() => {
    getEncoderAvailability().then(availability => {
      encoderAvailability = availability;
    });
  });
  
  // Check if raw video encoding is available
  function isRawEncodingAvailable(): boolean {
    if (!encoderAvailability) return false;
    return encoderAvailability.av1_available || encoderAvailability.vp9_available || encoderAvailability.vp8_available;
  }
  
  // Filter codecs to only show those that are actually usable, with 'raw' last
  function getAvailableCodecs(codecs: VideoCodec[]): VideoCodec[] {
    let filtered = codecs;
    if (!isRawEncodingAvailable()) {
      // Filter out 'raw' if no encoders are available
      filtered = codecs.filter(c => c !== 'raw');
    }
    // Sort so 'raw' is always last
    return [...filtered].sort((a, b) => {
      if (a === 'raw') return 1;
      if (b === 'raw') return -1;
      return 0;
    });
  }
  
  function getEncodingLabel(mode: VideoEncodingMode | undefined): string {
    switch (mode) {
      case 'av1': return 'AV1';
      case 'vp9': return 'VP9';
      case 'vp8': return 'VP8';
      default: return 'AV1';
    }
  }
  
  function getCodecDisplayName(codec: VideoCodec, encodingMode: VideoEncodingMode | undefined): string {
    switch (codec) {
      case 'mjpeg': return 'MJPEG';
      case 'vp8': return 'VP8';
      case 'vp9': return 'VP9';
      case 'av1': return 'AV1';
      case 'raw': return `Raw [${getEncodingLabel(encodingMode)}]`;
      default: return codec.toUpperCase();
    }
  }
  
  let expandedSections = $state<Set<string>>(new Set(['audio', 'midi']));
  let filterQuery = $state('');
  let showMidiHelp = $state(false);
  let showFormatHelp = $state(false);
  
  function toggleSection(section: string) {
    expandedSections = new Set(expandedSections);
    if (expandedSections.has(section)) {
      expandedSections.delete(section);
    } else {
      expandedSections.add(section);
    }
  }
  
  function filterDevices<T extends { name: string }>(devices: T[]): T[] {
    if (!filterQuery) return devices;
    const query = filterQuery.toLowerCase();
    return devices.filter(d => d.name.toLowerCase().includes(query));
  }
</script>

<div class="device-panel">
  <div class="panel-header">
    <h2>Devices</h2>
    <div class="header-actions">
      {#if $deviceSaveStatus === 'saving' || $deviceSaveStatus === 'saved' || $deviceSaveStatus === 'error'}
        <div class="save-status" class:saving={$deviceSaveStatus === 'saving'} class:saved={$deviceSaveStatus === 'saved'} class:error={$deviceSaveStatus === 'error'}>
          {#if $deviceSaveStatus === 'saving'}
            <svg class="icon spinner" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="10" stroke-opacity="0.25"/>
              <path d="M12 2a10 10 0 0 1 10 10" stroke-linecap="round"/>
            </svg>
            Saving...
          {:else if $deviceSaveStatus === 'saved'}
            <svg class="icon check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <polyline points="20 6 9 17 4 12"/>
            </svg>
            Saved
          {:else if $deviceSaveStatus === 'error'}
            Save failed
          {/if}
        </div>
      {/if}
      <button class="action-btn" onclick={refreshDevices}>
        Refresh
      </button>
    </div>
  </div>
  
  <div class="search-bar">
    <input 
      type="text" 
      placeholder="Filter devices..." 
      bind:value={filterQuery}
    />
  </div>
  
  <div class="device-sections">
    <!-- MIDI Devices -->
    <div class="device-section">
      <button 
        class="section-header"
        onclick={() => toggleSection('midi')}
      >
        <span class="section-arrow">{expandedSections.has('midi') ? '▼' : '▶'}</span>
        <span class="section-icon">🎹</span>
        <span class="section-title">MIDI Sources</span>
        <span class="section-count">
          ({$midiDeviceCount.triggers} trigger, {$midiDeviceCount.selected} record of {$midiDeviceCount.total})
        </span>
      </button>
      
      {#if expandedSections.has('midi')}
        <div class="section-content">
          <div class="midi-header">
            <span class="midi-col-device">Device</span>
            <div class="midi-col-trigger">
              <span>Trigger</span>
              <button 
                class="help-btn" 
                onclick={(e) => { e.stopPropagation(); showMidiHelp = !showMidiHelp; }}
                onblur={() => showMidiHelp = false}
              >
                ?
              </button>
              {#if showMidiHelp}
                <div class="help-tooltip">
                  When MIDI is detected on a device marked as <strong>Trigger</strong>, all devices marked as <strong>Record</strong> will automatically start recording.
                </div>
              {/if}
            </div>
            <span class="midi-col-record">Record</span>
          </div>
          <div class="device-list">
            {#each filterDevices($midiDevices) as device}
              <div class="device-row midi-row">
                <span class="device-name">{device.name}</span>
                <label class="checkbox-cell">
                  <input 
                    type="checkbox"
                    checked={$triggerMidiDevices.has(device.id)}
                    onchange={() => toggleMidiTrigger(device.id)}
                  />
                </label>
                <label class="checkbox-cell">
                  <input 
                    type="checkbox"
                    checked={$selectedMidiDevices.has(device.id)}
                    onchange={() => toggleMidiDevice(device.id)}
                  />
                </label>
              </div>
            {/each}
            {#if $midiDevices.length === 0}
              <p class="empty-message">No MIDI devices found</p>
            {/if}
          </div>
        </div>
      {/if}
    </div>
    
    <!-- Audio Devices -->
    <div class="device-section">
      <button 
        class="section-header"
        onclick={() => toggleSection('audio')}
      >
        <span class="section-arrow">{expandedSections.has('audio') ? '▼' : '▶'}</span>
        <span class="section-icon">🎤</span>
        <span class="section-title">Audio Sources</span>
        <span class="section-count">
          ({$audioDeviceCount.selected} selected of {$audioDeviceCount.total})
        </span>
      </button>
      
      {#if expandedSections.has('audio')}
        <div class="section-content">
          <div class="midi-header">
            <span class="midi-col-device">Device</span>
            <span class="midi-col-trigger"></span>
            <span class="midi-col-record">Record</span>
          </div>
          <div class="device-list">
            {#each filterDevices($audioDevices) as device}
              <div class="device-row midi-row">
                <div class="device-info">
                  <span class="device-name">{device.name}</span>
                  <div class="device-meta">
                    <span class="meta-tag">{device.channels}ch</span>
                    <span class="meta-tag">{device.sample_rate / 1000}kHz</span>
                    {#if device.is_default}
                      <span class="meta-tag default">System Default</span>
                    {/if}
                  </div>
                </div>
                <span class="placeholder-cell"></span>
                <label class="checkbox-cell">
                  <input 
                    type="checkbox"
                    checked={$selectedAudioDevices.has(device.id)}
                    onchange={() => toggleAudioDevice(device.id)}
                  />
                </label>
              </div>
            {/each}
            {#if $audioDevices.length === 0}
              <p class="empty-message">No audio devices found</p>
            {/if}
          </div>
        </div>
      {/if}
    </div>
    
    <!-- Video Devices -->
    <div class="device-section">
      <button 
        class="section-header"
        onclick={() => toggleSection('video')}
      >
        <span class="section-arrow">{expandedSections.has('video') ? '▼' : '▶'}</span>
        <span class="section-icon">🎥</span>
        <span class="section-title">Video Sources</span>
        <span class="section-count">
          ({$videoDeviceCount.selected} selected of {$videoDeviceCount.total})
        </span>
      </button>
      
      {#if expandedSections.has('video')}
        <div class="section-content">
          <div class="video-header">
            <span class="video-col-device">Device</span>
            <div class="video-col-format">
              <span>Stream Type</span>
              <button 
                class="help-btn" 
                onclick={(e) => { e.stopPropagation(); showFormatHelp = !showFormatHelp; }}
                onblur={() => showFormatHelp = false}
              >
                ?
              </button>
              {#if showFormatHelp}
                <div class="help-tooltip format-tooltip">
                  Video sources may provide pre-encoded streams (like MJPEG) which use less system resources. Raw streams need to be encoded by your system (configured in <b>Settings</b>).
                </div>
              {/if}
            </div>
            <span class="video-col-record">Record</span>
          </div>
          <div class="device-list">
            {#each filterDevices($videoDevices) as device}
              {@const availableCodecs = getAvailableCodecs(device.supported_codecs)}
              {@const isSupported = availableCodecs.length > 0}
              {@const selectedCodec = $videoDeviceCodecs[device.id]}
              {@const effectiveCodec = selectedCodec && availableCodecs.includes(selectedCodec) ? selectedCodec : availableCodecs[0]}
              <div class="device-row video-row" class:device-unsupported={!isSupported}>
                <div class="device-info">
                  <span class="device-name">{device.name}</span>
                  <div class="device-meta">
                    {#if device.resolutions.length > 0}
                      <span class="meta-tag">
                        {device.resolutions[0].width}x{device.resolutions[0].height}
                      </span>
                    {/if}
                  </div>
                </div>
                <div class="codec-tags-cell">
                  {#if isSupported}
                    {#each availableCodecs as codec}
                      {@const isSelected = codec === effectiveCodec}
                      <button 
                        class="codec-tag" 
                        class:codec-selected={isSelected}
                        onclick={() => setVideoDeviceCodec(device.id, codec)}
                      >
                        {getCodecDisplayName(codec, $settings?.video_encoding_mode)}
                      </button>
                    {/each}
                  {:else}
                    <span class="meta-tag unsupported">No formats</span>
                  {/if}
                </div>
                <label class="checkbox-cell">
                  <input 
                    type="checkbox"
                    checked={$selectedVideoDevices.has(device.id)}
                    onchange={() => toggleVideoDevice(device.id)}
                    disabled={!isSupported}
                  />
                </label>
              </div>
            {/each}
            {#if $videoDevices.length === 0}
              <p class="empty-message">No video devices found</p>
            {/if}
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .device-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    gap: 1rem;
  }
  
  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  
  .panel-header h2 {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
    font-size: 1.375rem;
    font-weight: 400;
    color: #e8e6e3;
    letter-spacing: 0.06em;
  }
  
  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  
  .save-status {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    border-radius: 0.25rem;
    font-size: 0.75rem;
    font-weight: 400;
    letter-spacing: 0.02em;
    transition: all 0.2s ease;
  }
  
  .save-status .icon {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
  }
  
  .save-status.saving {
    background: rgba(113, 113, 122, 0.1);
    color: #8a8a8a;
  }
  
  .save-status.saving .spinner {
    animation: spin 1s linear infinite;
  }
  
  .save-status.saved {
    background: rgba(201, 169, 98, 0.15);
    color: #c9a962;
    animation: fadeOut 2s ease forwards;
    animation-delay: 1s;
  }
  
  .save-status.error {
    background: rgba(239, 68, 68, 0.1);
    color: #fca5a5;
  }
  
  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
  
  @keyframes fadeOut {
    0% { opacity: 1; }
    70% { opacity: 1; }
    100% { opacity: 0; }
  }
  
  .action-btn {
    padding: 0.5rem 0.75rem;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #6b6b6b;
    font-family: inherit;
    font-size: 0.75rem;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  
  .action-btn:hover:not(:disabled) {
    color: #a8a8a8;
    border-color: rgba(255, 255, 255, 0.1);
  }
  
  .search-bar input {
    width: 100%;
    max-width: 400px;
    padding: 0.5rem 0.75rem;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #e8e6e3;
    font-family: inherit;
    font-size: 0.8125rem;
  }
  
  .search-bar input::placeholder {
    color: #4a4a4a;
  }
  
  .search-bar input:focus {
    outline: none;
    border-color: rgba(201, 169, 98, 0.4);
  }
  
  .device-sections {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-height: 0;
    padding-bottom: 1rem;
  }
  
  .device-section {
    background: rgba(255, 255, 255, 0.015);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    overflow: hidden;
    flex-shrink: 0;
  }
  
  .section-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    width: 100%;
    padding: 0.75rem 1rem;
    background: transparent;
    border: none;
    color: #a8a8a8;
    font-family: inherit;
    font-size: 0.875rem;
    text-align: left;
    cursor: pointer;
    transition: background 0.15s ease;
    position: sticky;
    top: 0;
    z-index: 1;
  }
  
  .section-header:hover {
    background: rgba(255, 255, 255, 0.02);
  }
  
  .section-arrow {
    font-size: 0.5rem;
    color: #4a4a4a;
    transition: transform 0.15s ease;
  }
  
  .section-icon {
    font-size: 1.125rem;
    opacity: 0.6;
  }
  
  .section-title {
    font-weight: 400;
  }
  
  .section-count {
    color: #5a5a5a;
    font-size: 0.75rem;
    margin-left: auto;
  }
  
  .section-content {
    padding: 0 1rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  
  /* Scrollable list for many devices */
  .device-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-height: 280px;
    overflow-y: auto;
    scrollbar-gutter: stable;
  }
  
  .device-list::-webkit-scrollbar {
    width: 6px;
  }
  
  .device-list::-webkit-scrollbar-track {
    background: rgba(0, 0, 0, 0.2);
    border-radius: 3px;
  }
  
  .device-list::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.15);
    border-radius: 3px;
  }
  
  .device-list::-webkit-scrollbar-thumb:hover {
    background: rgba(255, 255, 255, 0.25);
  }
  
  .device-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.75rem;
    background: rgba(0, 0, 0, 0.15);
    border-radius: 0.25rem;
  }
  
  
  .device-name {
    font-size: 0.8125rem;
    color: #a8a8a8;
  }
  
  .device-meta {
    display: flex;
    gap: 0.375rem;
  }
  
  .meta-tag {
    padding: 0.1875rem 0.4375rem;
    background: rgba(255, 255, 255, 0.04);
    border-radius: 0.125rem;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.625rem;
    color: #5a5a5a;
    letter-spacing: 0.02em;
  }
  
  .meta-tag.default {
    background: rgba(201, 169, 98, 0.15);
    color: #c9a962;
  }
  
  .meta-tag.unsupported {
    background: rgba(180, 60, 60, 0.15);
    color: #a65d5d;
  }
  
  .meta-tag.codec {
    background: rgba(120, 140, 180, 0.15);
    color: #8a9cc0;
  }
  
  .meta-tag.format-unsupported {
    background: rgba(100, 100, 100, 0.1);
    color: #5a5a5a;
  }
  
  
  .device-unsupported {
    opacity: 0.45;
  }
  
  .device-unsupported .device-name {
    color: #5a5a5a;
  }
  
  .midi-header {
    display: grid;
    grid-template-columns: 1fr 70px 70px;
    padding: 0.5rem 0.75rem;
    font-size: 0.625rem;
    font-weight: 400;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #4a4a4a;
    align-items: center;
    scrollbar-gutter: stable;
  }
  
  .midi-col-device {
    /* left aligned by default */
  }
  
  .midi-col-trigger {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    position: relative;
  }
  
  .midi-col-record {
    text-align: center;
  }
  
  /* Video device section */
  .video-header {
    display: grid;
    grid-template-columns: 1fr auto 70px;
    padding: 0.5rem 0.75rem;
    font-size: 0.625rem;
    font-weight: 400;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #4a4a4a;
    align-items: center;
    scrollbar-gutter: stable;
  }
  
  .video-col-device {
    /* left aligned by default */
  }
  
  .video-col-format {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.25rem;
    position: relative;
    padding-right: 0.5rem;
  }
  
  .format-tooltip {
    right: 0;
    left: auto;
    transform: none;
  }
  
  .video-col-record {
    text-align: center;
  }
  
  .video-row {
    display: grid;
    grid-template-columns: 1fr auto 70px;
  }
  
  .codec-tags-cell {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 0.25rem;
    padding-right: 0.5rem;
  }
  
  .codec-tag {
    padding: 0.125rem 0.4375rem;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 0.125rem;
    color: #5a5a5a;
    font-family: inherit;
    font-size: 0.625rem;
    letter-spacing: 0.02em;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .codec-tag:hover {
    background: rgba(255, 255, 255, 0.06);
    border-color: rgba(255, 255, 255, 0.15);
    color: #8a8a8a;
  }
  
  .codec-tag.codec-selected {
    background: rgba(201, 169, 98, 0.12);
    border-color: rgba(201, 169, 98, 0.35);
    color: #c9a962;
  }
  
  .codec-tag.codec-selected:hover {
    background: rgba(201, 169, 98, 0.18);
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
    display: flex;
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
    font-size: 0.75rem;
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    color: #8a8a8a;
    white-space: normal;
    width: 220px;
    line-height: 1.4;
    z-index: 100;
  }
  
  .help-tooltip strong {
    color: #e4e4e7;
  }
  
  .midi-row {
    display: grid;
    grid-template-columns: 1fr 70px 70px;
  }
  
  .device-info {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  
  .placeholder-cell {
    /* Empty cell to maintain grid alignment */
  }
  
  .checkbox-cell {
    display: flex;
    justify-content: center;
    align-items: center;
  }
  
  .checkbox-cell input {
    accent-color: #ef4444;
    width: 16px;
    height: 16px;
    margin: 0;
  }
  
  .empty-message {
    padding: 1rem;
    text-align: center;
    color: #5a5a5a;
    font-size: 0.875rem;
  }

  /* Light mode overrides */
  :global(body.light-mode) .panel-header h2 {
    color: #2a2a2a;
  }

  :global(body.light-mode) .action-btn {
    border-color: rgba(0, 0, 0, 0.12);
    color: #5a5a5a;
  }

  :global(body.light-mode) .action-btn:hover:not(:disabled) {
    color: #3a3a3a;
    border-color: rgba(0, 0, 0, 0.2);
  }

  :global(body.light-mode) .save-status.saving {
    background: rgba(0, 0, 0, 0.05);
    color: #6a6a6a;
  }

  :global(body.light-mode) .save-status.saved {
    background: rgba(160, 128, 48, 0.12);
    color: #8a6a20;
  }

  :global(body.light-mode) .save-status.error {
    background: rgba(200, 60, 60, 0.1);
    color: #a04040;
  }

  :global(body.light-mode) .search-bar input {
    background: rgba(255, 255, 255, 0.9);
    border-color: rgba(0, 0, 0, 0.12);
    color: #2a2a2a;
  }

  :global(body.light-mode) .search-bar input::placeholder {
    color: #8a8a8a;
  }

  :global(body.light-mode) .search-bar input:focus {
    border-color: rgba(160, 128, 48, 0.5);
  }

  :global(body.light-mode) .device-section {
    background: rgba(255, 255, 255, 0.7);
    border-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .section-header {
    color: #4a4a4a;
  }

  :global(body.light-mode) .section-header:hover {
    background: rgba(0, 0, 0, 0.03);
  }

  :global(body.light-mode) .section-arrow {
    color: #8a8a8a;
  }

  :global(body.light-mode) .section-count {
    color: #7a7a7a;
  }

  :global(body.light-mode) .midi-header,
  :global(body.light-mode) .video-header {
    color: #7a7a7a;
  }

  :global(body.light-mode) .device-row {
    background: rgba(0, 0, 0, 0.03);
  }

  :global(body.light-mode) .device-name {
    color: #3a3a3a;
  }

  :global(body.light-mode) .meta-tag {
    background: rgba(0, 0, 0, 0.06);
    color: #5a5a5a;
  }

  :global(body.light-mode) .meta-tag.default {
    background: rgba(160, 128, 48, 0.15);
    color: #8a6a20;
  }

  :global(body.light-mode) .meta-tag.unsupported {
    background: rgba(180, 60, 60, 0.1);
    color: #a04040;
  }

  :global(body.light-mode) .codec-tag {
    background: rgba(0, 0, 0, 0.04);
    border-color: rgba(0, 0, 0, 0.12);
    color: #5a5a5a;
  }

  :global(body.light-mode) .codec-tag:hover {
    background: rgba(0, 0, 0, 0.08);
    color: #3a3a3a;
  }

  :global(body.light-mode) .codec-tag.codec-selected {
    background: rgba(160, 128, 48, 0.15);
    border-color: rgba(160, 128, 48, 0.4);
    color: #8a6a20;
  }

  :global(body.light-mode) .codec-tag.codec-selected:hover {
    background: rgba(160, 128, 48, 0.2);
  }

  :global(body.light-mode) .help-btn {
    background: rgba(0, 0, 0, 0.08);
    color: #7a7a7a;
  }

  :global(body.light-mode) .help-btn:hover {
    background: rgba(0, 0, 0, 0.12);
    color: #4a4a4a;
  }

  :global(body.light-mode) .checkbox-cell input {
    accent-color: #c04040;
  }

  :global(body.light-mode) .empty-message {
    color: #8a8a8a;
  }

  :global(body.light-mode) .device-list::-webkit-scrollbar-track {
    background: rgba(0, 0, 0, 0.05);
  }

  :global(body.light-mode) .device-list::-webkit-scrollbar-thumb {
    background: rgba(0, 0, 0, 0.15);
  }

  :global(body.light-mode) .device-list::-webkit-scrollbar-thumb:hover {
    background: rgba(0, 0, 0, 0.25);
  }
</style>
