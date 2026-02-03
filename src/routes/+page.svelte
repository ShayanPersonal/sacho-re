<script lang="ts">
  import { onMount } from 'svelte';
  import RecordingIndicator from '$lib/components/RecordingIndicator.svelte';
  import SessionBrowser from '$lib/components/sessions/SessionBrowser.svelte';
  import SimilarityMap from '$lib/components/similarity/SimilarityMap.svelte';
  import DevicePanel from '$lib/components/devices/DevicePanel.svelte';
  import Settings from '$lib/components/Settings.svelte';
  import About from '$lib/components/About.svelte';
  import { refreshRecordingState } from '$lib/stores/recording';
  import { settings } from '$lib/stores/settings';
  
  type Tab = 'sessions' | 'similarity' | 'devices' | 'settings' | 'about';
  let activeTab: Tab = $state('sessions');
  
  // Reactive light mode from settings
  let isLightMode = $derived($settings?.light_mode ?? false);
  
  // Apply light mode class to document body for global styling
  $effect(() => {
    if (isLightMode) {
      document.body.classList.add('light-mode');
    } else {
      document.body.classList.remove('light-mode');
    }
  });
  
  onMount(() => {
    // Refresh recording state periodically
    const interval = setInterval(refreshRecordingState, 1000);
    return () => clearInterval(interval);
  });
</script>

<div class="app" class:light-mode={isLightMode}>
  <nav class="tabs">
    <button 
      class="tab" 
      class:active={activeTab === 'sessions'}
      onclick={() => activeTab = 'sessions'}
    >
      <svg class="tab-icon icon-recordings" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="12" cy="12" r="10"/>
        <circle cx="12" cy="12" r="3"/>
      </svg>
      Recordings
    </button>

    <button 
      class="tab" 
      class:active={activeTab === 'similarity'}
      onclick={() => activeTab = 'similarity'}
    >
      <svg class="tab-icon icon-visualize" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M3 3v18h18"/>
        <path d="M7 16l4-4 4 4 6-6"/>
      </svg>
      Visualize
    </button>
    <button 
      class="tab" 
      class:active={activeTab === 'devices'}
      onclick={() => activeTab = 'devices'}
    >
      <svg class="tab-icon icon-devices" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <rect x="4" y="4" width="16" height="16" rx="2"/>
        <circle cx="9" cy="9" r="1.5" fill="currentColor"/>
        <circle cx="15" cy="9" r="1.5" fill="currentColor"/>
        <circle cx="9" cy="15" r="1.5" fill="currentColor"/>
        <circle cx="15" cy="15" r="1.5" fill="currentColor"/>
      </svg>
      Devices
    </button>
    <button 
      class="tab" 
      class:active={activeTab === 'settings'}
      onclick={() => activeTab = 'settings'}
    >
      <svg class="tab-icon icon-settings" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="12" cy="12" r="3"/>
        <path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83"/>
      </svg>
      Settings
    </button>
    <button 
      class="tab" 
      class:active={activeTab === 'about'}
      onclick={() => activeTab = 'about'}
    >
      <svg class="tab-icon icon-about" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="12" cy="12" r="10"/>
        <path d="M12 16v-4M12 8h.01"/>
      </svg>
      About
    </button>
    <div class="spacer"></div>
    <RecordingIndicator />
  </nav>
  
  <main class="content">
    {#if activeTab === 'sessions'}
      <SessionBrowser />
    {:else if activeTab === 'similarity'}
      <SimilarityMap />
    {:else if activeTab === 'devices'}
      <DevicePanel />
    {:else if activeTab === 'settings'}
      <Settings />
    {:else if activeTab === 'about'}
      <About />
    {/if}
  </main>
</div>

<style>
  :global(*) {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }
  
  :global(body) {
    font-family: 'Roboto', -apple-system, BlinkMacSystemFont, sans-serif;
    background: #141414;
    color: #e8e6e3;
    min-height: 100vh;
    font-weight: 400;
    letter-spacing: 0.01em;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }
  
  :global(body.light-mode) {
    background: #f5f5f3;
    color: #2a2a2a;
  }
  
  :global(h1, h2, h3, h4, h5, h6) {
    font-family: 'Bebas Neue', Impact, 'Arial Narrow', sans-serif;
    font-weight: 400;
    letter-spacing: 0.05em;
  }
  
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    position: relative;
    background: linear-gradient(180deg, #141414 0%, #1a1917 100%);
  }
  
  .tabs {
    display: flex;
    align-items: center;
    gap: 0.125rem;
    padding: 0.625rem 1rem 0.625rem 0.5rem;
    background: linear-gradient(0,rgb(12, 12, 9) 0%, #1e1e1c 100%);
    border-bottom: 1px solid rgba(255, 255, 255, 0.2);
  }
  
  .spacer {
    flex: 1;
  }
  
  .tab {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 1.125rem;
    background: transparent;
    border: none;
    border-radius: 0.25rem;
    color: #8a8a8a;
    font-family: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.2s ease;
    position: relative;
  }
  
  .tab:not(:last-of-type)::after {
    content: '';
    position: absolute;
    right: -0.0625rem;
    top: 50%;
    transform: translateY(-50%);
    height: 1rem;
    width: 1px;
    background: rgba(255, 255, 255, 0.12);
  }
  
  .tab:hover {
    color: #b8b8b8;
  }
  
  .tab.active {
    color: rgb(219, 187, 116);
    text-shadow: 0 0 8px rgba(219, 187, 116, 0.5), 0 0 16px rgba(219, 187, 116, 0.25);
  }
  
  .tab-icon {
    width: 16px;
    height: 16px;
    stroke-width: 1.5;
  }
  
  .icon-recordings {
    stroke: #c75050;
  }
  
  .icon-visualize {
    stroke: #6bc750;
  }
  
  .icon-devices {
    stroke: #50a0c7;
  }
  
  .icon-settings {
    stroke: #9a8a8a;
  }
  
  .icon-about {
    stroke: #8a8ac7;
  }
  
  .content {
    flex: 1;
    overflow: hidden;
    padding: 1.5rem;
  }
  
  /* Light mode overrides */
  .app.light-mode {
    background: linear-gradient(180deg, #f5f5f3 0%, #eeeee8 100%);
  }
  
  .app.light-mode .tabs {
    background: linear-gradient(180deg, #ffffff 0%, #f8f8f6 100%);
    border-bottom: 1px solid rgba(201, 169, 98, 0.3);
  }
  
  .app.light-mode .tab {
    color: #6a6a6a;
  }
  
  .app.light-mode .tab:hover {
    color: #4a4a4a;
  }
  
  .app.light-mode .tab.active {
    color: #a08030;
    text-shadow: 0 0 8px rgba(160, 128, 48, 0.3), 0 0 16px rgba(160, 128, 48, 0.15);
  }
  
  .app.light-mode .tab:not(:last-of-type)::after {
    background: rgba(0, 0, 0, 0.1);
  }
  
  /* Global light mode overrides for child components */
  :global(body.light-mode h1),
  :global(body.light-mode h2),
  :global(body.light-mode h3),
  :global(body.light-mode h4) {
    color: #2a2a2a;
  }
  
  :global(body.light-mode p),
  :global(body.light-mode span),
  :global(body.light-mode label),
  :global(body.light-mode div) {
    color: #3a3a3a;
  }
  
  :global(body.light-mode input),
  :global(body.light-mode select),
  :global(body.light-mode textarea) {
    background: #ffffff;
    border-color: rgba(0, 0, 0, 0.2);
    color: #2a2a2a;
  }
  
  :global(body.light-mode input::placeholder) {
    color: #888888;
  }
  
  :global(body.light-mode input:focus),
  :global(body.light-mode select:focus),
  :global(body.light-mode textarea:focus) {
    border-color: rgba(160, 128, 48, 0.6);
  }
  
  :global(body.light-mode button) {
    color: #3a3a3a;
  }
  
  :global(body.light-mode .settings-section),
  :global(body.light-mode .about-card),
  :global(body.light-mode .session-card),
  :global(body.light-mode .device-card) {
    background: rgba(255, 255, 255, 0.7);
    border-color: rgba(0, 0, 0, 0.1);
  }
  
  /* Light mode text colors for common patterns */
  :global(body.light-mode .setting-label),
  :global(body.light-mode .feature-label) {
    color: #2a2a2a;
  }
  
  :global(body.light-mode .setting-description),
  :global(body.light-mode .feature-desc),
  :global(body.light-mode .setting-recommendation) {
    color: #5a5a5a;
  }
  
  :global(body.light-mode .encoder-info),
  :global(body.light-mode .tech-stack) {
    color: #4a4a4a;
    background: rgba(0, 0, 0, 0.04);
    border-color: rgba(0, 0, 0, 0.1);
  }
  
  :global(body.light-mode .encoder-warning) {
    color: #8a6a20;
    background: rgba(160, 128, 48, 0.1);
    border-color: rgba(160, 128, 48, 0.3);
  }
  
  :global(body.light-mode .version-badge) {
    color: #5a5a5a;
    background: rgba(0, 0, 0, 0.05);
    border-color: rgba(0, 0, 0, 0.1);
  }
  
  :global(body.light-mode .about-description) {
    color: #4a4a4a;
  }
  
  :global(body.light-mode .disclaimer p) {
    color: #6a6a6a;
  }
  
  :global(body.light-mode .input-suffix) {
    color: #5a5a5a;
  }
  
  /* Session browser light mode */
  :global(body.light-mode .session-item),
  :global(body.light-mode .session-list-item) {
    background: rgba(255, 255, 255, 0.6);
    border-color: rgba(0, 0, 0, 0.08);
  }
  
  :global(body.light-mode .session-item:hover),
  :global(body.light-mode .session-list-item:hover) {
    background: rgba(255, 255, 255, 0.9);
    border-color: rgba(160, 128, 48, 0.3);
  }
  
  :global(body.light-mode .session-date),
  :global(body.light-mode .session-time),
  :global(body.light-mode .session-duration) {
    color: #5a5a5a;
  }
  
  :global(body.light-mode .session-name) {
    color: #2a2a2a;
  }
  
  /* Device panel light mode */
  :global(body.light-mode .device-item),
  :global(body.light-mode .device-group) {
    background: rgba(255, 255, 255, 0.6);
    border-color: rgba(0, 0, 0, 0.08);
  }
  
  :global(body.light-mode .device-name) {
    color: #2a2a2a;
  }
  
  :global(body.light-mode .device-info),
  :global(body.light-mode .device-status) {
    color: #5a5a5a;
  }
  
  /* Recording indicator light mode */
  :global(body.light-mode .recording-indicator) {
    color: #3a3a3a;
  }
  
  /* Scrollbar light mode */
  :global(body.light-mode ::-webkit-scrollbar-track) {
    background: rgba(0, 0, 0, 0.05);
  }
  
  :global(body.light-mode ::-webkit-scrollbar-thumb) {
    background: rgba(0, 0, 0, 0.2);
  }
  
  :global(body.light-mode ::-webkit-scrollbar-thumb:hover) {
    background: rgba(0, 0, 0, 0.3);
  }
</style>
