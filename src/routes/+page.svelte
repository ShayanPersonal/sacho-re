<script lang="ts">
  import { onMount } from 'svelte';
  import RecordingIndicator from '$lib/components/RecordingIndicator.svelte';
  import SessionBrowser from '$lib/components/sessions/SessionBrowser.svelte';
  import SimilarityMap from '$lib/components/similarity/SimilarityMap.svelte';
  import DevicePanel from '$lib/components/devices/DevicePanel.svelte';
  import Settings from '$lib/components/Settings.svelte';
  import About from '$lib/components/About.svelte';
  import { refreshRecordingState } from '$lib/stores/recording';
  
  type Tab = 'sessions' | 'similarity' | 'devices' | 'settings' | 'about';
  let activeTab: Tab = $state('sessions');
  
  onMount(() => {
    // Refresh recording state periodically
    const interval = setInterval(refreshRecordingState, 1000);
    return () => clearInterval(interval);
  });
</script>

<div class="app">
  <nav class="tabs">
    <button 
      class="tab" 
      class:active={activeTab === 'sessions'}
      onclick={() => activeTab = 'sessions'}
    >
      <svg class="tab-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
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
      <svg class="tab-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
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
      <svg class="tab-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
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
      <svg class="tab-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
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
      <svg class="tab-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
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
    padding: 0.625rem 1.5rem;
    background: rgba(0, 0, 0, 0.4);
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
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
    color: #6b6b6b;
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 400;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  
  .tab:hover {
    color: #a8a8a8;
  }
  
  .tab.active {
    color: #c9a962;
  }
  
  .tab-icon {
    width: 16px;
    height: 16px;
    stroke-width: 1.5;
  }
  
  .content {
    flex: 1;
    overflow: hidden;
    padding: 1.5rem;
  }
</style>
