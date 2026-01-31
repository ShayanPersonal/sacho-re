<script lang="ts">
  import { onMount } from 'svelte';
  import RecordingIndicator from '$lib/components/RecordingIndicator.svelte';
  import SessionBrowser from '$lib/components/sessions/SessionBrowser.svelte';
  import SimilarityMap from '$lib/components/similarity/SimilarityMap.svelte';
  import DevicePanel from '$lib/components/devices/DevicePanel.svelte';
  import Settings from '$lib/components/Settings.svelte';
  import { refreshRecordingState } from '$lib/stores/recording';
  
  type Tab = 'sessions' | 'similarity' | 'devices' | 'settings';
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
      <span class="tab-icon">💿</span>
      Recordings
    </button>

    <button 
      class="tab" 
      class:active={activeTab === 'similarity'}
      onclick={() => activeTab = 'similarity'}
    >
      <span class="tab-icon">𖡎</span>
      Visualize
    </button>
    <button 
      class="tab" 
      class:active={activeTab === 'devices'}
      onclick={() => activeTab = 'devices'}
    >
      <span class="tab-icon">🎛️</span>
      Devices
    </button>
    <button 
      class="tab" 
      class:active={activeTab === 'settings'}
      onclick={() => activeTab = 'settings'}
    >
      <span class="tab-icon">⚙️</span>
      Settings
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
    font-family: 'Outfit', -apple-system, BlinkMacSystemFont, sans-serif;
    background: linear-gradient(135deg, #0f0f14 0%, #1a1a24 50%, #0d0d12 100%);
    color: #e4e4e7;
    min-height: 100vh;
  }
  
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    position: relative;
  }
  
  /* Subtle diagonal sheen effect mimicking light across a screen surface */
  .app::before {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    pointer-events: none;
    z-index: 9999;
    background: linear-gradient(
      125deg,
      transparent 0%,
      transparent 40%,
      rgba(255, 255, 255, 0.015) 45%,
      rgba(255, 255, 255, 0.03) 50%,
      rgba(255, 255, 255, 0.015) 55%,
      transparent 60%,
      transparent 100%
    );
  }
  
  /* Secondary softer highlight for depth */
  .app::after {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    pointer-events: none;
    z-index: 9998;
    background: 
      radial-gradient(
        ellipse 80% 50% at 20% 10%,
        rgba(255, 255, 255, 0.02) 0%,
        transparent 50%
      ),
      radial-gradient(
        ellipse 60% 40% at 85% 90%,
        rgba(200, 180, 255, 0.015) 0%,
        transparent 50%
      );
  }
  
  .tabs {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 1.5rem;
    background: rgba(0, 0, 0, 0.3);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  }
  
  .spacer {
    flex: 1;
  }
  
  .tab {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 1rem;
    background: transparent;
    border: none;
    border-radius: 0.5rem;
    color: #71717a;
    font-family: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .tab:hover {
    background: rgba(255, 255, 255, 0.04);
    color: #a1a1aa;
  }
  
  .tab.active {
    background: rgba(239, 68, 68, 0.1);
    color: #ef4444;
  }
  
  .tab-icon {
    font-size: 1rem;
  }
  
  .content {
    flex: 1;
    overflow: hidden;
    padding: 1.5rem;
  }
</style>
