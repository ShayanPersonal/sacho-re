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
      <button class="search-btn" onclick={handleSearch}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
      </button>
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
                        <svg class="meta-icon" title="MIDI" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="6" width="20" height="12" rx="1"/><line x1="6" y1="10" x2="6" y2="14"/><line x1="10" y1="10" x2="10" y2="14"/><line x1="14" y1="10" x2="14" y2="14"/><line x1="18" y1="10" x2="18" y2="14"/></svg>
                      {/if}
                      {#if session.has_audio}
                        <svg class="meta-icon" title="Audio" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/></svg>
                      {/if}
                      {#if session.has_video}
                        <svg class="meta-icon" title="Video" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="5" width="14" height="14" rx="2"/><path d="M16 10l6-4v12l-6-4"/></svg>
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
        <svg class="no-selection-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1">
          <circle cx="12" cy="12" r="10"/>
          <circle cx="12" cy="12" r="3"/>
          <path d="M12 2v4M12 18v4M2 12h4M18 12h4"/>
        </svg>
        <p>Select a recording to view</p>
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
    width: 300px;
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
    background: rgba(255, 255, 255, 0.015);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    padding: 1rem;
  }
  
  .search-bar {
    display: flex;
    gap: 0.5rem;
  }
  
  .search-bar input {
    flex: 1;
    padding: 0.5rem 0.75rem;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #e8e6e3;
    font-family: inherit;
    font-size: 0.8125rem;
    letter-spacing: 0.01em;
  }
  
  .search-bar input::placeholder {
    color: #4a4a4a;
  }
  
  .search-bar input:focus {
    outline: none;
    border-color: rgba(201, 169, 98, 0.4);
  }
  
  .search-btn {
    padding: 0.5rem 0.625rem;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    cursor: pointer;
    color: #6b6b6b;
    transition: all 0.2s ease;
  }
  
  .search-btn:hover {
    color: #a8a8a8;
    border-color: rgba(255, 255, 255, 0.1);
  }
  
  .search-btn svg {
    width: 16px;
    height: 16px;
  }
  
  .filter-menu-container {
    position: relative;
  }
  
  .filter-btn {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4375rem 0.625rem;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    color: #6b6b6b;
    font-family: inherit;
    font-size: 0.75rem;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  
  .filter-btn:hover {
    color: #a8a8a8;
    border-color: rgba(255, 255, 255, 0.08);
  }
  
  .filter-btn.active {
    border-color: rgba(201, 169, 98, 0.3);
    color: #c9a962;
  }
  
  .filter-badge {
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    background: #c9a962;
    border-radius: 8px;
    font-size: 0.625rem;
    font-weight: 500;
    color: #141414;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  
  .filter-arrow {
    margin-left: auto;
    font-size: 0.5rem;
    opacity: 0.4;
  }
  
  .filter-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.25rem;
    min-width: 160px;
    background: #1a1a1a;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.25rem;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    z-index: 100;
    overflow: hidden;
  }
  
  .filter-option {
    display: flex;
    align-items: center;
    gap: 0.625rem;
    padding: 0.5rem 0.75rem;
    font-size: 0.8125rem;
    color: #a8a8a8;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  
  .filter-option:hover {
    background: rgba(255, 255, 255, 0.04);
  }
  
  .filter-option input[type="checkbox"] {
    width: 13px;
    height: 13px;
    margin: 0;
    accent-color: #c9a962;
    flex-shrink: 0;
  }
  
  .filter-option .filter-icon {
    font-size: 0.8125rem;
    width: 1.125rem;
    text-align: center;
    flex-shrink: 0;
    opacity: 0.6;
  }
  
  .filter-option .filter-label {
    flex: 1;
  }
  
  .filter-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.04);
    margin: 0.25rem 0;
  }
  
  .session-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
  }
  
  .loading, .empty {
    padding: 2rem;
    text-align: center;
    color: #4a4a4a;
    font-size: 0.8125rem;
  }
  
  .session-group {
    display: flex;
    flex-direction: column;
  }
  
  .group-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.25rem;
    background: transparent;
    border: none;
    color: #6b6b6b;
    font-family: inherit;
    font-size: 0.6875rem;
    font-weight: 400;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    cursor: pointer;
    transition: color 0.2s ease;
  }
  
  .group-header:hover {
    color: #8a8a8a;
  }
  
  .group-arrow {
    font-size: 0.5rem;
    color: #4a4a4a;
  }
  
  .group-count {
    color: #4a4a4a;
    font-weight: 400;
  }
  
  .group-sessions {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    padding-left: 0.75rem;
  }
  
  .session-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 0.625rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 0.25rem;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition: all 0.15s ease;
    min-width: 0;
  }
  
  .session-item:hover {
    background: rgba(255, 255, 255, 0.02);
  }
  
  .session-item.selected {
    background: rgba(201, 169, 98, 0.08);
    border-color: rgba(201, 169, 98, 0.15);
  }
  
  .session-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    overflow: hidden;
  }
  
  .favorite {
    color: #c9a962;
    font-size: 0.6875rem;
  }
  
  .session-time {
    color: #e8e6e3;
    font-size: 0.8125rem;
  }
  
  .session-title {
    color: #e8e6e3;
    font-size: 0.8125rem;
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  
  .session-duration {
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.6875rem;
    color: #5a5a5a;
    letter-spacing: 0.02em;
  }
  
  .session-meta {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    flex-shrink: 0;
  }
  
  .meta-icon {
    width: 14px;
    height: 14px;
    opacity: 0.7;
    stroke-width: 1.5;
    color: #8a8a8a;
  }
  
  .sidebar-actions {
    padding-top: 0.625rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
  }
  
  .refresh-btn {
    width: 100%;
    padding: 0.5rem;
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
  
  .refresh-btn:hover {
    color: #a8a8a8;
    border-color: rgba(255, 255, 255, 0.1);
  }
  
  .detail-panel {
    flex: 1;
    background: rgba(255, 255, 255, 0.015);
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
    overflow: hidden;
  }
  
  .no-selection {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 1.25rem;
    color: #4a4a4a;
  }
  
  .no-selection-icon {
    width: 48px;
    height: 48px;
    opacity: 0.3;
  }
  
  .no-selection p {
    font-size: 0.8125rem;
    letter-spacing: 0.02em;
  }
</style>
