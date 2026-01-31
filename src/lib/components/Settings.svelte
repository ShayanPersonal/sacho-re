<script lang="ts">
  import { settings, saveSettings, saveSettingsDebounced, saveStatus } from '$lib/stores/settings';
  import { open } from '@tauri-apps/plugin-dialog';
  import type { Config } from '$lib/api';
  
  // Local editable copy
  let localSettings = $state<Config | null>(null);
  let showRawVideoHelp = $state(false);
  
  $effect(() => {
    if ($settings && !localSettings) {
      localSettings = { ...$settings };
    }
  });
  
  // Auto-save for immediate changes (checkboxes, selects)
  function autoSave() {
    if (!localSettings) return;
    saveSettings(localSettings);
  }
  
  // Debounced auto-save for text/number inputs
  function autoSaveDebounced() {
    if (!localSettings) return;
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
            <span class="setting-label">Idle Timeout (seconds)</span>
            <span class="setting-description">Stop recording after this many seconds of no MIDI activity</span>
          </label>
          <input 
            type="number" 
            min="5" 
            max="300"
            bind:value={localSettings.idle_timeout_secs}
            oninput={autoSaveDebounced}
          />
        </div>
        
        <div class="setting-row">
          <label>
            <span class="setting-label">Pre-roll Size (seconds)</span>
            <span class="setting-description">Include this many seconds before the trigger in recordings</span>
          </label>
          <input 
            type="number" 
            min="0" 
            max="5"
            bind:value={localSettings.pre_roll_secs}
            oninput={autoSaveDebounced}
          />
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
                  If you select a video device that's tagged as <strong>raw</strong>, your computer is responsible for encoding the video. Depending on your choice, this may use system resources such as <strong>RAM</strong>, <strong>CPU</strong>, and <strong>GPU</strong>.
                </div>
              {/if}
            </span>
            <span class="setting-description">Encoding to apply on raw video feeds</span>
          </div>
          <select bind:value={localSettings.video_encoding_mode} onchange={autoSave}>
            <option value="av1_hardware">AV1 Hardware (uses less system resources)</option>
            <option value="vp8_software">VP8 Software (uses more system resources)</option>
            <option value="raw">Raw/Lossless (huge files!)</option>
          </select>
        </div>
      </section>
      
      <section class="settings-section">
        <h3>Storage</h3>
        <div class="setting-row">
          <label>
            <span class="setting-label">Recording Location</span>
            <span class="setting-description">Where to save recorded sessions</span>
          </label>
          <div class="path-input">
            <input 
              type="text" 
              bind:value={localSettings.storage_path}
              readonly
            />
            <button class="browse-btn" onclick={browseStoragePath}>Browse</button>
          </div>
        </div>
        <div class="setting-row">
          <label>
            <span class="setting-label">Audio Format</span>
            <span class="setting-description">Format for recorded audio files</span>
          </label>
          <select bind:value={localSettings.audio_format} onchange={autoSave}>
            <option value="wav">WAV (lossless, larger files)</option>
            <option value="flac">FLAC (lossless, compressed)</option>
          </select>
        </div>
        

      </section>
      
      <section class="settings-section">
        <h3>System</h3>
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.auto_start}
              onchange={autoSave}
            />
            <span class="setting-label">Start with system</span>
          </label>
        </div>
        
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.minimize_to_tray}
              onchange={autoSave}
            />
            <span class="setting-label">Minimize to tray on close</span>
          </label>
        </div>
      </section>
      
      <section class="settings-section">
        <h3>Notifications</h3>
        <div class="setting-row">
          <label class="checkbox-row">
            <input 
              type="checkbox" 
              bind:checked={localSettings.show_notifications}
              onchange={autoSave}
            />
            <span class="setting-label">Show desktop notifications</span>
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
    font-size: 1.125rem;
    font-weight: 600;
    color: #fff;
  }
  
  .save-status {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    border-radius: 9999px;
    font-size: 0.8125rem;
    font-weight: 500;
    transition: all 0.2s ease;
  }
  
  .save-status .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  
  .save-status.saving {
    background: rgba(113, 113, 122, 0.15);
    color: #a1a1aa;
  }
  
  .save-status.saving .spinner {
    animation: spin 1s linear infinite;
  }
  
  .save-status.saved {
    background: rgba(34, 197, 94, 0.15);
    color: #22c55e;
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
    grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
    gap: 2rem;
    align-content: start;
  }
  
  .settings-section {
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.75rem;
    padding: 1.25rem;
  }
  
  .settings-section h3 {
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #71717a;
    margin-bottom: 1.25rem;
    padding-bottom: 0.75rem;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
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
    font-size: 0.9375rem;
    color: #e4e4e7;
  }
  
  .setting-description {
    font-size: 0.8125rem;
    color: #52525b;
  }
  
  .setting-row input[type="number"],
  .setting-row input[type="text"],
  .setting-row select {
    width: 100%;
    padding: 0.625rem 0.875rem;
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    color: #fff;
    font-family: inherit;
    font-size: 0.875rem;
  }
  
  .setting-row input[type="number"] {
    max-width: 120px;
  }
  
  .setting-row input:focus,
  .setting-row select:focus {
    outline: none;
    border-color: rgba(239, 68, 68, 0.4);
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
    padding: 0.625rem 0.875rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    color: #a1a1aa;
    font-family: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
    white-space: nowrap;
  }
  
  .browse-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #e4e4e7;
  }
  
  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    cursor: pointer;
  }
  
  .checkbox-row input {
    accent-color: #ef4444;
    width: 18px;
    height: 18px;
  }
  
  .loading {
    padding: 2rem;
    text-align: center;
    color: #52525b;
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
    font-size: 0.9375rem;
    color: #e4e4e7;
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
    left: 0;
    margin-top: 0.5rem;
    padding: 0.625rem 0.75rem;
    background: #27272a;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 0.5rem;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    font-size: 0.75rem;
    font-weight: 400;
    color: #a1a1aa;
    white-space: normal;
    width: 280px;
    line-height: 1.4;
    z-index: 100;
  }
  
  .help-tooltip strong {
    color: #e4e4e7;
  }
</style>
