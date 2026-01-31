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
    saveDeviceSelection,
    toggleAudioDevice,
    toggleMidiDevice,
    toggleMidiTrigger,
    toggleVideoDevice,
    setVideoDeviceCodec
  } from '$lib/stores/devices';
  import type { VideoCodec } from '$lib/api';
  
  let expandedSections = $state<Set<string>>(new Set(['audio', 'midi']));
  let isSaving = $state(false);
  let saveError = $state<string | null>(null);
  let filterQuery = $state('');
  let showMidiHelp = $state(false);
  
  function toggleSection(section: string) {
    expandedSections = new Set(expandedSections);
    if (expandedSections.has(section)) {
      expandedSections.delete(section);
    } else {
      expandedSections.add(section);
    }
  }
  
  async function handleSave() {
    isSaving = true;
    saveError = null;
    try {
      await saveDeviceSelection();
    } catch (error) {
      console.error('Failed to save:', error);
      saveError = error instanceof Error ? error.message : String(error);
    } finally {
      isSaving = false;
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
      <button class="action-btn" onclick={refreshDevices}>
        Refresh
      </button>
      <button 
        class="action-btn primary" 
        onclick={handleSave}
        disabled={isSaving}
      >
        {isSaving ? 'Saving...' : 'Save'}
      </button>
    </div>
  </div>
  
  {#if saveError}
    <div class="error-banner">
      <span class="error-icon">⚠️</span>
      <span class="error-text">{saveError}</span>
      <button class="error-dismiss" onclick={() => saveError = null}>×</button>
    </div>
  {/if}
  
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
                  When MIDI is detected on a device marked as <strong>Trigger</strong>, all devices marked as <strong>Record</strong> will start recording.
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
                      <span class="meta-tag default">Default</span>
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
          <div class="midi-header">
            <span class="midi-col-device">Device</span>
            <span class="midi-col-trigger"></span>
            <span class="midi-col-record">Record</span>
          </div>
          <div class="device-list">
            {#each filterDevices($videoDevices) as device}
              {@const isSupported = device.supported_codecs.length > 0}
              {@const selectedCodec = $videoDeviceCodecs[device.id]}
              {@const effectiveCodec = selectedCodec ?? device.supported_codecs[0]}
              <div class="device-row midi-row" class:device-unsupported={!isSupported}>
                <div class="device-info">
                  <span class="device-name">{device.name}</span>
                  <div class="device-meta">
                    <span class="meta-tag">{device.device_type}</span>
                    {#if device.resolutions.length > 0}
                      <span class="meta-tag">
                        {device.resolutions[0].width}x{device.resolutions[0].height}
                      </span>
                    {/if}
                    {#each device.all_formats as format}
                      {@const matchingCodec = ((): VideoCodec | null => {
                        if (format === 'MJPEG' && device.supported_codecs.includes('mjpeg')) return 'mjpeg';
                        if (format === 'H.264' && device.supported_codecs.includes('h264')) return 'h264';
                        if (format === 'H.265' && device.supported_codecs.includes('h265')) return 'h265';
                        if (format === 'AV1' && device.supported_codecs.includes('av1')) return 'av1';
                        return null;
                      })()}
                      {@const isSelected = matchingCodec !== null && matchingCodec === effectiveCodec}
                      {#if matchingCodec}
                        <button 
                          class="meta-tag codec-btn" 
                          class:codec-selected={isSelected}
                          class:codec-unselected={!isSelected}
                          onclick={() => { setVideoDeviceCodec(device.id, matchingCodec); }}
                          title="Click to use {format} for recording"
                        >
                          {format}
                        </button>
                      {:else}
                        <span class="meta-tag format-unsupported">
                          {format}
                        </span>
                      {/if}
                    {/each}
                    {#if !isSupported && device.all_formats.length === 0}
                      <span class="meta-tag unsupported">No formats detected</span>
                    {/if}
                  </div>
                </div>
                <span class="placeholder-cell"></span>
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
  
  .error-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 0.5rem;
    color: #fca5a5;
    font-size: 0.875rem;
  }
  
  .error-icon {
    flex-shrink: 0;
  }
  
  .error-text {
    flex: 1;
  }
  
  .error-dismiss {
    background: none;
    border: none;
    color: #fca5a5;
    cursor: pointer;
    font-size: 1.25rem;
    padding: 0;
    line-height: 1;
    opacity: 0.7;
  }
  
  .error-dismiss:hover {
    opacity: 1;
  }
  
  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  
  .panel-header h2 {
    font-size: 1.125rem;
    font-weight: 600;
    color: #fff;
  }
  
  .header-actions {
    display: flex;
    gap: 0.5rem;
  }
  
  .action-btn {
    padding: 0.5rem 0.875rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    color: #a1a1aa;
    font-family: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .action-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    color: #e4e4e7;
  }
  
  .action-btn.primary {
    background: rgba(239, 68, 68, 0.15);
    border-color: rgba(239, 68, 68, 0.3);
    color: #ef4444;
  }
  
  .action-btn.primary:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.25);
  }
  
  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  .search-bar input {
    width: 100%;
    max-width: 400px;
    padding: 0.625rem 0.875rem;
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    color: #fff;
    font-family: inherit;
    font-size: 0.875rem;
  }
  
  .search-bar input::placeholder {
    color: #52525b;
  }
  
  .search-bar input:focus {
    outline: none;
    border-color: rgba(239, 68, 68, 0.4);
  }
  
  .device-sections {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-height: 0; /* Important for nested flex scroll */
    padding-bottom: 1rem;
  }
  
  .device-section {
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.75rem;
    overflow: hidden;
    flex-shrink: 0;
  }
  
  .section-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    width: 100%;
    padding: 0.875rem 1rem;
    background: transparent;
    border: none;
    color: #e4e4e7;
    font-family: inherit;
    font-size: 0.9375rem;
    text-align: left;
    cursor: pointer;
    transition: background 0.1s ease;
    position: sticky;
    top: 0;
    z-index: 1;
  }
  
  .section-header:hover {
    background: rgba(255, 255, 255, 0.02);
  }
  
  .section-arrow {
    font-size: 0.625rem;
    color: #52525b;
    transition: transform 0.15s ease;
  }
  
  .section-icon {
    font-size: 1.25rem;
  }
  
  .section-title {
    font-weight: 500;
  }
  
  .section-count {
    color: #71717a;
    font-size: 0.8125rem;
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
    padding: 0.75rem;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 0.5rem;
  }
  
  
  .device-name {
    font-size: 0.875rem;
    color: #e4e4e7;
  }
  
  .device-meta {
    display: flex;
    gap: 0.5rem;
  }
  
  .meta-tag {
    padding: 0.25rem 0.5rem;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 0.25rem;
    font-family: 'JetBrains Mono', monospace;
    font-size: 0.6875rem;
    color: #71717a;
  }
  
  .meta-tag.default {
    background: rgba(34, 197, 94, 0.15);
    color: #22c55e;
  }
  
  .meta-tag.unsupported {
    background: rgba(239, 68, 68, 0.15);
    color: #ef4444;
  }
  
  .meta-tag.codec {
    background: rgba(59, 130, 246, 0.15);
    color: #3b82f6;
  }
  
  .meta-tag.format-unsupported {
    background: rgba(113, 113, 122, 0.15);
    color: #71717a;
  }
  
  .codec-btn {
    cursor: pointer;
    border: 1px solid transparent;
    transition: all 0.15s ease;
  }
  
  .codec-btn:hover {
    border-color: #3b82f6;
    transform: translateY(-1px);
  }
  
  .codec-btn.codec-selected {
    background: rgba(34, 197, 94, 0.2);
    color: #22c55e;
    border-color: #22c55e;
  }
  
  .codec-btn.codec-unselected {
    background: rgba(59, 130, 246, 0.1);
    color: #60a5fa;
    opacity: 0.7;
  }
  
  .codec-btn.codec-unselected:hover {
    opacity: 1;
  }
  
  .device-unsupported {
    opacity: 0.5;
  }
  
  .device-unsupported .device-name {
    color: #71717a;
  }
  
  .midi-header {
    display: grid;
    grid-template-columns: 1fr 70px 70px;
    padding: 0.5rem 0.75rem;
    font-size: 0.6875rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #52525b;
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
  
  .help-btn {
    width: 14px;
    height: 14px;
    padding: 0;
    background: rgba(255, 255, 255, 0.1);
    border: none;
    border-radius: 50%;
    color: #71717a;
    font-size: 0.625rem;
    font-weight: 600;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.1s ease;
  }
  
  .help-btn:hover {
    background: rgba(255, 255, 255, 0.2);
    color: #a1a1aa;
  }
  
  .help-tooltip {
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    margin-top: 0.5rem;
    padding: 0.625rem 0.75rem;
    background: #27272a;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 0.5rem;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    font-size: 0.75rem;
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    color: #a1a1aa;
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
    color: #52525b;
    font-size: 0.875rem;
  }
</style>
