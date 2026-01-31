<script lang="ts">
  import { 
    sessions, 
    groupedSessions, 
    selectedSession, 
    selectedSessionId,
    sessionFilter,
    isLoading,
    selectSession,
    deleteSessionById,
    updateFilter,
    refreshSessions
  } from '$lib/stores/sessions';
  import { formatDuration } from '$lib/api';
  import { ask } from '@tauri-apps/plugin-dialog';
  import SessionDetail from './SessionDetail.svelte';
  
  let searchQuery = $state('');
  let expandedGroups = $state<Set<string>>(new Set(['Today', 'Yesterday', 'This Week']));
  let filterMenuOpen = $state(false);
  
  // Count active filters
  let activeFilterCount = $derived.by(() => {
    let count = 0;
    if ($sessionFilter.favorites_only) count++;
    if ($sessionFilter.has_audio) count++;
    if ($sessionFilter.has_midi) count++;
    if ($sessionFilter.has_video) count++;
    if ($sessionFilter.has_notes) count++;
    return count;
  });
  
  function toggleGroup(group: string) {
    expandedGroups = new Set(expandedGroups);
    if (expandedGroups.has(group)) {
      expandedGroups.delete(group);
    } else {
      expandedGroups.add(group);
    }
  }
  
  function handleSearch() {
    updateFilter({ search: searchQuery || undefined });
  }
  
  async function handleDelete(sessionId: string) {
    const confirmed = await ask('Delete this session? This cannot be undone.', {
      title: 'Confirm Delete',
      kind: 'warning'
    });
    if (confirmed) {
      await deleteSessionById(sessionId);
    }
  }
  
  function toggleFilterMenu() {
    filterMenuOpen = !filterMenuOpen;
  }
  
  function closeFilterMenu(e: MouseEvent) {
    // Close if clicking outside the menu
    const target = e.target as HTMLElement;
    if (!target.closest('.filter-menu-container')) {
      filterMenuOpen = false;
    }
  }
</script>

<svelte:window onclick={closeFilterMenu} />

<div class="session-browser">
  <div class="sidebar">
    <div class="search-bar">
      <input 
        type="text" 
        placeholder="Search..." 
        bind:value={searchQuery}
        onkeydown={(e) => e.key === 'Enter' && handleSearch()}
      />
      <button class="search-btn" onclick={handleSearch}>🔍</button>
    </div>
    
    <div class="filter-menu-container">
      <button 
        class="filter-btn" 
        class:active={activeFilterCount > 0}
        onclick={(e) => { e.stopPropagation(); toggleFilterMenu(); }}
      >
        Filters
        {#if activeFilterCount > 0}
          <span class="filter-badge">{activeFilterCount}</span>
        {/if}
        <span class="filter-arrow">{filterMenuOpen ? '▲' : '▼'}</span>
      </button>
      
      {#if filterMenuOpen}
        <div class="filter-menu" onclick={(e) => e.stopPropagation()}>
          <label class="filter-option">
            <input 
              type="checkbox" 
              checked={$sessionFilter.favorites_only}
              onchange={() => updateFilter({ favorites_only: !$sessionFilter.favorites_only })}
            />
            <span class="filter-icon">★</span>
            <span class="filter-label">Favorites only</span>
          </label>
          <label class="filter-option">
            <input 
              type="checkbox" 
              checked={$sessionFilter.has_notes === true}
              onchange={() => updateFilter({ has_notes: $sessionFilter.has_notes ? undefined : true })}
            />
            <span class="filter-icon">📝</span>
            <span class="filter-label">Has Notes</span>
          </label>
          <div class="filter-divider"></div>
          <label class="filter-option">
            <input 
              type="checkbox" 
              checked={$sessionFilter.has_audio === true}
              onchange={() => updateFilter({ has_audio: $sessionFilter.has_audio ? undefined : true })}
            />
            <span class="filter-icon">🎤</span>
            <span class="filter-label">Has Audio</span>
          </label>
          <label class="filter-option">
            <input 
              type="checkbox" 
              checked={$sessionFilter.has_midi === true}
              onchange={() => updateFilter({ has_midi: $sessionFilter.has_midi ? undefined : true })}
            />
            <span class="filter-icon">🎹</span>
            <span class="filter-label">Has MIDI</span>
          </label>
          <label class="filter-option">
            <input 
              type="checkbox" 
              checked={$sessionFilter.has_video === true}
              onchange={() => updateFilter({ has_video: $sessionFilter.has_video ? undefined : true })}
            />
            <span class="filter-icon">🎥</span>
            <span class="filter-label">Has Video</span>
          </label>
        </div>
      {/if}
    </div>
    
    <div class="session-list">
      {#if $isLoading}
        <div class="loading">Loading sessions...</div>
      {:else if $sessions.length === 0}
        <div class="empty">No sessions found</div>
      {:else}
        {#each Object.entries($groupedSessions) as [group, groupSessions]}
          <div class="session-group">
            <button 
              class="group-header"
              onclick={() => toggleGroup(group)}
            >
              <span class="group-arrow">{expandedGroups.has(group) ? '▼' : '▶'}</span>
              <span class="group-name">{group}</span>
              <span class="group-count">({groupSessions.length})</span>
            </button>
            
            {#if expandedGroups.has(group)}
              <div class="group-sessions">
                {#each groupSessions as session}
                  <button 
                    class="session-item"
                    class:selected={$selectedSessionId === session.id}
                    onclick={() => selectSession(session.id)}
                  >
                    <div class="session-header">
                      {#if session.is_favorite}
                        <span class="favorite">★</span>
                      {/if}
                      {#if session.notes}
                        <span class="session-title" title={session.notes}>
                          {session.notes.split('\n')[0].slice(0, 18)}{session.notes.length > 18 ? '…' : ''}
                        </span>
                      {:else}
                        <span class="session-time">
                          {new Date(session.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                        </span>
                      {/if}
                    </div>
                    <div class="session-meta">
                      {#if session.has_midi}
                        <span class="meta-icon" title="MIDI">🎹</span>
                      {/if}
                      {#if session.has_audio}
                        <span class="meta-icon" title="Audio">🎤</span>
                      {/if}
                      {#if session.has_video}
                        <span class="meta-icon" title="Video">🎥</span>
                      {/if}
                      <span class="session-duration">{formatDuration(session.duration_secs)}</span>
                    </div>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
    
    <div class="sidebar-actions">
      <button class="refresh-btn" onclick={refreshSessions}>
        Refresh
      </button>
    </div>
  </div>
  
  <div class="detail-panel">
    {#if $selectedSession}
      <SessionDetail 
        session={$selectedSession} 
        onDelete={() => handleDelete($selectedSession!.id)}
      />
    {:else}
      <div class="no-selection">
        <div class="no-selection-icon">📁</div>
        <p>Select a session to view details</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .session-browser {
    display: flex;
    height: 100%;
    gap: 1.5rem;
  }
  
  .sidebar {
    width: 320px;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.75rem;
    padding: 1rem;
  }
  
  .search-bar {
    display: flex;
    gap: 0.5rem;
  }
  
  .search-bar input {
    flex: 1;
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
  
  .search-btn {
    padding: 0.625rem 0.75rem;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    cursor: pointer;
  }
  
  .filter-menu-container {
    position: relative;
  }
  
  .filter-btn {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.5rem;
    color: #a1a1aa;
    font-family: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
    transition: all 0.1s ease;
  }
  
  .filter-btn:hover {
    background: rgba(255, 255, 255, 0.06);
    color: #e4e4e7;
  }
  
  .filter-btn.active {
    background: rgba(239, 68, 68, 0.1);
    border-color: rgba(239, 68, 68, 0.2);
    color: #e4e4e7;
  }
  
  .filter-badge {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    background: #ef4444;
    border-radius: 9px;
    font-size: 0.6875rem;
    font-weight: 600;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  
  .filter-arrow {
    margin-left: auto;
    font-size: 0.625rem;
    opacity: 0.5;
  }
  
  .filter-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.25rem;
    min-width: 150px;
    background: #1c1c1e;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 0.5rem;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 100;
    overflow: hidden;
  }
  
  .filter-option {
    display: flex;
    align-items: center;
    gap: 0.625rem;
    padding: 0.625rem 0.75rem;
    font-size: 0.8125rem;
    color: #e4e4e7;
    cursor: pointer;
    transition: background 0.1s ease;
  }
  
  .filter-option:hover {
    background: rgba(255, 255, 255, 0.05);
  }
  
  .filter-option input[type="checkbox"] {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: #ef4444;
    flex-shrink: 0;
  }
  
  .filter-option .filter-icon {
    font-size: 0.875rem;
    width: 1.25rem;
    text-align: center;
    flex-shrink: 0;
  }
  
  .filter-option .filter-label {
    flex: 1;
  }
  
  .filter-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.06);
    margin: 0.25rem 0;
  }
  
  .session-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  
  .loading, .empty {
    padding: 2rem;
    text-align: center;
    color: #52525b;
  }
  
  .session-group {
    display: flex;
    flex-direction: column;
  }
  
  .group-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem;
    background: transparent;
    border: none;
    color: #a1a1aa;
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    cursor: pointer;
  }
  
  .group-arrow {
    font-size: 0.625rem;
    color: #52525b;
  }
  
  .group-count {
    color: #52525b;
    font-weight: 400;
  }
  
  .group-sessions {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding-left: 1rem;
  }
  
  .session-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.625rem 0.75rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 0.5rem;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition: all 0.1s ease;
    min-width: 0;
  }
  
  .session-item:hover {
    background: rgba(255, 255, 255, 0.03);
  }
  
  .session-item.selected {
    background: rgba(239, 68, 68, 0.1);
    border-color: rgba(239, 68, 68, 0.2);
  }
  
  .session-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    overflow: hidden;
  }
  
  .favorite {
    color: #eab308;
    font-size: 0.75rem;
  }
  
  .session-time {
    color: #e4e4e7;
    font-size: 0.875rem;
  }
  
  .session-title {
    color: #e4e4e7;
    font-size: 0.8125rem;
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  
  .session-duration {
    font-family: 'JetBrains Mono', monospace;
    font-size: 0.75rem;
    color: #71717a;
  }
  
  .session-meta {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    flex-shrink: 0;
  }
  
  .meta-icon {
    font-size: 0.625rem;
    opacity: 0.4;
  }
  
  .sidebar-actions {
    padding-top: 0.5rem;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
  }
  
  .refresh-btn {
    width: 100%;
    padding: 0.625rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    color: #a1a1aa;
    font-family: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .refresh-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #e4e4e7;
  }
  
  .detail-panel {
    flex: 1;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.75rem;
    overflow: hidden;
  }
  
  .no-selection {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 1rem;
    color: #52525b;
  }
  
  .no-selection-icon {
    font-size: 3rem;
    opacity: 0.5;
  }
</style>
