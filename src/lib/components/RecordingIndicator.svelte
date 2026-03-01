<script lang="ts">
    import {
        recordingState,
        isRecording,
        isInitializing,
        canRecord,
        doStartRecording,
        doStopRecording,
    } from "$lib/stores/recording";
    import { settings } from "$lib/stores/settings";
    import {
        audioDeviceCount,
        midiDeviceCount,
        videoDeviceCount,
    } from "$lib/stores/devices";
    import { formatDuration } from "$lib/api";

    let isLoading = $state(false);

    // Check if auto-record is configured (has trigger MIDI or audio devices)
    let hasTrigger = $derived(
        ($settings?.trigger_midi_devices &&
            $settings.trigger_midi_devices.length > 0) ||
        ($settings?.trigger_audio_devices &&
            $settings.trigger_audio_devices.length > 0),
    );

    // Track counts for each device type (filtered by actual existing devices)
    let midiCount = $derived($midiDeviceCount.selected);
    let audioCount = $derived($audioDeviceCount.selected);
    let videoCount = $derived($videoDeviceCount.selected);

    // No devices selected at all
    let noDevices = $derived(
        midiCount === 0 && audioCount === 0 && videoCount === 0,
    );

    // Trigger set but nothing to record
    let triggerNoRecord = $derived(hasTrigger && noDevices);

    // Button should be disabled during loading, stopping, initializing, or if no devices
    let buttonDisabled = $derived(
        isLoading ||
            $recordingState.status === "stopping" ||
            $recordingState.status === "initializing" ||
            (noDevices && !$isRecording),
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
            console.error("Recording toggle failed:", error);
        } finally {
            isLoading = false;
        }
    }
</script>

<div
    class="recording-indicator"
    class:recording={$isRecording}
    class:initializing={$isInitializing}
>
    <div class="status-container">
        {#if $isRecording}
            <div class="status">
                <div class="status-dot active"></div>
                <span class="status-text recording">Recording</span>
            </div>
        {:else if $recordingState.status === "stopping"}
            <div class="status">
                <div class="status-dot"></div>
                <span class="status-text">Stopping...</span>
            </div>
        {:else if $isInitializing}
            <div class="status">
                <div class="status-dot initializing"></div>
                <span class="status-text initializing">Initializing...</span>
            </div>
        {:else}
            <div class="track-counts">
                <span
                    class="track-count"
                    class:empty={midiCount === 0}
                    title="MIDI devices"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.5"
                        ><rect x="2" y="6" width="20" height="12" rx="1" /><line
                            x1="5"
                            y1="10"
                            x2="5"
                            y2="14"
                        /><line x1="8" y1="10" x2="8" y2="14" /><line
                            x1="11"
                            y1="10"
                            x2="11"
                            y2="14"
                        /><line x1="14" y1="10" x2="14" y2="14" /><line
                            x1="17"
                            y1="10"
                            x2="17"
                            y2="14"
                        /></svg
                    >
                    {midiCount}
                </span>
                <span
                    class="track-count"
                    class:empty={audioCount === 0}
                    title="Audio devices"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.5"
                        ><path
                            d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"
                        /><path d="M19 10v2a7 7 0 0 1-14 0v-2" /><line
                            x1="12"
                            y1="19"
                            x2="12"
                            y2="23"
                        /></svg
                    >
                    {audioCount}
                </span>
                <span
                    class="track-count"
                    class:empty={videoCount === 0}
                    title="Video devices"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.5"
                        ><rect x="2" y="5" width="14" height="14" rx="2" /><path
                            d="M16 10l6-4v12l-6-4"
                        /></svg
                    >
                    {videoCount}
                </span>
            </div>
            <div
                class="trigger-status"
                class:ready={hasTrigger && !noDevices}
                class:manual={!hasTrigger && !noDevices}
                class:warning={triggerNoRecord}
            >
                {#if noDevices && !hasTrigger}
                    ⚠ No device selected
                {:else if triggerNoRecord}
                    ⚠ Trigger set, but no devices to record
                {:else if hasTrigger}
                    Waiting for trigger<span class="ellipsis"></span>
                {:else}
                    ⚠ No trigger set
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
        title="Manually start and stop recording"
    >
        {#if $isRecording}
            <svg class="btn-icon" viewBox="0 0 24 24" fill="currentColor"
                ><rect x="6" y="6" width="12" height="12" rx="1" /></svg
            >
            Stop
        {:else if $isInitializing}
            <svg
                class="btn-icon"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                ><circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" /></svg
            >
            Wait
        {:else}
            <svg class="btn-icon" viewBox="0 0 24 24" fill="currentColor"
                ><circle cx="12" cy="12" r="8" /></svg
            >
            Record
        {/if}
    </button>
</div>

<style>
    .recording-indicator {
        display: flex;
        align-items: center;
        gap: 1rem;
        padding: 0.5rem 0.875rem;
        background: rgba(255, 255, 255, 0.02);
        border: 1px solid rgba(255, 255, 255, 0.04);
        border-radius: 0.25rem;
        transition: all 0.3s ease;
    }

    .recording-indicator.recording {
        background: rgba(180, 60, 60, 0.1);
        border-color: rgba(180, 60, 60, 0.25);
    }

    .recording-indicator.initializing {
        background: rgba(201, 169, 98, 0.08);
        border-color: rgba(201, 169, 98, 0.2);
    }

    .status-container {
        display: flex;
        flex-direction: column;
        gap: 0.1875rem;
    }

    .status {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .track-counts {
        display: flex;
        align-items: center;
        gap: 0.75rem;
    }

    .track-count {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        font-size: 0.75rem;
        color: #8a8a8a;
    }

    .track-count svg {
        width: 14px;
        height: 14px;
        opacity: 0.7;
    }

    .track-count.empty {
        opacity: 0.35;
    }

    .trigger-status {
        font-size: 0.6875rem;
        color: #5a5a5a;
        letter-spacing: 0.02em;
    }

    .trigger-status.ready {
        color: #5a5a5a;
    }

    .trigger-status.manual {
        color: #5a5a5a;
    }

    .trigger-status.warning {
        color: #5a5a5a;
    }

    .ellipsis {
        display: inline-block;
        width: 1em;
        text-align: left;
    }

    .ellipsis::after {
        content: "";
        animation: ellipsis 2s infinite;
    }

    @keyframes ellipsis {
        0% {
            content: "";
        }
        25% {
            content: ".";
        }
        50% {
            content: "..";
        }
        75% {
            content: "...";
        }
        100% {
            content: "";
        }
    }

    .status-dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: #4a4a4a;
        transition: all 0.3s ease;
    }

    .status-dot.active {
        background: #b43c3c;
        box-shadow: 0 0 10px rgba(180, 60, 60, 0.6);
        animation: pulse-glow 2s ease-in-out infinite;
    }

    .status-dot.initializing {
        background: #c9a962;
        box-shadow: 0 0 8px rgba(201, 169, 98, 0.4);
        animation: pulse 2s ease-in-out infinite;
    }

    @keyframes pulse {
        0%,
        100% {
            transform: scale(1);
            opacity: 1;
        }
        50% {
            transform: scale(1.15);
            opacity: 0.8;
        }
    }

    @keyframes pulse-glow {
        0%,
        100% {
            box-shadow: 0 0 10px rgba(180, 60, 60, 0.6);
        }
        50% {
            box-shadow: 0 0 16px rgba(180, 60, 60, 0.8);
        }
    }

    .status-text {
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.6875rem;
        font-weight: 400;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: #8a8a8a;
    }

    .status-text.recording {
        color: #b43c3c;
    }

    .status-text.initializing {
        color: #c9a962;
    }

    .elapsed {
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.9375rem;
        font-weight: 400;
        color: #e8e6e3;
        min-width: 56px;
        letter-spacing: 0.02em;
    }

    .control-btn {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.5rem 0.875rem;
        background: transparent;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 0.25rem;
        color: #6b6b6b;
        font-family: inherit;
        font-size: 0.75rem;
        font-weight: 400;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        cursor: pointer;
        transition: all 0.2s ease;
    }

    .control-btn:hover:not(:disabled) {
        border-color: rgba(255, 255, 255, 0.12);
        color: #a8a8a8;
    }

    .control-btn.stop {
        border-color: rgba(180, 60, 60, 0.4);
        color: #b43c3c;
    }

    .control-btn.stop:hover:not(:disabled) {
        background: rgba(180, 60, 60, 0.1);
        border-color: rgba(180, 60, 60, 0.5);
    }

    .control-btn:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .btn-icon {
        width: 12px;
        height: 12px;
    }

    /* Light mode overrides */
    :global(body.light-mode) .recording-indicator {
        background: rgba(0, 0, 0, 0.03);
        border-color: rgba(0, 0, 0, 0.1);
    }

    :global(body.light-mode) .recording-indicator.recording {
        background: rgba(180, 60, 60, 0.08);
        border-color: rgba(180, 60, 60, 0.2);
    }

    :global(body.light-mode) .recording-indicator.initializing {
        background: rgba(160, 128, 48, 0.08);
        border-color: rgba(160, 128, 48, 0.2);
    }

    :global(body.light-mode) .track-count {
        color: #5a5a5a;
    }

    :global(body.light-mode) .track-count.empty {
        opacity: 0.4;
    }

    :global(body.light-mode) .trigger-status {
        color: #7a7a7a;
    }

    :global(body.light-mode) .trigger-status.manual {
        color: #7a7a7a;
    }

    :global(body.light-mode) .trigger-status.warning {
        color: #7a7a7a;
    }

    :global(body.light-mode) .status-dot {
        background: #8a8a8a;
    }

    :global(body.light-mode) .status-text {
        color: #5a5a5a;
    }

    :global(body.light-mode) .elapsed {
        color: #2a2a2a;
    }

    :global(body.light-mode) .control-btn {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode) .control-btn:hover:not(:disabled) {
        border-color: rgba(0, 0, 0, 0.2);
        color: #3a3a3a;
    }

    :global(body.light-mode) .control-btn.stop {
        border-color: rgba(180, 60, 60, 0.4);
        color: #b43c3c;
    }

    :global(body.light-mode) .control-btn.stop:hover:not(:disabled) {
        background: rgba(180, 60, 60, 0.1);
        border-color: rgba(180, 60, 60, 0.5);
    }
</style>
