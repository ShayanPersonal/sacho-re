<script lang="ts">
  import { settings, saveSettings, saveSettingsDebounced, saveStatus } from '$lib/stores/settings';
  import { open } from '@tauri-apps/plugin-dialog';
  import type { Config, EncoderAvailability, AutoSelectProgress } from '$lib/api';
  import { getEncoderAvailability, autoSelectEncoderPreset } from '$lib/api';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
    import { recordingState } from '$lib/stores/recording';
  
  // Local editable copy
  let localSettings = $state<Config | null>(null);
  let showRawVideoHelp = $state(false);
  let showAudioAdvanced = $state(false);
  let showEncoderAdvanced = $state(false);
  let encoderAvailability = $state<EncoderAvailability | null>(null);
  
  // Auto-select state
  let autoSelectRunning = $state(false);
  let autoSelectProgress = $state<AutoSelectProgress | null>(null);
  let autoSelectError = $state<string | null>(null);
  
  // Preset labels
  const presetLabels: Record<number, string> = {
    1: 'Lightest',
    2: 'Light',
    3: 'Balanced',
    4: 'Quality',
    5: 'Maximum',
  };
  
  // Listen for auto-select progress events
  onMount(() => {
    const unlisten = listen<AutoSelectProgress>('auto-select-progress', (event) => {
      autoSelectProgress = event.payload;
    });
    return () => { unlisten.then(fn => fn()); };
  });
  
  // Get current preset level for the selected encoding mode
  function getCurrentPresetLevel(): number {
    if (!localSettings) return 3;
    const mode = localSettings.video_encoding_mode;
    if (mode === 'raw') return 3;
    return localSettings.encoder_preset_levels?.[mode] ?? 3;
  }
  
  // Set preset level for the selected encoding mode
  function setPresetLevel(level: number) {
    if (!localSettings) return;
    const mode = localSettings.video_encoding_mode;
    if (mode === 'raw') return;
    if (!localSettings.encoder_preset_levels) {
      localSettings.encoder_preset_levels = {};
    }
    localSettings.encoder_preset_levels[mode] = level;
    autoSave();
  }
  
  // Run auto-select
  async function runAutoSelect() {
    if (autoSelectRunning) return;
    autoSelectRunning = true;
    autoSelectProgress = null;
    autoSelectError = null;
    
    try {
      const bestLevel = await autoSelectEncoderPreset();
      setPresetLevel(bestLevel);
    } catch (e) {
      autoSelectError = typeof e === 'string' ? e : (e as Error).message || 'Auto-select failed';
    } finally {
      autoSelectRunning = false;
      autoSelectProgress = null;
    }
  }
  
  $effect(() => {
    if ($settings && !localSettings) {
      localSettings = { ...$settings };
    }
  });
  
  // Load encoder availability on mount and set default if needed
  $effect(() => {
    getEncoderAvailability().then(availability => {
      encoderAvailability = availability;
      
      // If the current encoding mode is not valid or not set, use the recommended default
      if (localSettings && availability) {
        const currentMode = localSettings.video_encoding_mode;
        const isCurrentValid = (
          (currentMode === 'av1' && availability.av1_available) ||
          (currentMode === 'vp9' && availability.vp9_available) ||
          (currentMode === 'vp8' && availability.vp8_available)
        );
        
        if (!isCurrentValid) {
          localSettings.video_encoding_mode = availability.recommended_default;
          // Don't auto-save here - this is just setting the UI default
        }
      }
    });
  });
  
  // Auto-save for immediate changes (checkboxes, selects)
  function autoSave() {
    if (!localSettings) return;
    saveSettings(localSettings);
  }
  
  // Debounced auto-save for text/number inputs
  function autoSaveDebounced() {
    if (!localSettings) return;
    
    // Clamp numeric values to valid ranges
    localSettings.idle_timeout_secs = Math.max(5, Math.min(30, localSettings.idle_timeout_secs));
    localSettings.pre_roll_secs = Math.max(0, Math.min(30, localSettings.pre_roll_secs));
    
    saveSettingsDebounced(localSettings);
  }
  
  // Browse for recording location
  async function browseStoragePath() {
    if (!localSettings) return;
    
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: localSettings.storage_path,
      title: 'Select Recording Location'
    });
    
    if (selected && typeof selected === 'string') {
      localSettings.storage_path = selected;
      autoSave();
    }
  }
</script>

<div class="settings">
  <div class="settings-header">
    <h2>Settings</h2>
    {#if $saveStatus === 'saving' || $saveStatus === 'saved'}
      <div class="save-status" class:saving={$saveStatus === 'saving'} class:saved={$saveStatus === 'saved'}>
        {#if $saveStatus === 'saving'}
          <svg class="icon spinner" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10" stroke-opacity="0.25"/>
            <path d="M12 2a10 10 0 0 1 10 10" stroke-linecap="round"/>
          </svg>
          Saving...
        {:else if $saveStatus === 'saved'}
          <svg class="icon check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="20 6 9 17 4 12"/>
          </svg>
          Saved
        {/if}
      </div>
    {/if}
  </div>
  
  {#if localSettings}
    <div class="settings-content">
      <section class="settings-section">
        <h3>Recording</h3>
        <div class="setting-row">
          <label>
            <span class="setting-label">Auto-recording timeout</span>
            <span class="setting-description">Stop automatic recordings after no MIDI is detected for this length of time</span>
          </label>
          <div class="input-with-suffix">
            <input 
              type="number" 
              min="5" 
              max="30"
              bind:value={localSettings.idle_timeout_secs}
              oninput={autoSaveDebounced}
            />
            <span class="input-suffix">seconds</span>
          </div>
        </div>
        
        <div class="setting-row">
          <label>
            <span class="setting-label">Pre-roll Length</span>
            <span class="setting-description">How much of the past to retrospectively include at the start of a recording</span>
          </label>
          <div class="input-with-suffix">
            <input 
              type="number" 
              min="0" 
              max="30"
              bind:value={localSettings.pre_roll_secs}
              oninput={autoSaveDebounced}
            />
            <span class="input-suffix">seconds</span>
          </div>
        </div>
        <div class="setting-row">
          <div class="setting-label-group">
            <span class="setting-label-with-help">
              <span>Raw Video Handling</span>
              <button 
                class="help-btn" 
                onclick={() => showRawVideoHelp = !showRawVideoHelp}
                onblur={() => showRawVideoHelp = false}
              >
                ?
              </button>
              {#if showRawVideoHelp}
                <div class="help-tooltip">
                  If you select a video device that's tagged as <strong>raw</strong>, your system must encode the video. Depending on your choice, this uses system resources such as <strong>CPU</strong> and <strong>GPU</strong>.
                </div>
              {/if}
            </span>
            <span class="setting-description">Encoding to apply to raw video feeds</span>
          </div>
          <select bind:value={localSettings.video_encoding_mode} onchange={autoSave}>
            {#if encoderAvailability?.av1_available}
              <option value="av1">AV1 ({encoderAvailability.av1_encoder_name + (encoderAvailability.av1_hardware ? '' : ' - requires configuration')})</option>
            {/if}
            {#if encoderAvailability?.vp9_available}
              <option value="vp9">VP9 ({encoderAvailability.vp9_encoder_name + (encoderAvailability.vp9_hardware ? '' : ' - requires configuration')})</option>
            {/if}
            {#if encoderAvailability?.vp8_available}
              <option value="vp8">VP8 ({encoderAvailability.vp8_encoder_name + (encoderAvailability.vp8_hardware ? '' : ' - requires configuration')})</option>
            {/if}
            {#if !encoderAvailability?.av1_available && !encoderAvailability?.vp9_available && !encoderAvailability?.vp8_available}
              <option value="" disabled>No encoders available</option>
            {/if}
          </select>
          {#if encoderAvailability && !encoderAvailability.av1_available && !encoderAvailability.vp9_available && !encoderAvailability.vp8_available}
            <p class="encoder-warning">No encoders detected. Raw video recording is not available.</p>
          {:else if encoderAvailability}
            <p class="encoder-info">
              {#if encoderAvailability.av1_hardware || encoderAvailability.vp9_hardware || encoderAvailability.vp8_hardware}
                Your system supports hardware acceleration for {[
                  encoderAvailability.av1_hardware ? 'AV1' : null,
                  encoderAvailability.vp9_hardware ? 'VP9' : null,
                  encoderAvailability.vp8_hardware ? 'VP8' : null
                ].filter(Boolean).join(', ').replace(/, ([^,]*)$/, ' and $1')}. We recommend using <strong>{encoderAvailability.av1_hardware ? 'AV1' : encoderAvailability.vp9_hardware ? 'VP9' : 'VP8'}</strong> for the best experience.
              {:else}
                Your system does not support hardware acceleration for any available codec. We recommend using <strong>VP8</strong> for the best experience. Use the Advanced menu if you experience choppiness.
              {/if}
            </p>
          {/if}
          {#if localSettings.video_encoding_mode !== 'raw' && encoderAvailability && (encoderAvailability.av1_available || encoderAvailability.vp9_available || encoderAvailability.vp8_available)}
            <button class="advanced-toggle" onclick={() => showEncoderAdvanced = !showEncoderAdvanced}>
              Advanced
              <svg class="toggle-chevron" class:open={showEncoderAdvanced} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="6 9 12 15 18 9"></polyline>
              </svg>
            </button>
            {#if showEncoderAdvanced}
              <div class="encoder-advanced-section">
                <div class="preset-slider-group">
                  <div class="preset-header">
                    <span class="setting-label">Encoder Preset ({localSettings.video_encoding_mode.toUpperCase()})</span>
                    <span class="preset-value">{getCurrentPresetLevel()} — {presetLabels[getCurrentPresetLevel()] ?? 'Balanced'}</span>
                  </div>
                  <div class="preset-slider-row">
                    <span class="preset-endpoint">Lightest</span>
                    <input
                      type="range"
                      min="1"
                      max="5"
                      step="1"
                      value={getCurrentPresetLevel()}
                      oninput={(e) => setPresetLevel(parseInt((e.target as HTMLInputElement).value))}
                      class="preset-slider"
                      disabled={autoSelectRunning}
                    />
                    <span class="preset-endpoint">Maximum</span>
                  </div>
                  <p class="preset-description">
                    {#if getCurrentPresetLevel() <= 3}
                      Smaller files. Smoother recordings on less powerful systems.
                    {:else}
                      Larger files. Higher quality video. Requires a more powerful system.
                    {/if}
                  </p>
                </div>
                <div class="auto-select-group">
                  <button
                    class="auto-select-btn"
                    onclick={runAutoSelect}
                    disabled={autoSelectRunning}
                  >
                    {#if autoSelectRunning}
                      <svg class="icon spinner" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <circle cx="12" cy="12" r="10" stroke-opacity="0.25"/>
                        <path d="M12 2a10 10 0 0 1 10 10" stroke-linecap="round"/>
                      </svg>
                      {#if autoSelectProgress}
                        {autoSelectProgress.message}
                      {:else}
                        Starting...
                      {/if}
                    {:else}
                      Auto-select
                    {/if}
                  </button>
                  <p class="auto-select-description">
                    Tests each preset on your video sources to find the best one your system can handle without choppiness. Takes up to a minute.
                  </p>
                  {#if autoSelectError}
                    <p class="auto-select-error">{autoSelectError}</p>
                  {/if}
                </div>
              </div>
            {/if}
          {/if}
        </div>
      </section>
      
      <section class="settings-section">
        <h3>Storage</h3>
        <div class="setting-row">
          <label>
            <span class="setting-label">Recording Location</span>
            <span class="setting-description">Where to save and load recorded sessions</span>
          </label>
          <div class="path-input">
            <input 
              type="text" 
              bind:value={localSettings.storage_path}
              readonly
            />
            <button class="browse-btn" onclick={browseStoragePath}>Browse</button>
          </div>
          <p class="setting-recommendation">We recommend backing up this folder with a cloud storage service.</p>
        </div>
        <div class="setting-row">
          <label>
            <span class="setting-label">Audio Format</span>
            <span class="setting-description">Format for recorded audio files</span>
          </label>
          <select bind:value={localSettings.audio_format} onchange={autoSave}>
            <option value="wav">WAV (lossless, largest files)</option>
            <option value="flac">FLAC (lossless, smaller files)</option>
          </select>
          <button class="advanced-toggle" onclick={() => showAudioAdvanced = !showAudioAdvanced}>
            Advanced
            <svg class="toggle-chevron" class:open={showAudioAdvanced} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="6 9 12 15 18 9"></polyline>
            </svg>
          </button>
          {#if showAudioAdvanced}
          <div class="advanced-audio-section">
            <div class="advanced-audio-field">
              <div class="advanced-field-header">
                <span class="setting-label">Bit Depth ({localSettings.audio_format.toUpperCase()})</span>
                <span class="advanced-field-value">
                  {#if localSettings.audio_format === 'wav'}
                    {localSettings.wav_bit_depth === 'int16' ? '16-bit' : localSettings.wav_bit_depth === 'int24' ? '24-bit' : '32-bit float'}
                  {:else}
                    {localSettings.flac_bit_depth === 'int16' ? '16-bit' : localSettings.flac_bit_depth === 'int24' ? '24-bit' : '32-bit'}
                  {/if}
                </span>
              </div>
              {#if localSettings.audio_format === 'wav'}
                <select bind:value={localSettings.wav_bit_depth} onchange={autoSave}>
                  <option value="int16">16-bit</option>
                  <option value="int24">24-bit (default)</option>
                  <option value="float32">32-bit float</option>
                </select>
              {:else}
                <select bind:value={localSettings.flac_bit_depth} onchange={autoSave}>
                  <option value="int16">16-bit</option>
                  <option value="int24">24-bit (default)</option>
                  <option value="float32">32-bit (limited compatibility)</option>
                </select>
              {/if}
              <p class="advanced-field-description">
                {#if (localSettings.audio_format === 'wav' ? localSettings.wav_bit_depth : localSettings.flac_bit_depth) === 'int16'}
                  Smallest files. Not optimal if your audio has high dynamic range (both very quiet and very loud sections in the same recording).
                {:else if (localSettings.audio_format === 'wav' ? localSettings.wav_bit_depth : localSettings.flac_bit_depth) === 'int24'}
                  Studio quality. Wide compatibility.
                {:else}
                  {localSettings.audio_format === 'flac' ? ' New - some programs may not support 32-bit FLAC recordings.' : 'Good if the audio source is also 32-bit float.'}
                {/if}
              </p>
            </div>
            <!--<div class="advanced-audio-divider"></div>
            <div class="advanced-audio-field">
              <div class="advanced-field-header">
                <span class="setting-label">Sample Rate</span>
                <span class="advanced-field-value">
                  {#if localSettings.audio_format === 'wav'}
                    {localSettings.wav_sample_rate === 'passthrough' ? 'Device Native' : localSettings.wav_sample_rate.replace('rate', '').replace(/(\d+)/, (_, n) => (parseInt(n) / 1000).toFixed(parseInt(n) % 1000 ? 1 : 0)) + ' kHz'}
                  {:else}
                    {localSettings.flac_sample_rate === 'passthrough' ? 'Device Native' : localSettings.flac_sample_rate.replace('rate', '').replace(/(\d+)/, (_, n) => (parseInt(n) / 1000).toFixed(parseInt(n) % 1000 ? 1 : 0)) + ' kHz'}
                  {/if}
                </span>
              </div>
              {#if localSettings.audio_format === 'wav'}
                <select bind:value={localSettings.wav_sample_rate} onchange={autoSave}>
                  <option value="passthrough">Device Native (default)</option>
                </select>
              {:else}
                <select bind:value={localSettings.flac_sample_rate} onchange={autoSave}>
                  <option value="passthrough">Device Native (default)</option>
                </select>
              {/if}
              <p class="advanced-field-description">
                Records at whatever sample rate your audio device uses. No resampling.
              </p>
            </div>-->
          </div>
          {/if}
        </div>

      </section>
      
      <section class="settings-section">
        <h3>System</h3>
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.dark_mode}
              onchange={autoSave}
            />
            <span class="setting-label">Dark color scheme</span>
          </label>
        </div>
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.auto_start}
              onchange={autoSave}
            />
            <span class="setting-label">Start at system startup <i>(recommended)</i></span>
          </label>
           <p class="setting-recommendation">This ensures the application will start up again if your system restarts (such as for system updates). <b>On password-protected systems, you may have to log back in before the app starts.</b></p>
          <p class="setting-recommendation">To stop the application from running in the background, right-click the tray icon and select Quit. Note that your performances will not be recorded until the application is started again.</p>
        </div>
      </section>
      
      <section class="settings-section">
        <h3>Notifications</h3>
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.notify_recording_start}
              onchange={autoSave}
            />
            <span class="setting-label">Notify when recording starts</span>
          </label>
        </div>
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.notify_recording_stop}
              onchange={autoSave}
            />
            <span class="setting-label">Notify when recording stops</span>
          </label>
        </div>
      </section>
    </div>
  {:else}
    <div class="loading">Loading settings...</div>
  {/if}
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    height: 100%;
    gap: 1.5rem;
  }
  
  .settings-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  
  .settings-header h2 {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
    font-size: 1.375rem;
    font-weight: 400;
    color: #e8e6e3;
    letter-spacing: 0.06em;
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
  
  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
  
  @keyframes fadeOut {
    0% {
      opacity: 1;
    }
    70% {
      opacity: 1;
    }
    100% {
      opacity: 0;
    }
  }
  
  .settings-content {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
    gap: 1.5rem;
    align-content: start;
  }
  
  .settings-section {
    background: rgba(255, 255, 255, 0.015);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    padding: 1.25rem;
  }
  
  .settings-section h3 {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
    font-size: 0.6875rem;
    font-weight: 400;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #5a5a5a;
    margin-bottom: 1.25rem;
    padding-bottom: 0.75rem;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }
  
  .setting-row {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-bottom: 1.25rem;
  }
  
  .setting-row:last-child {
    margin-bottom: 0;
  }
  
  .setting-row > label:not(.checkbox-row) {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  
  .setting-label {
    font-size: 0.875rem;
    color: #a8a8a8;
  }
  
  .setting-description {
    font-size: 0.75rem;
    color: #4a4a4a;
  }

  .setting-recommendation {
    font-size: 0.75rem;
    color: #6a6a6a;
    font-style: italic;
    margin: 0.5rem 0 0 0;
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

  .advanced-audio-section {
    padding: 0.75rem;
    background: rgba(0, 0, 0, 0.15);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .advanced-audio-field {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .advanced-audio-field select {
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #e8e6e3;
    font-family: inherit;
    font-size: 0.8125rem;
  }

  .advanced-audio-field select:focus {
    outline: none;
    border-color: rgba(201, 169, 98, 0.4);
  }

  .advanced-audio-field select option {
    background: #1a1a1a;
    color: #e8e6e3;
  }

  .advanced-field-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .advanced-field-value {
    font-size: 0.75rem;
    color: #c9a962;
    font-weight: 500;
  }

  .advanced-field-description {
    font-size: 0.6875rem;
    color: #5a5a5a;
    line-height: 1.5;
    margin: 0;
  }

  .advanced-audio-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.04);
  }
  
  .setting-row input[type="number"],
  .setting-row input[type="text"],
  .setting-row select {
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #e8e6e3;
    font-family: inherit;
    font-size: 0.8125rem;
  }
  
  .setting-row input[type="number"] {
    max-width: 60px;
  }
  
  .input-with-suffix {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  
  .input-with-suffix input {
    flex-shrink: 0;
  }
  
  .input-suffix {
    font-size: 0.8125rem;
    color: #6b6b6b;
  }
  
  .setting-row input:focus,
  .setting-row select:focus {
    outline: none;
    border-color: rgba(201, 169, 98, 0.4);
  }
  
  .setting-row select option {
    background: #1a1a1a;
    color: #e8e6e3;
    padding: 0.5rem;
  }
  
  .setting-row select option:hover {
    background: #252525;
  }
  
  .encoder-warning {
    margin-top: 0.5rem;
    padding: 0.5rem 0.75rem;
    background: rgba(201, 169, 98, 0.08);
    border: 1px solid rgba(201, 169, 98, 0.2);
    border-radius: 0.25rem;
    color: #c9a962;
    font-size: 0.75rem;
  }
  
  .encoder-info {
    margin-top: 0.5rem;
    padding: 0.5rem 0.75rem;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #6b6b6b;
    font-size: 0.75rem;
    line-height: 1.5;
  }
  
  .encoder-info strong {
    color: #a8a8a8;
  }
  
  .encoder-advanced-section {
    padding: 0.75rem;
    background: rgba(0, 0, 0, 0.15);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  
  .preset-slider-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  
  .preset-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  
  .preset-value {
    font-size: 0.75rem;
    color: #c9a962;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  
  .preset-slider-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  
  .preset-endpoint {
    font-size: 0.6875rem;
    color: #5a5a5a;
    white-space: nowrap;
    min-width: 52px;
  }
  
  .preset-endpoint:last-child {
    text-align: right;
  }
  
  .preset-slider {
    flex: 1;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    background: rgba(255, 255, 255, 0.08);
    border-radius: 2px;
    outline: none;
    cursor: pointer;
  }
  
  .preset-slider::-webkit-slider-thumb {
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
  
  .preset-slider::-webkit-slider-thumb:hover {
    transform: scale(1.15);
  }
  
  .preset-slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #c9a962;
    cursor: pointer;
    border: 2px solid rgba(0, 0, 0, 0.3);
  }
  
  .preset-slider:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  .preset-slider:disabled::-webkit-slider-thumb {
    cursor: not-allowed;
  }
  
  .preset-description {
    font-size: 0.6875rem;
    color: #5a5a5a;
    line-height: 1.5;
    margin: 0;
  }
  
  .auto-select-group {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    padding-top: 0.375rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
  }
  
  .auto-select-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.375rem;
    padding: 0.5rem 0.75rem;
    background: transparent;
    border: 1px solid rgba(201, 169, 98, 0.25);
    border-radius: 0.25rem;
    color: #c9a962;
    font-family: inherit;
    font-size: 0.75rem;
    letter-spacing: 0.03em;
    cursor: pointer;
    transition: all 0.2s ease;
    width: 100%;
  }
  
  .auto-select-btn:hover:not(:disabled) {
    background: rgba(201, 169, 98, 0.08);
    border-color: rgba(201, 169, 98, 0.4);
  }
  
  .auto-select-btn:disabled {
    cursor: not-allowed;
    opacity: 0.8;
    border-color: rgba(201, 169, 98, 0.15);
  }
  
  .auto-select-btn .spinner {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    animation: spin 1s linear infinite;
  }
  
  .auto-select-description {
    font-size: 0.6875rem;
    color: #5a5a5a;
    line-height: 1.5;
    margin: 0;
  }
  
  .auto-select-error {
    font-size: 0.6875rem;
    color: #c96262;
    line-height: 1.5;
    margin: 0;
    padding: 0.375rem 0.5rem;
    background: rgba(201, 98, 98, 0.08);
    border: 1px solid rgba(201, 98, 98, 0.2);
    border-radius: 0.25rem;
  }
  
  .path-input {
    display: flex;
    gap: 0.5rem;
    width: 100%;
  }
  
  .path-input input {
    flex: 1;
    min-width: 0;
  }
  
  .browse-btn {
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
    white-space: nowrap;
    transition: all 0.2s ease;
  }
  
  .browse-btn:hover {
    color: #a8a8a8;
    border-color: rgba(255, 255, 255, 0.1);
  }
  
  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    cursor: pointer;
  }
  
  .checkbox-row input {
    accent-color: #c9a962;
    width: 16px;
    height: 16px;
  }
  
  .loading {
    padding: 2rem;
    text-align: center;
    color: #4a4a4a;
    font-size: 0.8125rem;
  }
  
  .setting-label-group {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .setting-label-with-help {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    position: relative;
  }
  
  .setting-label-with-help > span:first-child {
    font-size: 0.875rem;
    color: #a8a8a8;
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
    background: rgba(255, 255, 255, 0.1);
    color: #8a8a8a;
  }
  
  .help-tooltip {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.5rem;
    padding: 0.625rem 0.75rem;
    background: #1a1a1a;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.25rem;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    font-size: 0.75rem;
    font-weight: 400;
    color: #8a8a8a;
    white-space: normal;
    width: 280px;
    line-height: 1.5;
    z-index: 100;
  }
  
  .help-tooltip strong {
    color: #e8e6e3;
  }

  /* Light mode overrides */
  :global(body.light-mode) .settings-header h2 {
    color: #2a2a2a;
  }

  :global(body.light-mode) .settings-section {
    background: rgba(255, 255, 255, 0.7);
    border-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .settings-section h3 {
    color: #7a7a7a;
    border-bottom-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .setting-label {
    color: #3a3a3a;
  }

  :global(body.light-mode) .setting-description {
    color: #6a6a6a;
  }

  :global(body.light-mode) .setting-recommendation {
    color: #7a7a7a;
  }

  :global(body.light-mode) .advanced-toggle {
    color: #7a7a7a;
  }

  :global(body.light-mode) .advanced-toggle:hover {
    color: #4a4a4a;
  }

  :global(body.light-mode) .advanced-audio-section {
    background: rgba(0, 0, 0, 0.03);
    border-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .advanced-audio-field select {
    background: rgba(255, 255, 255, 0.9);
    border-color: rgba(0, 0, 0, 0.15);
    color: #2a2a2a;
  }

  :global(body.light-mode) .advanced-audio-field select:focus {
    border-color: rgba(160, 128, 48, 0.5);
  }

  :global(body.light-mode) .advanced-audio-field select option {
    background: #ffffff;
    color: #2a2a2a;
  }

  :global(body.light-mode) .advanced-field-value {
    color: #8a6a20;
  }

  :global(body.light-mode) .advanced-field-description {
    color: #7a7a7a;
  }

  :global(body.light-mode) .advanced-audio-divider {
    background: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .setting-row input[type="number"],
  :global(body.light-mode) .setting-row input[type="text"],
  :global(body.light-mode) .setting-row select {
    background: rgba(255, 255, 255, 0.9);
    border-color: rgba(0, 0, 0, 0.15);
    color: #2a2a2a;
  }

  :global(body.light-mode) .setting-row input:focus,
  :global(body.light-mode) .setting-row select:focus {
    border-color: rgba(160, 128, 48, 0.5);
  }

  :global(body.light-mode) .setting-row select option {
    background: #ffffff;
    color: #2a2a2a;
  }

  :global(body.light-mode) .input-suffix {
    color: #6a6a6a;
  }

  :global(body.light-mode) .browse-btn {
    border-color: rgba(0, 0, 0, 0.12);
    color: #5a5a5a;
  }

  :global(body.light-mode) .browse-btn:hover {
    color: #3a3a3a;
    border-color: rgba(0, 0, 0, 0.2);
  }

  :global(body.light-mode) .checkbox-row input {
    accent-color: #a08030;
  }

  :global(body.light-mode) .encoder-warning {
    background: rgba(180, 140, 40, 0.1);
    border-color: rgba(180, 140, 40, 0.3);
    color: #8a6a20;
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

  :global(body.light-mode) .save-status.saving {
    background: rgba(0, 0, 0, 0.05);
    color: #6a6a6a;
  }

  :global(body.light-mode) .save-status.saved {
    background: rgba(160, 128, 48, 0.12);
    color: #8a6a20;
  }

  :global(body.light-mode) .loading {
    color: #8a8a8a;
  }

  :global(body.light-mode) .setting-label-with-help > span:first-child {
    color: #3a3a3a;
  }

  :global(body.light-mode) .encoder-advanced-section {
    background: rgba(0, 0, 0, 0.03);
    border-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .preset-value {
    color: #8a6a20;
  }

  :global(body.light-mode) .preset-endpoint {
    color: #7a7a7a;
  }

  :global(body.light-mode) .preset-slider {
    background: rgba(0, 0, 0, 0.1);
  }

  :global(body.light-mode) .preset-slider::-webkit-slider-thumb {
    background: #a08030;
    border-color: rgba(255, 255, 255, 0.5);
  }

  :global(body.light-mode) .preset-slider::-moz-range-thumb {
    background: #a08030;
    border-color: rgba(255, 255, 255, 0.5);
  }

  :global(body.light-mode) .preset-description {
    color: #7a7a7a;
  }

  :global(body.light-mode) .auto-select-group {
    border-top-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .auto-select-btn {
    border-color: rgba(160, 128, 48, 0.3);
    color: #8a6a20;
  }

  :global(body.light-mode) .auto-select-btn:hover:not(:disabled) {
    background: rgba(160, 128, 48, 0.08);
    border-color: rgba(160, 128, 48, 0.5);
  }

  :global(body.light-mode) .auto-select-description {
    color: #7a7a7a;
  }

  :global(body.light-mode) .auto-select-error {
    color: #a04040;
    background: rgba(160, 64, 64, 0.08);
    border-color: rgba(160, 64, 64, 0.2);
  }
</style>
