<script lang="ts">
    import {
        settings,
        saveSettings,
        saveSettingsDebounced,
        saveStatus,
    } from "$lib/stores/settings";
    import { open } from "@tauri-apps/plugin-dialog";
    import type { Config, AutostartInfo, AppStats } from "$lib/api";
    import {
        getAutostartInfo,
        setAllUsersAutostart,
        getAppStats,
    } from "$lib/api";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount, onDestroy } from "svelte";
    import {
        playStartSound,
        playStopSound,
        playDisconnectWarningSound,
        previewCustomSound,
    } from "$lib/sounds";
    import { setCustomSound, clearCustomSound } from "$lib/api";
    import { refreshSessions } from "$lib/stores/sessions";
    import About from "$lib/components/About.svelte";

    let showAbout = $state(false);

    function positionTooltip(node: HTMLElement) {
        const rect = node.getBoundingClientRect();
        if (rect.bottom > window.innerHeight) {
            node.style.top = "auto";
            node.style.bottom = "100%";
            node.style.marginTop = "0";
            node.style.marginBottom = "0.5rem";
        }
    }

    // Local editable copy
    let localSettings = $state<Config | null>(null);
    let showPrerollEncodeHelp = $state(false);
    let showAutostartHelp = $state(false);
    let showBackgroundHelp = $state(false);
    let showCombineHelp = $state(false);
    let showAudioAdvanced = $state(false);

    // All-users autostart state
    let autostartInfo = $state<AutostartInfo | null>(null);
    let allUsersToggling = $state(false);

    // App stats (CPU, RAM, Storage)
    let appStats = $state<AppStats | null>(null);
    let statsInterval: ReturnType<typeof setInterval> | null = null;

    /** Format bytes as GB with 1 decimal place (e.g. "0.2 GB", "62.7 GB") */
    function formatGB(bytes: number): string {
        return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
    }

    async function fetchStats() {
        try {
            appStats = await getAppStats();
        } catch (e) {
            // Silently ignore - stats are non-critical
        }
    }

    onMount(() => {
        fetchStats();
        statsInterval = setInterval(fetchStats, 1000);
    });

    onDestroy(() => {
        if (statsInterval) clearInterval(statsInterval);
    });

    // Load autostart info
    onMount(() => {
        getAutostartInfo()
            .then((info) => {
                autostartInfo = info;
            })
            .catch((e) => {
                console.error("Failed to get autostart info:", e);
            });
    });

    // Smart autostart toggle: uses HKLM (all-users) for per-machine installs,
    // HKCU (per-user) for per-user installs.
    function isAutostartEnabled(): boolean {
        if (autostartInfo?.is_per_machine_install) {
            return autostartInfo.all_users_autostart;
        }
        return localSettings?.auto_start ?? false;
    }

    async function handleAutostartToggle(event: Event) {
        const checkbox = event.target as HTMLInputElement;
        const newValue = checkbox.checked;

        if (autostartInfo?.is_per_machine_install) {
            // Per-machine install: toggle HKLM (may trigger UAC)
            checkbox.checked = !newValue; // revert while waiting
            allUsersToggling = true;
            try {
                await setAllUsersAutostart(newValue);
                checkbox.checked = newValue;
                autostartInfo = {
                    ...autostartInfo,
                    all_users_autostart: newValue,
                };
                // Also sync per-user config to match (disable per-user if all-users is on)
                if (localSettings) {
                    localSettings.auto_start = false;
                    autoSave();
                }
            } catch (e) {
                console.error("Failed to toggle autostart:", e);
            } finally {
                allUsersToggling = false;
            }
        } else {
            // Per-user install: toggle HKCU via tauri-plugin-autostart
            if (localSettings) {
                localSettings.auto_start = newValue;
                autoSave();
            }
        }
    }

    $effect(() => {
        if ($settings && !localSettings) {
            localSettings = { ...$settings };
        } else if ($settings && localSettings) {
            // Keep device selections in sync (they're managed by DevicePanel, not Settings)
            localSettings.selected_audio_devices =
                $settings.selected_audio_devices;
            localSettings.selected_midi_devices =
                $settings.selected_midi_devices;
            localSettings.trigger_midi_devices = $settings.trigger_midi_devices;
            localSettings.selected_video_devices =
                $settings.selected_video_devices;
            localSettings.video_device_configs = $settings.video_device_configs;
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

        // Clamp numeric values to valid ranges
        localSettings.idle_timeout_secs = Math.max(
            2,
            Math.min(30, localSettings.idle_timeout_secs),
        );
        const preRollMax = localSettings.encode_during_preroll ? 30 : 5;
        localSettings.pre_roll_secs = Math.max(
            0,
            Math.min(preRollMax, localSettings.pre_roll_secs),
        );

        saveSettingsDebounced(localSettings);
    }

    // Browse for recording location
    async function browseStoragePath() {
        if (!localSettings) return;

        const selected = await open({
            directory: true,
            multiple: false,
            defaultPath: localSettings.storage_path,
            title: "Select Recording Location",
        });

        if (selected && typeof selected === "string") {
            localSettings.storage_path = selected;
            await saveSettings(localSettings);
            refreshSessions();
        }
    }

    /** Extract just the filename from a relative path like "sounds/start_mysound.mp3" */
    function customSoundFilename(relativePath: string | null): string {
        if (!relativePath) return "";
        const parts = relativePath.split("/");
        const name = parts[parts.length - 1];
        // Strip the "start_", "stop_", or "disconnect_" prefix added by the backend
        const prefixMatch = name.match(/^(?:start|stop|disconnect)_(.+)$/);
        return prefixMatch ? prefixMatch[1] : name;
    }

    async function browseCustomSound(
        soundType: "start" | "stop" | "disconnect",
    ) {
        if (!localSettings) return;

        const selected = await open({
            multiple: false,
            title: `Select custom ${soundType} sound`,
            filters: [
                {
                    name: "Audio Files",
                    extensions: ["wav", "mp3", "ogg", "flac", "webm"],
                },
            ],
        });

        if (selected && typeof selected === "string") {
            try {
                const relativePath = await setCustomSound(selected, soundType);
                if (soundType === "start") {
                    localSettings.custom_sound_start = relativePath;
                } else if (soundType === "stop") {
                    localSettings.custom_sound_stop = relativePath;
                } else {
                    localSettings.custom_sound_disconnect = relativePath;
                }
                autoSave();
            } catch (e) {
                console.error("Failed to set custom sound:", e);
            }
        }
    }

    async function resetCustomSound(
        soundType: "start" | "stop" | "disconnect",
    ) {
        if (!localSettings) return;

        try {
            await clearCustomSound(soundType);
            if (soundType === "start") {
                localSettings.custom_sound_start = null;
            } else if (soundType === "stop") {
                localSettings.custom_sound_stop = null;
            } else {
                localSettings.custom_sound_disconnect = null;
            }
            autoSave();
        } catch (e) {
            console.error("Failed to clear custom sound:", e);
        }
    }
</script>

<div class="settings">
    <div class="settings-header">
        <h2>Settings</h2>
        <button class="about-btn" onclick={() => (showAbout = true)} title="About Sacho">
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
            >
                <circle cx="12" cy="12" r="10" />
                <path d="M12 16v-4M12 8h.01" />
            </svg>
            About
        </button>
        <div class="header-right">
            {#if $saveStatus === "saving" || $saveStatus === "saved"}
                <div
                    class="save-status"
                    class:saving={$saveStatus === "saving"}
                    class:saved={$saveStatus === "saved"}
                >
                    {#if $saveStatus === "saving"}
                        <svg
                            class="icon spinner"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                        >
                            <circle
                                cx="12"
                                cy="12"
                                r="10"
                                stroke-opacity="0.25"
                            />
                            <path
                                d="M12 2a10 10 0 0 1 10 10"
                                stroke-linecap="round"
                            />
                        </svg>
                        Saving...
                    {:else if $saveStatus === "saved"}
                        <svg
                            class="icon check"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2.5"
                        >
                            <polyline points="20 6 9 17 4 12" />
                        </svg>
                        Saved
                    {/if}
                </div>
            {/if}
        </div>
    </div>

    {#if localSettings}
        <div class="settings-content">
            <section class="settings-section">
                <div class="section-header">
                    <h3>Recording</h3>
                    {#if appStats}
                        <span
                            class="section-stats"
                            title="Current CPU and memory usage of the application (estimate)"
                            >CPU: {Math.round(appStats.cpu_percent)}% · RAM: {formatGB(
                                appStats.memory_bytes,
                            )}</span
                        >
                    {/if}
                </div>
                <div class="setting-row">
                    <label for="idle-timeout">
                        <span class="setting-label">Auto-recording timeout</span
                        >
                        <span class="setting-description"
                            >Stop automatic recording after no trigger activity
                            is detected for this length of time</span
                        >
                    </label>
                    <div class="input-with-suffix">
                        <input
                            id="idle-timeout"
                            type="number"
                            min="2"
                            max="30"
                            bind:value={localSettings.idle_timeout_secs}
                            oninput={autoSaveDebounced}
                        />
                        <span class="input-suffix">seconds</span>
                    </div>
                </div>

                <div class="setting-row">
                    <label for="pre-roll">
                        <span class="setting-label">Pre-roll Length</span>
                        <span class="setting-description"
                            >How much of the moments before playing to
                            retrospectively include at the start of recordings</span
                        >
                    </label>
                    <div class="input-with-suffix">
                        <input
                            id="pre-roll"
                            type="number"
                            min="0"
                            max={localSettings.encode_during_preroll ? 30 : 5}
                            bind:value={localSettings.pre_roll_secs}
                            oninput={autoSaveDebounced}
                        />
                        <span class="input-suffix">seconds</span>
                        {#if localSettings.pre_roll_secs === 0}
                            <span
                                class="input-suffix"
                                style="margin-left: 0.25rem;">(turned off)</span
                            >
                        {/if}
                        <span style="flex: 1;"></span>
                        <label
                            class="inline-checkbox"
                            class:inline-checkbox-disabled={localSettings.pre_roll_secs ===
                                0}
                        >
                            <input
                                type="checkbox"
                                bind:checked={
                                    localSettings.encode_during_preroll
                                }
                                disabled={localSettings.pre_roll_secs === 0}
                                onchange={() => {
                                    if (!localSettings) return;
                                    if (
                                        !localSettings.encode_during_preroll &&
                                        localSettings.pre_roll_secs > 5
                                    ) {
                                        localSettings.pre_roll_secs = 5;
                                    }
                                    autoSave();
                                }}
                            />
                            <span class="input-suffix"
                                >Encode during pre-roll</span
                            >
                        </label>
                        <span class="setting-label-with-help">
                            <button
                                class="help-btn"
                                onclick={() =>
                                    (showPrerollEncodeHelp =
                                        !showPrerollEncodeHelp)}
                                onblur={() => (showPrerollEncodeHelp = false)}
                            >
                                ?
                            </button>
                            {#if showPrerollEncodeHelp}
                                <div class="help-tooltip" use:positionTooltip>
                                    Increases the pre-roll limit from 5 to 30
                                    seconds at the cost of background CPU usage. <br
                                    /><br />Best combined with hardware
                                    acceleration. If not sure, leave this off.
                                </div>
                            {/if}
                        </span>
                    </div>
                </div>
            </section>

            <section class="settings-section">
                <div class="section-header">
                    <h3>Storage</h3>
                    {#if appStats}
                        <span
                            class="section-stats"
                            title="Size of the recordings folder, and remaining disk space on this system"
                            >Used {formatGB(appStats.storage_used_bytes)} · {formatGB(
                                appStats.disk_free_bytes,
                            )} remaining</span
                        >
                    {/if}
                </div>
                <div class="setting-row">
                    <label for="storage-path">
                        <span class="setting-label">Recording Location</span>
                        <span class="setting-description"
                            >Where to save and load recorded sessions</span
                        >
                    </label>
                    <div class="path-input">
                        <input
                            id="storage-path"
                            type="text"
                            bind:value={localSettings.storage_path}
                            readonly
                        />
                        <button class="browse-btn" onclick={browseStoragePath}
                            >Browse</button
                        >
                    </div>
                    <p class="setting-recommendation">
                        Tip: You can back up this folder to cloud storage and
                        open it on other instances of this app.
                    </p>
                </div>
                <div class="setting-row">
                    <div class="format-fields">
                        <div class="format-field">
                            <label for="audio-format">
                                <span class="setting-label">Audio File Type</span>
                            </label>
                            <select
                                id="audio-format"
                                bind:value={localSettings.audio_format}
                                onchange={autoSave}
                            >
                                <option value="wav">WAV (uses more disk space)</option>
                                <option value="flac">FLAC (uses less disk space)</option>
                            </select>
                        </div>
                        <div class="format-field">
                            <label for="video-container">
                                <span class="setting-label">Video File Type</span>
                            </label>
                            <select
                                id="video-container"
                                bind:value={localSettings.preferred_video_container}
                                onchange={autoSave}
                            >
                                <option value="mp4">MP4 (default)</option>
                                <option value="mkv">MKV</option>
                            </select>
                        </div>
                    </div>
                    <button
                        class="advanced-toggle"
                        onclick={() => (showAudioAdvanced = !showAudioAdvanced)}
                    >
                        More
                        <svg
                            class="toggle-chevron"
                            class:open={showAudioAdvanced}
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <polyline points="6 9 12 15 18 9"></polyline>
                        </svg>
                    </button>
                    {#if showAudioAdvanced}
                        <div class="advanced-audio-section">
                            <div class="advanced-audio-field">
                                <div class="advanced-field-header">
                                    <span class="setting-label"
                                        >Bit Depth ({localSettings.audio_format.toUpperCase()})</span
                                    >
                                    <span class="advanced-field-value">
                                        {#if localSettings.audio_format === "wav"}
                                            {localSettings.wav_bit_depth ===
                                            "int16"
                                                ? "16-bit"
                                                : localSettings.wav_bit_depth ===
                                                    "int24"
                                                  ? "24-bit"
                                                  : "32-bit float"}
                                        {:else}
                                            {localSettings.flac_bit_depth ===
                                            "int16"
                                                ? "16-bit"
                                                : localSettings.flac_bit_depth ===
                                                    "int24"
                                                  ? "24-bit"
                                                  : "32-bit"}
                                        {/if}
                                    </span>
                                </div>
                                {#if localSettings.audio_format === "wav"}
                                    <select
                                        bind:value={localSettings.wav_bit_depth}
                                        onchange={autoSave}
                                    >
                                        <option value="int16">16-bit</option>
                                        <option value="int24"
                                            >24-bit (default)</option
                                        >
                                        <option value="float32"
                                            >32-bit float</option
                                        >
                                    </select>
                                {:else}
                                    <select
                                        bind:value={
                                            localSettings.flac_bit_depth
                                        }
                                        onchange={autoSave}
                                    >
                                        <option value="int16">16-bit</option>
                                        <option value="int24"
                                            >24-bit (default)</option
                                        >
                                        <option value="float32"
                                            >32-bit (limited compatibility)</option
                                        >
                                    </select>
                                {/if}
                                <p class="advanced-field-description">
                                    {#if (localSettings.audio_format === "wav" ? localSettings.wav_bit_depth : localSettings.flac_bit_depth) === "int16"}
                                        {localSettings.audio_format === "flac"
                                            ? "Smallest files. Not optimal if you need to boost the volume of quiet sections."
                                            : "Smaller files. Not optimal if you need to boost the volume of quiet sections."}
                                    {:else if (localSettings.audio_format === "wav" ? localSettings.wav_bit_depth : localSettings.flac_bit_depth) === "int24"}
                                        Studio quality. Wide compatibility.
                                    {:else}
                                        {localSettings.audio_format === "flac"
                                            ? " Many programs do not support 32-bit FLAC recordings. Use at your own risk."
                                            : "Good if the audio source is also 32-bit float. Otherwise just uses more disk space."}
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
                <!--
        <div class="setting-row" style="margin-bottom: 0.5rem;">
          <div style="display: flex; align-items: center; gap: 0.5rem;">
            <label class="checkbox-row">
              <input
                type="checkbox"
                bind:checked={localSettings.combine_audio_video}
                disabled={($settings?.selected_video_devices?.length ?? 0) !== 1 || ($settings?.selected_audio_devices?.length ?? 0) !== 1}
                onchange={autoSave}
              />
              <span class="setting-label">Store audio and video as one file</span>
            </label>
            <span class="setting-label-with-help">
              <button
                class="help-btn"
                onclick={() => showCombineHelp = !showCombineHelp}
                onblur={() => showCombineHelp = false}
              >
                ?
              </button>
              {#if showCombineHelp}
                <div class="help-tooltip help-tooltip-right">
                  Stores audio and video as a single container file instead of separate files. <br><br>Available when exactly one audio source and one video source is selected.
                </div>
              {/if}
            </span>
          </div>
        </div>
        -->
            </section>
            <section class="settings-section">
                <h3>Application</h3>
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
                            checked={isAutostartEnabled()}
                            disabled={allUsersToggling}
                            onchange={handleAutostartToggle}
                        />
                        <span class="setting-label"
                            >Start application at system startup (<i
                                >recommended</i
                            >)</span
                        >
                    </label>
                    <label
                        class="checkbox-row checkbox-sub-option"
                        class:checkbox-row-disabled={!isAutostartEnabled()}
                    >
                        <input
                            type="checkbox"
                            bind:checked={localSettings.start_minimized}
                            disabled={!isAutostartEnabled()}
                            onchange={autoSave}
                        />
                        <span class="setting-label"
                            >Hide application window at startup</span
                        >
                    </label>
                    <p class="setting-recommendation">
                        Ensures the application will start back up if the system
                        restarts (such as for system updates). You may have to
                        log back in if your computer has a login screen.
                    </p>
                    <p class="setting-recommendation">
                        To stop the application,
                        right-click the tray icon and select Quit.
                    </p>
                    <!--
                    <button
                        class="debug-crash-btn"
                        onclick={() => invoke("simulate_crash")}
                    >
                        Simulate Crash (dev)
                    </button>
                    -->
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
                        <span class="setting-label"
                            >Desktop notification when recording starts</span
                        >
                    </label>
                </div>
                <div class="setting-row">
                    <label class="checkbox-row">
                        <input
                            type="checkbox"
                            bind:checked={localSettings.notify_recording_stop}
                            onchange={autoSave}
                        />
                        <span class="setting-label"
                            >Desktop notification when recording stops</span
                        >
                    </label>
                </div>
                <div class="setting-row">
                    <div class="sound-setting">
                        <label class="checkbox-row">
                            <input
                                type="checkbox"
                                bind:checked={
                                    localSettings.sound_recording_start
                                }
                                onchange={() => {
                                    if (!localSettings) return;
                                    if (!localSettings.sound_recording_start) {
                                        resetCustomSound("start");
                                    } else {
                                        autoSave();
                                    }
                                }}
                            />
                            <span class="setting-label"
                                >Play sound when recording starts</span
                            >
                        </label>
                        {#if localSettings.sound_recording_start}
                            <div class="sound-controls">
                                <input
                                    type="range"
                                    class="sound-volume-slider"
                                    min="0"
                                    max="1"
                                    step="0.05"
                                    bind:value={
                                        localSettings.sound_volume_start
                                    }
                                    oninput={autoSaveDebounced}
                                />
                                <span class="volume-value"
                                    >{Math.round(
                                        localSettings.sound_volume_start * 100,
                                    )}%</span
                                >
                                <button
                                    class="preview-btn"
                                    onclick={() =>
                                        localSettings &&
                                        playStartSound(
                                            localSettings.sound_volume_start,
                                            localSettings.custom_sound_start,
                                        )}
                                    title="Preview start sound"
                                    ><svg
                                        class="preview-icon"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2"
                                        ><polygon
                                            points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"
                                        /><path
                                            d="M15.54 8.46a5 5 0 0 1 0 7.07"
                                        /></svg
                                    ></button
                                >
                                <button
                                    class="customize-btn"
                                    onclick={() => browseCustomSound("start")}
                                    >Customize</button
                                >
                                {#if localSettings.custom_sound_start}
                                    <button
                                        class="custom-sound-clear"
                                        onclick={() =>
                                            resetCustomSound("start")}
                                        title="Reset to default sound"
                                        >&times;</button
                                    >
                                {/if}
                            </div>
                        {/if}
                    </div>
                </div>
                <div class="setting-row">
                    <div class="sound-setting">
                        <label class="checkbox-row">
                            <input
                                type="checkbox"
                                bind:checked={
                                    localSettings.sound_recording_stop
                                }
                                onchange={() => {
                                    if (!localSettings) return;
                                    if (!localSettings.sound_recording_stop) {
                                        resetCustomSound("stop");
                                    } else {
                                        autoSave();
                                    }
                                }}
                            />
                            <span class="setting-label"
                                >Play sound when recording stops</span
                            >
                        </label>
                        {#if localSettings.sound_recording_stop}
                            <div class="sound-controls">
                                <input
                                    type="range"
                                    class="sound-volume-slider"
                                    min="0"
                                    max="1"
                                    step="0.05"
                                    bind:value={localSettings.sound_volume_stop}
                                    oninput={autoSaveDebounced}
                                />
                                <span class="volume-value"
                                    >{Math.round(
                                        localSettings.sound_volume_stop * 100,
                                    )}%</span
                                >
                                <button
                                    class="preview-btn"
                                    onclick={() =>
                                        localSettings &&
                                        playStopSound(
                                            localSettings.sound_volume_stop,
                                            localSettings.custom_sound_stop,
                                        )}
                                    title="Preview stop sound"
                                    ><svg
                                        class="preview-icon"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2"
                                        ><polygon
                                            points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"
                                        /><path
                                            d="M15.54 8.46a5 5 0 0 1 0 7.07"
                                        /></svg
                                    ></button
                                >
                                <button
                                    class="customize-btn"
                                    onclick={() => browseCustomSound("stop")}
                                    >Customize</button
                                >
                                {#if localSettings.custom_sound_stop}
                                    <button
                                        class="custom-sound-clear"
                                        onclick={() => resetCustomSound("stop")}
                                        title="Reset to default sound"
                                        >&times;</button
                                    >
                                {/if}
                            </div>
                        {/if}
                    </div>
                </div>
                <div class="setting-row">
                    <div class="sound-setting">
                        <label class="checkbox-row">
                            <input
                                type="checkbox"
                                bind:checked={
                                    localSettings.sound_device_disconnect
                                }
                                onchange={() => {
                                    if (!localSettings) return;
                                    if (
                                        !localSettings.sound_device_disconnect
                                    ) {
                                        resetCustomSound("disconnect");
                                    } else {
                                        autoSave();
                                    }
                                }}
                            />
                            <span class="setting-label"
                                >Play warning sound if a device disconnects</span
                            >
                        </label>
                        {#if localSettings.sound_device_disconnect}
                            <div class="sound-controls">
                                <input
                                    type="range"
                                    class="sound-volume-slider"
                                    min="0"
                                    max="1"
                                    step="0.05"
                                    bind:value={
                                        localSettings.sound_volume_disconnect
                                    }
                                    oninput={autoSaveDebounced}
                                />
                                <span class="volume-value"
                                    >{Math.round(
                                        localSettings.sound_volume_disconnect *
                                            100,
                                    )}%</span
                                >
                                <button
                                    class="preview-btn"
                                    onclick={() =>
                                        localSettings &&
                                        playDisconnectWarningSound(
                                            localSettings.sound_volume_disconnect,
                                            localSettings.custom_sound_disconnect,
                                        )}
                                    title="Preview disconnect warning sound"
                                    ><svg
                                        class="preview-icon"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2"
                                        ><polygon
                                            points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"
                                        /><path
                                            d="M15.54 8.46a5 5 0 0 1 0 7.07"
                                        /></svg
                                    ></button
                                >
                                <button
                                    class="customize-btn"
                                    onclick={() =>
                                        browseCustomSound("disconnect")}
                                    >Customize</button
                                >
                                {#if localSettings.custom_sound_disconnect}
                                    <button
                                        class="custom-sound-clear"
                                        onclick={() =>
                                            resetCustomSound("disconnect")}
                                        title="Reset to default sound"
                                        >&times;</button
                                    >
                                {/if}
                            </div>
                        {/if}
                    </div>
                </div>
            </section>
        </div>
    {:else}
        <div class="loading">Loading settings...</div>
    {/if}
</div>

<About open={showAbout} onclose={() => (showAbout = false)} />

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
        position: relative;
    }

    .settings-header h2 {
        font-family: "Roboto", -apple-system, BlinkMacSystemFont, sans-serif;
        font-size: 1.125rem;
        font-weight: 500;
        color: #e8e6e3;
        letter-spacing: 0.02em;
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

    .header-right {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .section-header {
        display: flex;
        justify-content: space-between;
        align-items: baseline;
        margin-bottom: 1.25rem;
        padding-bottom: 0.75rem;
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
    }

    .settings-section .section-header > h3 {
        margin: 0;
        padding: 0;
        border: none;
    }

    .section-stats {
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.5625rem;
        color: #525252;
        letter-spacing: 0.01em;
        white-space: nowrap;
        cursor: help;
    }

    @keyframes spin {
        from {
            transform: rotate(0deg);
        }
        to {
            transform: rotate(360deg);
        }
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
        font-family: "Roboto", -apple-system, BlinkMacSystemFont, sans-serif;
        font-size: 0.6875rem;
        font-weight: 500;
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

    .format-fields {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 5.5rem;
        width: fit-content;
    }

    .format-field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .inline-checkbox {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        cursor: pointer;
        font-size: 0.8125rem;
        color: #6b6b6b;
        white-space: nowrap;
    }

    .inline-checkbox input {
        accent-color: #c9a962;
        width: 13px;
        height: 13px;
        margin: 0;
    }

    .inline-checkbox-disabled {
        opacity: 0.4;
        pointer-events: none;
    }

    :global(body.light-mode) .inline-checkbox {
        color: #888;
    }

    :global(body.light-mode) .inline-checkbox input {
        accent-color: #a08030;
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

    .checkbox-sub-option {
        margin-left: 2.25rem;
    }

    .checkbox-sub-option .setting-label {
        font-size: 0.8125rem;
        color: #7a7a7a;
    }

    :global(body.light-mode) .checkbox-sub-option .setting-label {
        color: #6a6a6a;
    }

    .checkbox-row:has(input:disabled) {
        cursor: not-allowed;
    }

    .checkbox-row:has(input:disabled) .setting-label {
        color: #5a5a5a;
    }

    :global(body.light-mode) .checkbox-row:has(input:disabled) .setting-label {
        color: #999;
    }

    .checkbox-row input {
        accent-color: #c9a962;
        width: 16px;
        height: 16px;
    }

    .sound-setting {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .sound-controls {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin-left: auto;
    }

    .sound-volume-slider {
        width: 5rem;
        height: 4px;
        accent-color: #c9a962;
        cursor: pointer;
    }

    .volume-value {
        font-size: 0.75rem;
        color: #6b6b6b;
        min-width: 2.5rem;
        text-align: right;
        font-variant-numeric: tabular-nums;
    }

    .preview-btn {
        padding: 0.25rem 0.625rem;
        background: transparent;
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
        color: #6b6b6b;
        font-family: inherit;
        font-size: 0.6875rem;
        letter-spacing: 0.03em;
        text-transform: uppercase;
        cursor: pointer;
        white-space: nowrap;
        transition: all 0.2s ease;
    }

    .preview-btn:hover:not(:disabled) {
        color: #a8a8a8;
        border-color: rgba(255, 255, 255, 0.1);
    }

    .preview-btn:disabled {
        opacity: 0.3;
        cursor: not-allowed;
    }

    .preview-icon {
        width: 11px;
        height: 11px;
        vertical-align: -1px;
    }

    .custom-sound-clear {
        background: none;
        border: none;
        color: #6b6b6b;
        font-size: 0.875rem;
        cursor: pointer;
        padding: 0 0.125rem;
        line-height: 1;
        transition: color 0.15s ease;
    }

    .custom-sound-clear:hover {
        color: #e57373;
    }

    .customize-btn {
        padding: 0.1875rem 0.5rem;
        background: transparent;
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
        color: #6b6b6b;
        font-family: inherit;
        font-size: 0.625rem;
        letter-spacing: 0.03em;
        text-transform: uppercase;
        cursor: pointer;
        white-space: nowrap;
        transition: all 0.2s ease;
    }

    .customize-btn:hover {
        color: #a8a8a8;
        border-color: rgba(255, 255, 255, 0.1);
    }

    :global(body.light-mode) .custom-sound-clear {
        color: #888;
    }

    :global(body.light-mode) .custom-sound-clear:hover {
        color: #d32f2f;
    }

    :global(body.light-mode) .customize-btn {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode) .customize-btn:hover {
        color: #3a3a3a;
        border-color: rgba(0, 0, 0, 0.2);
    }

    :global(body.light-mode) .sound-volume-slider {
        accent-color: #a08030;
    }

    :global(body.light-mode) .volume-value {
        color: #6a6a6a;
    }

    :global(body.light-mode) .preview-btn {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode) .preview-btn:hover {
        color: #3a3a3a;
        border-color: rgba(0, 0, 0, 0.2);
    }

    .loading {
        padding: 2rem;
        text-align: center;
        color: #4a4a4a;
        font-size: 0.8125rem;
    }

    .setting-label-with-help {
        display: flex;
        align-items: center;
        gap: 0.625rem;
        position: relative;
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

    /* Light mode overrides */
    :global(body.light-mode) .settings-header h2 {
        color: #2a2a2a;
    }

    :global(body.light-mode) .section-header {
        border-bottom-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .section-stats {
        color: #8a8a8a;
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

    /* About button */
    .about-btn {
        position: absolute;
        left: 50%;
        transform: translateX(-50%);
        display: flex;
        align-items: center;
        gap: 0.4rem;
        padding: 0.375rem 0.75rem;
        background: transparent;
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
        color: #6b6b6b;
        font-family: inherit;
        font-size: 0.75rem;
        font-weight: 500;
        letter-spacing: 0.03em;
        cursor: pointer;
        white-space: nowrap;
        transition: all 0.2s ease;
    }

    .about-btn svg {
        width: 14px;
        height: 14px;
    }

    .about-btn:hover {
        color: #a8a8a8;
        border-color: rgba(255, 255, 255, 0.1);
    }

    :global(body.light-mode) .about-btn {
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode) .about-btn:hover {
        color: #3a3a3a;
        border-color: rgba(0, 0, 0, 0.2);
    }

</style>
