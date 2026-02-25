<script lang="ts">
    import { onMount } from "svelte";
    import RecordingIndicator from "$lib/components/RecordingIndicator.svelte";
    import SessionBrowser from "$lib/components/sessions/SessionBrowser.svelte";
    import SimilarityTab from "$lib/components/similarity/SimilarityTab.svelte";
    import DevicePanel from "$lib/components/devices/DevicePanel.svelte";
    import Settings from "$lib/components/Settings.svelte";
    import {
        refreshRecordingState,
        isRecording,
        isStopping,
    } from "$lib/stores/recording";
    import { settings } from "$lib/stores/settings";
    import {
        disconnectedDeviceInfos,
        disconnectBannerDismissed,
    } from "$lib/stores/devices";

    // Devices and Settings tabs are locked while recording
    let recordingLocked = $derived($isRecording || $isStopping);

    type Tab = "sessions" | "similarity" | "devices" | "settings";
    let activeTab: Tab = $state("sessions");

    // Reactive dark mode from settings (default is light mode)
    let isDarkMode = $derived($settings?.dark_mode ?? false);

    // Apply light mode class to document body for global styling (light is default)
    $effect(() => {
        if (isDarkMode) {
            document.body.classList.remove("light-mode");
        } else {
            document.body.classList.add("light-mode");
        }
    });

    onMount(() => {
        // Refresh recording state periodically
        const interval = setInterval(refreshRecordingState, 1000);
        return () => clearInterval(interval);
    });
</script>

<div class="app" class:light-mode={!isDarkMode}>
    <nav class="tabs">
        <button
            class="tab"
            class:active={activeTab === "sessions"}
            onclick={() => (activeTab = "sessions")}
        >
            <svg
                class="tab-icon icon-recordings"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
            >
                <circle cx="12" cy="12" r="10" />
                <circle cx="12" cy="12" r="3" />
            </svg>
            Recordings
        </button>

        <button
            class="tab"
            class:active={activeTab === "similarity"}
            onclick={() => (activeTab = "similarity")}
        >
            <svg
                class="tab-icon icon-visualize"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
            >
                <path d="M3 3v18h18" />
                <path d="M7 16l4-4 4 4 6-6" />
            </svg>
            Similarity
        </button>
        <button
            class="tab"
            class:active={activeTab === "devices"}
            onclick={() => (activeTab = "devices")}
        >
            <svg
                class="tab-icon icon-devices"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
            >
                <rect x="4" y="4" width="16" height="16" rx="2" />
                <circle cx="9" cy="9" r="1.5" fill="currentColor" />
                <circle cx="15" cy="9" r="1.5" fill="currentColor" />
                <circle cx="9" cy="15" r="1.5" fill="currentColor" />
                <circle cx="15" cy="15" r="1.5" fill="currentColor" />
            </svg>
            Devices
        </button>
        <button
            class="tab"
            class:active={activeTab === "settings"}
            onclick={() => (activeTab = "settings")}
        >
            <svg
                class="tab-icon icon-settings"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
            >
                <circle cx="12" cy="12" r="3" />
                <path
                    d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83"
                />
            </svg>
            Settings
        </button>
        <div class="spacer"></div>
        <RecordingIndicator />
    </nav>

    {#if $disconnectedDeviceInfos.length > 0 && !$disconnectBannerDismissed}
        <div class="disconnect-banner">
            <svg
                class="disconnect-banner-icon"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
            >
                <path
                    d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"
                />
                <line x1="12" y1="9" x2="12" y2="13" />
                <line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
            <span class="disconnect-banner-text">
                {#if $disconnectedDeviceInfos.length === 1}
                    {$disconnectedDeviceInfos[0].name} has disconnected
                {:else}
                    {$disconnectedDeviceInfos.length} devices have disconnected: {$disconnectedDeviceInfos.map(d => d.name).join(", ")}
                {/if}
            </span>
            <button
                class="disconnect-banner-close"
                onclick={() => disconnectBannerDismissed.set(true)}
                title="Dismiss"
            >&times;</button>
        </div>
    {/if}

    <main class="content">
        {#if activeTab === "sessions"}
            <SessionBrowser />
        {:else if activeTab === "similarity"}
            <SimilarityTab />
        {:else if activeTab === "devices"}
            <div class="lockable-content">
                <DevicePanel />
                {#if recordingLocked}
                    <div class="recording-lock-overlay">
                        <div class="lock-message">
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.5"
                                width="32"
                                height="32"
                            >
                                <rect
                                    x="3"
                                    y="11"
                                    width="18"
                                    height="11"
                                    rx="2"
                                />
                                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                            </svg>
                            <span>Recording in progress</span>
                        </div>
                    </div>
                {/if}
            </div>
        {:else if activeTab === "settings"}
            <div class="lockable-content">
                <Settings />
                {#if recordingLocked}
                    <div class="recording-lock-overlay">
                        <div class="lock-message">
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.5"
                                width="32"
                                height="32"
                            >
                                <rect
                                    x="3"
                                    y="11"
                                    width="18"
                                    height="11"
                                    rx="2"
                                />
                                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                            </svg>
                            <span>Recording in progress</span>
                        </div>
                    </div>
                {/if}
            </div>
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
        font-family:
            "Roboto",
            -apple-system,
            BlinkMacSystemFont,
            sans-serif;
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
        font-family: "Bebas Neue", Impact, "Arial Narrow", sans-serif;
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
        background: rgb(14, 14, 12);
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
        content: "";
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
        text-shadow:
            0 0 8px rgba(219, 187, 116, 0.5),
            0 0 16px rgba(219, 187, 116, 0.25);
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
        text-shadow:
            0 0 8px rgba(160, 128, 48, 0.3),
            0 0 16px rgba(160, 128, 48, 0.15);
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

    /* Help buttons and tooltips */
    :global(body.light-mode .help-btn) {
        background: rgba(0, 0, 0, 0.08);
        color: #6a6a6a;
    }

    :global(body.light-mode .help-btn:hover) {
        background: rgba(0, 0, 0, 0.12);
        color: #4a4a4a;
    }

    :global(body.light-mode .help-tooltip) {
        background: #ffffff;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
        color: #4a4a4a;
    }

    :global(body.light-mode .help-tooltip strong) {
        color: #2a2a2a;
    }

    /* Browse and action buttons */
    :global(body.light-mode .browse-btn),
    :global(body.light-mode .action-btn),
    :global(body.light-mode .refresh-btn),
    :global(body.light-mode .search-btn),
    :global(body.light-mode .filter-btn) {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode .browse-btn:hover),
    :global(body.light-mode .action-btn:hover),
    :global(body.light-mode .refresh-btn:hover),
    :global(body.light-mode .search-btn:hover),
    :global(body.light-mode .filter-btn:hover) {
        color: #3a3a3a;
        border-color: rgba(0, 0, 0, 0.2);
    }

    :global(body.light-mode .action-btn.primary),
    :global(body.light-mode .filter-btn.active) {
        border-color: rgba(160, 128, 48, 0.4);
        color: #8a6a20;
    }

    :global(body.light-mode .action-btn.primary:hover) {
        background: rgba(160, 128, 48, 0.1);
    }

    /* Select dropdown options */
    :global(body.light-mode select option) {
        background: #ffffff;
        color: #2a2a2a;
    }

    /* Section headers in device panel and settings */
    :global(body.light-mode .settings-section h3),
    :global(body.light-mode .section-header) {
        color: #6a6a6a;
    }

    :global(body.light-mode .settings-section h3) {
        border-bottom-color: rgba(0, 0, 0, 0.08);
    }

    /* Save status */
    :global(body.light-mode .save-status.saving) {
        background: rgba(0, 0, 0, 0.05);
        color: #6a6a6a;
    }

    :global(body.light-mode .save-status.saved) {
        background: rgba(160, 128, 48, 0.12);
        color: #8a6a20;
    }

    /* Filter menu */
    :global(body.light-mode .filter-menu) {
        background: #ffffff;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
    }

    :global(body.light-mode .filter-option) {
        color: #4a4a4a;
    }

    :global(body.light-mode .filter-option:hover) {
        background: rgba(0, 0, 0, 0.04);
    }

    :global(body.light-mode .filter-divider) {
        background: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .filter-badge) {
        background: #a08030;
        color: #ffffff;
    }

    /* Group headers in session list */
    :global(body.light-mode .group-header) {
        color: #5a5a5a;
    }

    :global(body.light-mode .group-header:hover) {
        color: #3a3a3a;
    }

    :global(body.light-mode .group-arrow),
    :global(body.light-mode .group-count),
    :global(body.light-mode .section-arrow),
    :global(body.light-mode .section-count) {
        color: #8a8a8a;
    }

    /* Meta tags and badges */
    :global(body.light-mode .meta-tag) {
        background: rgba(0, 0, 0, 0.06);
        color: #5a5a5a;
    }

    :global(body.light-mode .meta-tag.default) {
        background: rgba(160, 128, 48, 0.15);
        color: #8a6a20;
    }

    :global(body.light-mode .meta-tag.unsupported) {
        background: rgba(180, 60, 60, 0.1);
        color: #a04040;
    }

    /* Codec tags */
    :global(body.light-mode .codec-tag) {
        background: rgba(0, 0, 0, 0.04);
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode .codec-tag:hover) {
        background: rgba(0, 0, 0, 0.08);
        color: #3a3a3a;
    }

    :global(body.light-mode .codec-tag.codec-selected) {
        background: rgba(160, 128, 48, 0.15);
        border-color: rgba(160, 128, 48, 0.4);
        color: #8a6a20;
    }

    /* Device rows and sections */
    :global(body.light-mode .device-section) {
        background: rgba(255, 255, 255, 0.7);
        border-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .device-row) {
        background: rgba(0, 0, 0, 0.03);
    }

    :global(body.light-mode .device-name) {
        color: #3a3a3a;
    }

    /* Session items */
    :global(body.light-mode .session-item.selected) {
        background: rgba(160, 128, 48, 0.12);
        border-color: rgba(160, 128, 48, 0.25);
    }

    /* Sidebar and panels */
    :global(body.light-mode .sidebar) {
        background: rgba(255, 255, 255, 0.7);
        border-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .detail-panel) {
        background: rgba(255, 255, 255, 0.7);
        border-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .sidebar-actions) {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    /* No selection state */
    :global(body.light-mode .no-selection) {
        color: #8a8a8a;
    }

    :global(body.light-mode .no-selection-icon) {
        opacity: 0.4;
    }

    /* More menu */
    :global(body.light-mode .more-menu) {
        background: #ffffff;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
    }

    :global(body.light-mode .more-menu-item) {
        color: #3a3a3a;
    }

    :global(body.light-mode .more-menu-item:hover) {
        background: rgba(0, 0, 0, 0.04);
    }

    :global(body.light-mode .more-menu-item.danger) {
        color: #c04040;
    }

    :global(body.light-mode .more-menu-item.danger:hover) {
        background: rgba(180, 60, 60, 0.1);
    }

    /* Player section */
    :global(body.light-mode .player-section) {
        background: rgba(245, 245, 240, 1);
        border-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .video-container) {
        border-color: rgba(0, 0, 0, 0.12);
    }

    /* Control buttons */
    :global(body.light-mode .control-btn) {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode .control-btn:hover:not(:disabled)) {
        border-color: rgba(0, 0, 0, 0.2);
        color: #3a3a3a;
    }

    /* Track controls */
    :global(body.light-mode .track-control) {
        background: rgba(0, 0, 0, 0.03);
    }

    :global(body.light-mode .mute-btn) {
        background: rgba(0, 0, 0, 0.05);
        border-color: rgba(0, 0, 0, 0.1);
        color: #5a5a5a;
    }

    :global(body.light-mode .mute-btn:hover) {
        background: rgba(0, 0, 0, 0.08);
        color: #2a2a2a;
    }

    :global(body.light-mode .track-label) {
        color: #5a8a4a;
    }

    :global(body.light-mode .track-label.midi) {
        color: #8a6a20;
    }

    :global(body.light-mode .track-info) {
        color: #5a5a5a;
    }

    :global(body.light-mode .switch-btn) {
        background: rgba(0, 0, 0, 0.05);
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode .switch-btn:hover) {
        background: rgba(0, 0, 0, 0.08);
        color: #2a2a2a;
    }

    /* Notes input */
    :global(body.light-mode .notes-section) {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .notes-input) {
        background: rgba(255, 255, 255, 0.8);
        border-color: rgba(0, 0, 0, 0.12);
        color: #2a2a2a;
    }

    :global(body.light-mode .notes-input::placeholder) {
        color: #8a8a8a;
    }

    :global(body.light-mode .notes-input:focus) {
        border-color: rgba(180, 60, 60, 0.4);
    }

    /* Tags */
    :global(body.light-mode .tag) {
        background: rgba(160, 128, 48, 0.12);
        border-color: rgba(160, 128, 48, 0.3);
        color: #8a6a20;
    }

    /* Detail headers and borders */
    :global(body.light-mode .detail-header) {
        border-bottom-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .detail-actions) {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode .session-title) {
        color: #2a2a2a;
    }

    :global(body.light-mode .session-time) {
        color: #2a2a2a;
    }

    /* Time display */
    :global(body.light-mode .time-display) {
        color: #5a5a5a;
    }

    :global(body.light-mode .elapsed) {
        color: #2a2a2a;
    }

    /* Similarity tab overrides handled by SimilarityTab.svelte scoped styles */

    /* Empty and loading states */
    :global(body.light-mode .loading),
    :global(body.light-mode .empty),
    :global(body.light-mode .empty-message) {
        color: #8a8a8a;
    }

    /* Error states */
    :global(body.light-mode .error-banner) {
        background: rgba(200, 60, 60, 0.1);
        border-color: rgba(200, 60, 60, 0.3);
        color: #a04040;
    }

    /* Meta icons */
    :global(body.light-mode .meta-icon) {
        color: #5a5a5a;
    }

    /* MIDI/Video header columns */
    :global(body.light-mode .midi-header),
    :global(body.light-mode .video-header) {
        color: #7a7a7a;
    }

    /* Seek bar */
    :global(body.light-mode .seek-bar) {
        background: rgba(0, 0, 0, 0.12);
    }

    /* Recording lock overlay */
    .lockable-content {
        position: relative;
        height: 100%;
        overflow: hidden;
    }

    .recording-lock-overlay {
        position: absolute;
        inset: 0;
        background: rgba(14, 14, 12, 0.55);
        backdrop-filter: blur(3px);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 50;
        border-radius: 0.5rem;
    }

    .lock-message {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.75rem;
        color: #8a8a8a;
        font-size: 0.9rem;
        font-weight: 500;
        letter-spacing: 0.03em;
        text-transform: uppercase;
    }

    .lock-message svg {
        color: rgba(219, 187, 116, 0.6);
    }

    .app.light-mode .recording-lock-overlay {
        background: rgba(245, 245, 243, 0.6);
    }

    .app.light-mode .lock-message {
        color: #6a6a6a;
    }

    .app.light-mode .lock-message svg {
        color: rgba(160, 128, 48, 0.6);
    }

    /* Disconnect warning banner */
    .disconnect-banner {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 1rem;
        background: rgba(200, 60, 60, 0.15);
        border-bottom: 1px solid rgba(200, 60, 60, 0.3);
        color: #e57373;
        font-size: 0.8125rem;
    }

    .disconnect-banner-icon {
        width: 16px;
        height: 16px;
        flex-shrink: 0;
        stroke: #e57373;
    }

    .disconnect-banner-text {
        flex: 1;
        color: #e57373;
    }

    .disconnect-banner-close {
        background: none;
        border: none;
        color: #e57373;
        font-size: 1.125rem;
        cursor: pointer;
        padding: 0 0.25rem;
        line-height: 1;
        opacity: 0.7;
        transition: opacity 0.15s ease;
    }

    .disconnect-banner-close:hover {
        opacity: 1;
    }

    .app.light-mode .disconnect-banner {
        background: rgba(200, 60, 60, 0.1);
        border-bottom-color: rgba(200, 60, 60, 0.25);
        color: #c04040;
    }

    .app.light-mode .disconnect-banner-icon {
        stroke: #c04040;
    }

    .app.light-mode .disconnect-banner-text {
        color: #c04040;
    }

    .app.light-mode .disconnect-banner-close {
        color: #c04040;
    }
</style>
