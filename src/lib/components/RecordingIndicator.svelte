<script lang="ts">
  import { recordingState, isRecording, isInitializing, canRecord, doStartRecording, doStopRecording } from '$lib/stores/recording';
  import { settings } from '$lib/stores/settings';
  import { audioDeviceCount, midiDeviceCount, videoDeviceCount } from '$lib/stores/devices';
  import { formatDuration } from '$lib/api';
  
  let isLoading = $state(false);
  
  // Check if auto-record is configured (has trigger MIDI devices)
  let hasTrigger = $derived(
    $settings?.trigger_midi_devices && $settings.trigger_midi_devices.length > 0
  );
  
  // Track counts for each device type (filtered by actual existing devices)
  let midiCount = $derived($midiDeviceCount.selected);
  let audioCount = $derived($audioDeviceCount.selected);
  let videoCount = $derived($videoDeviceCount.selected);
  
  // Button should be disabled during loading, stopping, or initializing
  let buttonDisabled = $derived(
    isLoading || 
    $recordingState.status === 'stopping' || 
    $recordingState.status === 'initializing'
  );
  
  async function handleToggle() {
    isLoading = true;
    try {
      if ($isRecording) {
        await doStopRecording();
      } else {
        await doStartRecording();
      }
    } catch (error) {
      console.error('Recording toggle failed:', error);
    } finally {
      isLoading = false;
    }
  }
</script>

<div class="recording-indicator" class:recording={$isRecording} class:initializing={$isInitializing}>
  <div class="status-container">
    {#if $isRecording}
      <div class="status">
        <div class="status-dot active"></div>
        <span class="status-text recording">RECORDING</span>
      </div>
    {:else if $recordingState.status === 'stopping'}
      <div class="status">
        <div class="status-dot"></div>
        <span class="status-text">STOPPING...</span>
      </div>
    {:else if $isInitializing}
      <div class="status">
        <div class="status-dot initializing"></div>
        <span class="status-text initializing">INITIALIZING...</span>
      </div>
    {:else}
      <div class="track-counts">
        <span class="track-count" class:empty={midiCount === 0}>🎹 {midiCount}</span>
        <span class="track-count" class:empty={audioCount === 0}>🎤 {audioCount}</span>
        <span class="track-count" class:empty={videoCount === 0}>🎥 {videoCount}</span>
      </div>
      <div class="trigger-status" class:ready={hasTrigger} class:warning={!hasTrigger}>
        {#if hasTrigger}
          Waiting for MIDI trigger<span class="ellipsis"></span>
        {:else}
          No MIDI trigger selected!
        {/if}
      </div>
    {/if}
  </div>
  
  {#if $isRecording}
    <div class="elapsed">
      {formatDuration($recordingState.elapsed_seconds)}
    </div>
  {/if}
  
  <button 
    class="control-btn" 
    class:stop={$isRecording}
    onclick={handleToggle}
    disabled={buttonDisabled}
  >
    {#if $isRecording}
      <span class="btn-icon">⏹</span>
      Stop
    {:else if $isInitializing}
      <span class="btn-icon">⏳</span>
      Please Wait
    {:else}
      <span class="btn-icon">⏺</span>
      Start Manually
    {/if}
  </button>
</div>

<style>
  .recording-indicator {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 0.75rem;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.75rem;
    transition: all 0.2s ease;
  }
  
  .recording-indicator.recording {
    background: rgba(239, 68, 68, 0.08);
    border-color: rgba(239, 68, 68, 0.2);
  }
  
  .recording-indicator.initializing {
    background: rgba(251, 191, 36, 0.08);
    border-color: rgba(251, 191, 36, 0.2);
  }
  
  .status-container {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  
  .status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  
  .track-counts {
    display: flex;
    align-items: center;
    gap: 0.625rem;
  }
  
  .track-count {
    font-size: 0.75rem;
    color: #a1a1aa;
  }
  
  .track-count.empty {
    opacity: 0.4;
  }
  
  .trigger-status {
    font-size: 0.6875rem;
    color: #71717a;
  }
  
  .trigger-status.ready {
    color: #71717a;
  }
  
  .trigger-status.warning {
    color: #ef4444;
  }
  
  .ellipsis {
    display: inline-block;
    width: 1em;
    text-align: left;
  }
  
  .ellipsis::after {
    content: '';
    animation: ellipsis 1.5s infinite;
  }
  
  @keyframes ellipsis {
    0% { content: ''; }
    25% { content: '.'; }
    50% { content: '..'; }
    75% { content: '...'; }
    100% { content: ''; }
  }
  
  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #71717a;
    transition: all 0.2s ease;
  }
  
  .status-dot.active {
    background: #ef4444;
    box-shadow: 0 0 8px rgba(239, 68, 68, 0.5);
    animation: blink 1s ease-in-out infinite;
  }
  
  .status-dot.initializing {
    background: #fbbf24;
    box-shadow: 0 0 8px rgba(251, 191, 36, 0.5);
    animation: pulse 1.5s ease-in-out infinite;
  }
  
  @keyframes pulse {
    0%, 100% { transform: scale(1); opacity: 1; }
    50% { transform: scale(1.2); opacity: 0.7; }
  }
  
  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }
  
  .status-text {
    font-family: 'JetBrains Mono', monospace;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: #a1a1aa;
  }
  
  .status-text.recording {
    color: #ef4444;
  }
  
  .status-text.initializing {
    color: #fbbf24;
  }
  
  .elapsed {
    font-family: 'JetBrains Mono', monospace;
    font-size: 1rem;
    font-weight: 500;
    color: #fff;
    min-width: 60px;
  }
  
  .control-btn {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.5rem 0.875rem;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.5rem;
    color: #71717a;
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .control-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.06);
    border-color: rgba(255, 255, 255, 0.1);
    color: #a1a1aa;
  }
  
  .control-btn.stop {
    background: rgba(239, 68, 68, 0.15);
    border-color: rgba(239, 68, 68, 0.3);
    color: #ef4444;
  }
  
  .control-btn.stop:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.25);
  }
  
  .control-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  .btn-icon {
    font-size: 0.875rem;
  }
</style>
