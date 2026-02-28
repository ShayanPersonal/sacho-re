<script lang="ts">
    import type { SessionMetadata, SessionSimilarityResult } from "$lib/api";
    import {
        formatDuration,
        formatDate,
        readSessionFile,
        checkVideoCodec,
        repairSession,
        getSessionSimilarPreview,
    } from "$lib/api";
    import {
        updateNotes,
        selectedSession,
        selectSession,
        renameCurrentSession,
    } from "$lib/stores/sessions";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { convertFileSrc } from "@tauri-apps/api/core";
    import { onMount, onDestroy } from "svelte";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import * as Tone from "tone";
    import { Midi } from "@tonejs/midi";
    import VideoPlayer from "./VideoPlayer.svelte";

    interface Props {
        session: SessionMetadata;
        onDelete: () => void;
    }

    let { session, onDelete }: Props = $props();

    // Current file indices for each modality
    let videoIndex = $state(0);
    let audioIndex = $state(0);
    let midiIndex = $state(0);

    // Playback state
    let isPlaying = $state(false);
    let currentTime = $state(0);
    let duration = $state(0);
    let audioMuted = $state(false);
    let midiMuted = $state(true); // Muted by default
    let videoError = $state<string | null>(null);
    let useCustomPlayer = $state(false); // Switch to custom JPEG frame player on error
    let videoUnsupportedCodec = $state<string | null>(null); // Detected unsupported codec
    let detectedCodec = $state<string | null>(null); // Actual codec detected by probing
    let isCheckingCodec = $state(false); // Loading state for codec check

    // Fallback time tracking when no video/audio is playing
    let playStartTime = 0;
    let playStartOffset = 0;

    // Media elements
    let videoElement: HTMLVideoElement | null = $state(null);
    let audioElement: HTMLAudioElement | null = $state(null);

    // MIDI synth and data
    let synth: Tone.PolySynth | null = null;
    let midiData: Midi | null = $state(null);
    let midiNotes: Array<{
        time: number;
        note: string;
        duration: number;
        velocity: number;
    }> = [];

    // Per-track volume in dB (-12 to +12, default 0)
    // svelte-ignore state_referenced_locally
    let audioVolumes = $state<number[]>(
        new Array(session.audio_files.length).fill(0),
    );

    // Web Audio API routing for volume boost/cut + metering
    let audioContext: AudioContext | null = null;
    let audioGainNode: GainNode | null = null;
    let audioAnalyser: AnalyserNode | null = null;
    let analyserBuffer: Float32Array | null = null;
    let connectedAudioElement: HTMLAudioElement | null = null;
    let audioMeterLevel = $state(0); // post-gain RMS (linear 0-1)

    function dbToGain(db: number): number {
        return Math.pow(10, db / 20);
    }

    /** Map linear RMS to 0-100% on a -60..0 dB meter scale */
    function rmsToMeterPercent(rms: number): number {
        if (rms <= 0) return 0;
        const db = 20 * Math.log10(rms);
        return Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
    }

    function connectAudioRouting(el: HTMLAudioElement) {
        if (!audioContext) {
            audioContext = new AudioContext();
            audioGainNode = audioContext.createGain();
            audioAnalyser = audioContext.createAnalyser();
            audioAnalyser.fftSize = 2048; // ~46ms at 44.1kHz
            analyserBuffer = new Float32Array(audioAnalyser.fftSize);
            audioGainNode.connect(audioAnalyser);
            audioAnalyser.connect(audioContext.destination);
        }
        try {
            const source = audioContext.createMediaElementSource(el);
            source.connect(audioGainNode!);
            connectedAudioElement = el;
            // Apply current volume
            audioGainNode!.gain.value = dbToGain(audioVolumes[audioIndex] ?? 0);
        } catch (e) {
            // Element may already be connected (shouldn't happen with {#key})
            console.warn("Audio routing error:", e);
        }
    }

    function updateAudioMeter() {
        if (!audioAnalyser || !analyserBuffer) {
            audioMeterLevel = 0;
            return;
        }
        audioAnalyser.getFloatTimeDomainData(analyserBuffer);
        let sum = 0;
        for (let i = 0; i < analyserBuffer.length; i++) {
            sum += analyserBuffer[i] * analyserBuffer[i];
        }
        audioMeterLevel = Math.sqrt(sum / analyserBuffer.length);
    }

    // Connect new audio elements when they appear (recreated by {#key})
    $effect(() => {
        if (audioElement && audioElement !== connectedAudioElement) {
            connectAudioRouting(audioElement);
        }
    });

    // Sync gain node when volume or track changes
    $effect(() => {
        if (audioGainNode) {
            const db = audioVolumes[audioIndex] ?? 0;
            audioGainNode.gain.value = dbToGain(db);
        }
    });

    function setAudioVolume(index: number, db: number) {
        // Snap to 0 when within ±0.5 dB
        audioVolumes[index] = Math.abs(db) < 0.5 ? 0 : db;
    }

    // Title editing state
    // Non-standard folders (no valid timestamp prefix) are not renamable
    const TIMESTAMP_RE = /^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}/;
    let isRenamable = $derived(TIMESTAMP_RE.test(session.id));
    // svelte-ignore state_referenced_locally
    let titleValue = $state(session.title ?? "");
    let isRenaming = $state(false);
    let titleMeasure: HTMLSpanElement | null = $state(null);
    let titleWidth = $state("4ch");

    // Notes editing state
    // svelte-ignore state_referenced_locally
    let notesValue = $state(session.notes);
    let saveTimeout: ReturnType<typeof setTimeout> | null = null;

    // Pending save context — stores session ID + value at time of edit
    // so flushes always target the correct session even after switching
    let pendingTitleSave: { sessionId: string; value: string } | null = null;
    let pendingNotesSave: { sessionId: string; value: string } | null = null;

    function flushPendingTitle() {
        if (pendingTitleSave) {
            const { sessionId, value } = pendingTitleSave;
            pendingTitleSave = null;
            const trimmed = value.trim();
            if (trimmed) {
                renameCurrentSession(sessionId, trimmed).catch(console.error);
            }
        }
    }

    function flushPendingNotes() {
        if (saveTimeout && pendingNotesSave) {
            clearTimeout(saveTimeout);
            saveTimeout = null;
            const { sessionId, value } = pendingNotesSave;
            pendingNotesSave = null;
            updateNotes(sessionId, value);
        }
    }

    // More menu state
    let moreMenuOpen = $state(false);

    // Session repair state (interrupted recordings with corrupt MIDI headers)
    let isRepairing = $state(false);
    let hasInterruptedMidi = $derived(
        session.midi_files.some((f) => f.needs_repair),
    );

    let lockIsStale = $derived.by(() => {
        if (!session.recording_lock_updated_at) return false;
        const lastHeartbeat = new Date(session.recording_lock_updated_at).getTime();
        return Date.now() - lastHeartbeat > 60 * 60 * 1000;
    });

    let canRepair = $derived(
        !session.recording_in_progress ||
        session.recording_lock_is_local ||
        lockIsStale
    );

    async function handleRepairSession() {
        isRepairing = true;
        try {
            const repaired = await repairSession(session.id);
            selectedSession.set(repaired);
        } catch (e) {
            console.error("Failed to repair session:", e);
        } finally {
            isRepairing = false;
        }
    }

    // Similar recordings preview
    let similarRecordings = $state<SessionSimilarityResult[]>([]);
    let hasMidi = $derived(session.midi_files.length > 0);

    $effect(() => {
        if (hasMidi) {
            getSessionSimilarPreview(session.id).then(results => {
                similarRecordings = results;
            }).catch(() => {
                similarRecordings = [];
            });
        } else {
            similarRecordings = [];
        }
    });

    let featuresUnlisten: UnlistenFn | undefined;

    function formatTimestamp(ts: string): string {
        try {
            const d = new Date(ts);
            return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
                + ' ' + d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
        } catch {
            return ts;
        }
    }

    // Sync notes and title when session changes
    $effect(() => {
        notesValue = session.notes;
        titleValue = session.title ?? "";
    });

    // Measure title width from hidden span
    function measureTitleWidth() {
        if (!titleMeasure) return;
        titleWidth = titleMeasure.offsetWidth + 20 + "px";
    }

    $effect(() => {
        titleValue;
        titleMeasure;
        requestAnimationFrame(measureTitleWidth);
    });

    // Save title (shared logic for blur/enter and session-switch flush)
    async function doTitleSave() {
        const trimmed = titleValue.trim();
        if (trimmed === (session.title ?? "")) return;
        isRenaming = true;
        try {
            await renameCurrentSession(session.id, trimmed);
        } catch (e) {
            console.error("Failed to rename session:", e);
            titleValue = session.title ?? "";
        } finally {
            isRenaming = false;
        }
    }

    function handleTitleSave() {
        pendingTitleSave = null;
        doTitleSave();
    }

    function handleTitleInput() {
        // No debounce — just mark dirty so flush-on-blur / session-switch saves it
        pendingTitleSave = { sessionId: session.id, value: titleValue };
    }

    function handleTitleKeydown(e: KeyboardEvent) {
        if (e.key === "Enter") {
            e.preventDefault();
            (e.target as HTMLInputElement).blur();
        }
    }

    // Save notes with debounce
    function handleNotesChange(e: Event) {
        const target = e.target as HTMLTextAreaElement;
        notesValue = target.value;

        // Debounce save
        if (saveTimeout) clearTimeout(saveTimeout);
        pendingNotesSave = { sessionId: session.id, value: notesValue };
        saveTimeout = setTimeout(() => {
            pendingNotesSave = null;
            updateNotes(session.id, notesValue);
        }, 500);
    }

    function handleNotesBlur() {
        if (saveTimeout) {
            clearTimeout(saveTimeout);
            saveTimeout = null;
            pendingNotesSave = null;
            updateNotes(session.id, notesValue);
        }
    }

    // Check if the detected codec needs the custom frame player (MJPEG or FFV1)
    function needsCustomPlayer(): boolean {
        return detectedCodec === "mjpeg" || detectedCodec === "ffv1";
    }

    // Check the video codec and determine if it's playable
    async function checkCurrentVideoCodec() {
        if (!currentVideoFile) {
            videoUnsupportedCodec = null;
            detectedCodec = null;
            return;
        }

        isCheckingCodec = true;
        try {
            const result = await checkVideoCodec(
                session.path,
                currentVideoFile.filename,
            );
            console.log("[Video] Codec check:", result);
            detectedCodec = result.codec.toLowerCase();

            if (!result.is_playable) {
                videoUnsupportedCodec = result.codec.toUpperCase();
                videoError = null; // Don't show generic error, use specific unsupported message
            } else {
                videoUnsupportedCodec = null;
            }
        } catch (e) {
            console.error("[Video] Failed to check codec:", e);
            // If we can't probe, try to play anyway - native player will show error if needed
            detectedCodec = null;
            videoUnsupportedCodec = null;
        } finally {
            isCheckingCodec = false;
        }
    }

    // Handle video error - switch to custom player only for MJPEG codec
    function handleVideoError(e: Event) {
        const video = e.target as HTMLVideoElement;
        if (video.error && currentVideoFile) {
            console.log(
                "[handleVideoError] Video error:",
                video.error.code,
                video.error.message,
                "codec:",
                detectedCodec,
            );
            // Only switch to custom frame player if the actual codec is MJPEG
            // VP8/VP9/AV1 in MKV containers should NOT use the MJPEG frame player
            if (needsCustomPlayer()) {
                useCustomPlayer = true;
                videoError = null;
            } else {
                // For VP8/VP9/AV1 that fail in native player, show error
                videoError = "Video playback failed — try an external player";
            }
        }
    }

    // Reset video error when switching videos
    function resetVideoError() {
        videoError = null;
        // Reset custom player flag if the new video doesn't need it
        if (currentVideoFile && !needsCustomPlayer()) {
            useCustomPlayer = false;
        }
    }

    // Check codec when video file changes
    $effect(() => {
        if (currentVideoFile) {
            checkCurrentVideoCodec();
        } else {
            videoUnsupportedCodec = null;
        }
    });

    // Helper to build file path with correct separator
    function buildFilePath(basePath: string, filename: string): string {
        // Normalize path separators for Windows
        const separator = basePath.includes("\\") ? "\\" : "/";
        const cleanBase = basePath.endsWith(separator)
            ? basePath.slice(0, -1)
            : basePath;
        return `${cleanBase}${separator}${filename}`;
    }

    // Current file sources
    let videoSrc = $derived(
        session.video_files.length > 0 &&
            videoIndex < session.video_files.length
            ? convertFileSrc(
                  buildFilePath(
                      session.path,
                      session.video_files[videoIndex].filename,
                  ),
              )
            : null,
    );

    let audioSrc = $derived(
        session.audio_files.length > 0 &&
            audioIndex < session.audio_files.length
            ? convertFileSrc(
                  buildFilePath(
                      session.path,
                      session.audio_files[audioIndex].filename,
                  ),
              )
            : null,
    );

    // Current file info
    let currentVideoFile = $derived(session.video_files[videoIndex] ?? null);
    let currentAudioFile = $derived(session.audio_files[audioIndex] ?? null);
    let currentMidiFile = $derived(session.midi_files[midiIndex] ?? null);

    // Calculate max duration from all sources (including MIDI)
    $effect(() => {
        let maxDuration = session.duration_secs;
        for (const vf of session.video_files) {
            maxDuration = Math.max(maxDuration, vf.duration_secs);
        }
        for (const af of session.audio_files) {
            maxDuration = Math.max(maxDuration, af.duration_secs);
        }
        if (midiData) {
            maxDuration = Math.max(maxDuration, midiData.duration);
        }
        duration = maxDuration || 60; // Default to 60s if no duration
    });

    // Track session ID to detect actual session changes
    let previousSessionId = "";

    // Reset playback state when session changes
    $effect(() => {
        const currentSessionId = session.id;

        // Only reset if the session actually changed (different ID)
        if (currentSessionId === previousSessionId) {
            return;
        }

        // Flush any pending saves for the old session before switching
        flushPendingTitle();
        flushPendingNotes();

        previousSessionId = currentSessionId;

        // Reset all playback state
        pause();
        isPlaying = false;
        currentTime = 0;
        videoIndex = 0;
        audioIndex = 0;
        midiIndex = 0;
        lastMidiTime = 0;
        videoError = null;
        detectedCodec = null;
        useCustomPlayer = false; // Try native player first for new session
        playStartTime = 0;
        playStartOffset = 0;

        // Reset per-track volumes
        audioVolumes = new Array(session.audio_files.length).fill(0);
        // Disconnect previous audio routing (new elements will reconnect via $effect)
        connectedAudioElement = null;

        // Clean up MIDI state from previous session
        if (synth) {
            synth.dispose();
            synth = null;
        }
        midiData = null;
        midiNotes = [];

        // Reset media elements to beginning
        if (videoElement) videoElement.currentTime = 0;
        if (audioElement) audioElement.currentTime = 0;
    });

    // Load MIDI when current MIDI file changes (handles index changes within a session)
    // Effect cleanup cancels stale async loads when session/file changes mid-flight
    $effect(() => {
        const midiFile = currentMidiFile;
        const sessionPath = session.path;

        // Clean up previous
        if (synth) {
            synth.dispose();
            synth = null;
        }
        midiData = null;
        midiNotes = [];

        if (!midiFile) return;

        let cancelled = false;

        (async () => {
            try {
                console.log("[MIDI] Loading file:", midiFile.filename);
                const midiBytes = await readSessionFile(
                    sessionPath,
                    midiFile.filename,
                );
                if (cancelled) return;

                console.log("[MIDI] File size:", midiBytes.length, "bytes");
                console.log(
                    "[MIDI] First 20 bytes:",
                    Array.from(midiBytes.slice(0, 20)),
                );

                midiData = new Midi(midiBytes);
                console.log(
                    "[MIDI] Parsed - tracks:",
                    midiData.tracks.length,
                    "duration:",
                    midiData.duration,
                );

                // Create synth - use a more piano-like sound
                synth = new Tone.PolySynth(Tone.Synth, {
                    oscillator: {
                        type: "fmsine",
                        modulationType: "sine",
                        modulationIndex: 2,
                        harmonicity: 3,
                    },
                    envelope: {
                        attack: 0.005,
                        decay: 0.3,
                        sustain: 0.2,
                        release: 1.2,
                    },
                }).toDestination();
                synth.volume.value = -8;

                // Extract notes
                if (midiData.tracks.length > 0) {
                    midiNotes = midiData.tracks
                        .flatMap((track) => {
                            console.log(
                                "[MIDI] Track notes:",
                                track.notes.length,
                                "name:",
                                track.name,
                            );
                            return track.notes.map((note) => ({
                                time: note.time,
                                note: note.name,
                                duration: note.duration,
                                velocity: note.velocity,
                            }));
                        })
                        .sort((a, b) => a.time - b.time);
                    console.log(
                        "[MIDI] Total notes extracted:",
                        midiNotes.length,
                    );
                    if (midiNotes.length > 0) {
                        console.log("[MIDI] First note:", midiNotes[0]);
                        console.log(
                            "[MIDI] Last note:",
                            midiNotes[midiNotes.length - 1],
                        );
                    }
                }
            } catch (e) {
                if (cancelled) return;
                console.error("[MIDI] Failed to load:", e);
            }
        })();

        return () => {
            cancelled = true;
        };
    });

    // Sync playback time from video, audio, or fallback timer
    function updateTime() {
        if (isPlaying) {
            if (
                videoElement &&
                !videoElement.paused &&
                !videoElement.error &&
                !videoError
            ) {
                currentTime = videoElement.currentTime;
            } else if (
                audioElement &&
                !audioElement.paused &&
                !audioElement.error
            ) {
                currentTime = audioElement.currentTime;
            } else {
                // Fallback: calculate time from when play started
                const elapsed = (performance.now() - playStartTime) / 1000;
                currentTime = playStartOffset + elapsed;

                // Stop at end of duration
                if (currentTime >= duration) {
                    currentTime = duration;
                    handleEnded();
                }
            }
        }
    }

    // Play MIDI notes at current time
    let lastMidiTime = 0;
    function playMidiNotes() {
        if (midiMuted || !synth || midiNotes.length === 0) return;

        const now = currentTime;
        // Find notes that should play between lastMidiTime and now
        for (const note of midiNotes) {
            if (note.time > lastMidiTime && note.time <= now) {
                try {
                    console.log(
                        "Playing MIDI note:",
                        note.note,
                        "at",
                        note.time,
                    );
                    synth.triggerAttackRelease(
                        note.note,
                        Math.max(0.1, note.duration),
                        undefined,
                        note.velocity,
                    );
                } catch (e) {
                    console.error("MIDI note error:", e);
                }
            }
        }
        lastMidiTime = now;
    }

    // Play/Pause all media
    async function togglePlay() {
        if (isPlaying) {
            pause();
        } else {
            await play();
        }
    }

    async function play() {
        // If we're at the end, reset to the beginning
        if (duration > 0 && currentTime >= duration - 0.1) {
            currentTime = 0;
            lastMidiTime = 0;
        }

        // Start Tone.js context if needed
        try {
            await Tone.start();
        } catch (e) {
            console.error("Tone.js start failed:", e);
        }

        // Resume Web Audio context (created outside user gesture, starts suspended)
        if (audioContext && audioContext.state === "suspended") {
            await audioContext.resume();
        }

        lastMidiTime = currentTime;

        // Set up fallback time tracking
        playStartTime = performance.now();
        playStartOffset = currentTime;

        // Play video (skip if there's an error)
        if (videoElement && videoSrc && !videoElement.error && !videoError) {
            try {
                videoElement.currentTime = currentTime;
                await videoElement.play();
            } catch (e) {
                // Video failed, but continue with audio/MIDI
            }
        }

        // Play audio
        if (audioElement && audioSrc && !audioElement.error) {
            try {
                audioElement.currentTime = currentTime;
                await audioElement.play();
            } catch (e) {
                console.error("Audio play failed:", e);
            }
        }

        isPlaying = true;
    }

    function pause() {
        videoElement?.pause();
        audioElement?.pause();
        isPlaying = false;
    }

    // Seek
    function seek(e: Event) {
        const input = e.target as HTMLInputElement;
        const time = parseFloat(input.value);
        currentTime = time;
        lastMidiTime = time;

        // Update fallback timer offset so MIDI-only playback tracks the new position
        playStartOffset = time;
        playStartTime = performance.now();

        if (videoElement) videoElement.currentTime = time;
        if (audioElement) audioElement.currentTime = time;
    }

    // Handle media ended
    function handleEnded() {
        isPlaying = false;
    }

    // Toggle mutes
    function toggleAudioMute() {
        audioMuted = !audioMuted;
        if (audioElement) audioElement.muted = audioMuted;
        if (videoElement) videoElement.muted = audioMuted;
    }

    function toggleMidiMute() {
        midiMuted = !midiMuted;
    }

    // Switch to next/previous file
    function nextVideo() {
        if (session.video_files.length <= 1) return;
        const wasPlaying = isPlaying;
        pause();
        videoError = null; // Reset error when switching

        // Always try native player first when switching videos
        // Error handler will switch to custom player if needed (for MKV/MJPEG)
        useCustomPlayer = false;

        videoIndex = (videoIndex + 1) % session.video_files.length;

        if (wasPlaying) {
            // Wait for video to load then play
            setTimeout(() => play(), 100);
        }
    }

    function nextAudio() {
        if (session.audio_files.length <= 1) return;
        const wasPlaying = isPlaying;
        pause();
        audioIndex = (audioIndex + 1) % session.audio_files.length;
        if (wasPlaying) {
            setTimeout(() => play(), 100);
        }
    }

    function nextMidi() {
        if (session.midi_files.length <= 1) return;
        midiIndex = (midiIndex + 1) % session.midi_files.length;
    }

    async function openFolder() {
        try {
            const firstFile =
                session.video_files[0]?.filename ??
                session.audio_files[0]?.filename ??
                session.midi_files[0]?.filename;
            if (firstFile) {
                await revealItemInDir(buildFilePath(session.path, firstFile));
            }
        } catch (error) {
            console.error("Failed to open folder:", error);
        }
    }

    // Animation frame for time updates and MIDI playback
    let animationFrame: number;
    function tick() {
        updateTime();
        if (isPlaying) {
            playMidiNotes();
            updateAudioMeter();
        } else if (audioMeterLevel > 0) {
            audioMeterLevel = 0;
        }
        animationFrame = requestAnimationFrame(tick);
    }

    onMount(async () => {
        animationFrame = requestAnimationFrame(tick);
        featuresUnlisten = await listen('session-features-computed', (event) => {
            if (event.payload === session.id && hasMidi) {
                getSessionSimilarPreview(session.id).then(results => {
                    similarRecordings = results;
                }).catch(() => {});
            }
        });
    });

    onDestroy(() => {
        cancelAnimationFrame(animationFrame);
        featuresUnlisten?.();
        synth?.dispose();
        pause();
        flushPendingTitle();
        flushPendingNotes();
        if (audioContext) {
            audioContext.close();
            audioContext = null;
            audioGainNode = null;
            audioAnalyser = null;
            analyserBuffer = null;
            connectedAudioElement = null;
        }
    });
</script>

<div class="session-detail">
    <div class="detail-header">
        <div class="header-info">
            {#if isRenamable}
                <span class="title-measure" bind:this={titleMeasure}
                    >{titleValue || "Title..."}</span
                >
                <input
                    class="title-input"
                    type="text"
                    placeholder="Title..."
                    maxlength="60"
                    style="width: {titleWidth}"
                    bind:value={titleValue}
                    onblur={handleTitleSave}
                    oninput={handleTitleInput}
                    onkeydown={handleTitleKeydown}
                    disabled={isRenaming}
                />
            {:else}
                <span class="title-readonly">{session.title ?? session.id}</span
                >
            {/if}
            <p class="session-date">
                {formatDate(session.timestamp)} &middot; {formatDuration(
                    session.duration_secs,
                )}
            </p>
        </div>
    </div>

    <div class="detail-scrollable">
        <div class="player-section">
            <!-- Video Player -->
            {#if session.video_files.length > 0}
                <div class="video-container">
                    {#if isCheckingCodec}
                        <!-- Loading state while checking codec -->
                        <div class="video-loading-overlay">
                            <span class="loading-text">Checking video...</span>
                        </div>
                    {:else if videoUnsupportedCodec}
                        <!-- Unsupported codec - block playback -->
                        <div class="video-unsupported-overlay">
                            <span class="error-icon">⚠</span>
                            <span class="error-text"
                                >Unsupported video format</span
                            >
                            <span class="error-hint">Use external player</span>
                        </div>
                    {:else if useCustomPlayer && currentVideoFile}
                        <!-- Custom JPEG frame player for MJPEG -->
                        <VideoPlayer
                            sessionPath={session.path}
                            filename={currentVideoFile.filename}
                            {currentTime}
                            {isPlaying}
                        />
                    {:else}
                        {#key videoSrc}
                            <video
                                bind:this={videoElement}
                                src={videoSrc}
                                onended={handleEnded}
                                onerror={handleVideoError}
                                onloadeddata={resetVideoError}
                                muted={audioMuted}
                                playsinline
                                preload="metadata"
                            >
                                <track kind="captions" />
                            </video>
                        {/key}
                        {#if videoError}
                            <div class="video-error-overlay">
                                <span class="error-icon">⚠</span>
                                <span class="error-text">{videoError}</span>
                                <span class="error-hint"
                                    >Use an external player for this video</span
                                >
                            </div>
                        {/if}
                    {/if}
                    {#if session.video_files.length > 1}
                        <button
                            class="switch-btn video-switch"
                            onclick={nextVideo}
                            title="Switch video source"
                        >
                            {videoIndex + 1}/{session.video_files.length}
                        </button>
                    {/if}
                </div>
                {#if currentVideoFile}
                    <p class="source-label">
                        {useCustomPlayer ? " (frame player)" : ""}
                    </p>
                {/if}
            {:else}
                <div class="no-video">
                    <span>No video</span>
                </div>
            {/if}

            <!-- Unified Controls -->
            <div class="player-controls">
                <button class="play-btn" onclick={togglePlay}>
                    {#if isPlaying}
                        <svg viewBox="0 0 24 24" fill="currentColor">
                            <rect x="6" y="4" width="4" height="16" />
                            <rect x="14" y="4" width="4" height="16" />
                        </svg>
                    {:else}
                        <svg viewBox="0 0 24 24" fill="currentColor">
                            <polygon points="5,3 19,12 5,21" />
                        </svg>
                    {/if}
                </button>

                <div class="time-display">
                    {formatDuration(Math.floor(currentTime))}
                </div>

                <input
                    type="range"
                    class="seek-bar"
                    min="0"
                    max={duration}
                    step="0.1"
                    value={currentTime}
                    oninput={seek}
                />

                <div class="time-display">
                    {formatDuration(Math.floor(duration))}
                </div>
            </div>

            <!-- Interrupted Recording Banner -->
            {#if hasInterruptedMidi}
                <div class="interrupted-banner">
                    <svg
                        class="interrupted-icon"
                        viewBox="0 0 24 24"
                        fill="currentColor"
                    >
                        <path
                            d="M1 21h22L12 2 1 21zm12-3h-2v-2h2v2zm0-4h-2v-4h2v4z"
                        />
                    </svg>
                    {#if session.recording_in_progress && !session.recording_lock_is_local && !lockIsStale}
                        <span class="interrupted-text"
                            >A recording may still be in progress on another device.
                            Do not repair until the recording is complete.</span
                        >
                    {:else}
                        {#if session.recording_in_progress && session.recording_lock_is_local}
                            <span class="interrupted-text"
                                >This recording was interrupted. Click Repair to recover files.</span
                            >
                        {:else if session.recording_in_progress && lockIsStale}
                            <span class="interrupted-text"
                                >A recording on another device appears to have been interrupted.</span
                            >
                        {:else}
                            <span class="interrupted-text"
                                >This recording may have been interrupted. Click Repair to recover files.</span
                            >
                        {/if}
                        <button
                            class="repair-btn"
                            onclick={handleRepairSession}
                            disabled={isRepairing}
                        >
                            {isRepairing ? "Repairing..." : "Repair"}
                        </button>
                    {/if}
                </div>
            {/if}

            <!-- Track Controls -->
            <div class="track-controls">
                {#if session.audio_files.length > 0 || session.video_files.length > 0}
                    <div class="track-control">
                        <button
                            class="mute-btn"
                            class:muted={audioMuted}
                            onclick={toggleAudioMute}
                            title={audioMuted ? "Unmute audio" : "Mute audio"}
                        >
                            {#if audioMuted}
                                <svg viewBox="0 0 24 24" fill="currentColor">
                                    <path
                                        d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"
                                    />
                                </svg>
                            {:else}
                                <svg viewBox="0 0 24 24" fill="currentColor">
                                    <path
                                        d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"
                                    />
                                </svg>
                            {/if}
                        </button>
                        <span class="track-label">Audio</span>
                        <span class="track-info">
                            {currentAudioFile?.device_name ??
                                currentVideoFile?.device_name ??
                                "Unknown"}
                        </span>
                        {#if session.audio_files.length > 0}
                            <div class="volume-control">
                                <div class="volume-slider-wrapper">
                                    <div class="meter-track">
                                        <div
                                            class="meter-fill"
                                            style="width: {rmsToMeterPercent(
                                                audioMeterLevel,
                                            )}%"
                                        ></div>
                                    </div>
                                    <input
                                        type="range"
                                        class="volume-slider"
                                        min="-12"
                                        max="12"
                                        step="0.1"
                                        value={audioVolumes[audioIndex] ?? 0}
                                        oninput={(e) =>
                                            setAudioVolume(
                                                audioIndex,
                                                parseFloat(
                                                    (
                                                        e.target as HTMLInputElement
                                                    ).value,
                                                ),
                                            )}
                                        ondblclick={() =>
                                            setAudioVolume(audioIndex, 0)}
                                        title="Track volume (double-click to reset)"
                                    />
                                </div>
                                <span class="volume-label"
                                    >{(audioVolumes[audioIndex] ?? 0) > 0
                                        ? "+"
                                        : ""}{(
                                        audioVolumes[audioIndex] ?? 0
                                    ).toFixed(1)} dB</span
                                >
                            </div>
                        {/if}
                        {#if session.audio_files.length > 1}
                            <button
                                class="switch-btn"
                                onclick={nextAudio}
                                title="Switch audio source"
                            >
                                {audioIndex + 1}/{session.audio_files.length}
                            </button>
                        {/if}
                    </div>
                    <!-- Hidden audio element (only needed for separate audio files) -->
                    {#if session.audio_files.length > 0}
                        {#key audioSrc}
                            <audio
                                bind:this={audioElement}
                                src={audioSrc}
                                onended={handleEnded}
                                muted={audioMuted}
                                crossorigin="anonymous"
                                preload="metadata"
                            ></audio>
                        {/key}
                    {/if}
                {/if}

                {#if session.midi_files.length > 0}
                    <div class="track-control">
                        <button
                            class="mute-btn"
                            class:muted={midiMuted}
                            onclick={toggleMidiMute}
                            title={midiMuted ? "Unmute MIDI" : "Mute MIDI"}
                        >
                            {#if midiMuted}
                                <svg viewBox="0 0 24 24" fill="currentColor">
                                    <path
                                        d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"
                                    />
                                </svg>
                            {:else}
                                <svg viewBox="0 0 24 24" fill="currentColor">
                                    <path
                                        d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"
                                    />
                                </svg>
                            {/if}
                        </button>
                        <span class="track-label midi">MIDI</span>
                        <span class="track-info"
                            >{currentMidiFile?.device_name ?? "Unknown"} ({currentMidiFile?.event_count ??
                                0} events)</span
                        >
                        {#if session.midi_files.length > 1}
                            <button
                                class="switch-btn"
                                onclick={nextMidi}
                                title="Switch MIDI source"
                            >
                                {midiIndex + 1}/{session.midi_files.length}
                            </button>
                        {/if}
                    </div>
                {/if}
            </div>

            <!-- Notes Input -->
            <div class="notes-section">
                <textarea
                    class="notes-input"
                    placeholder="Notes..."
                    value={notesValue}
                    oninput={handleNotesChange}
                    onblur={handleNotesBlur}
                    rows="3"
                ></textarea>
            </div>
        </div>

        <div class="detail-content">
            {#if hasMidi && similarRecordings.length > 0}
                <div class="similar-section">
                    <h3 class="similar-title">Similar Recordings</h3>
                    <div class="similar-list">
                        {#each similarRecordings as result (result.session_id)}
                            <button
                                class="similar-item"
                                onclick={() => selectSession(result.session_id)}
                                title={result.title || result.timestamp}
                            >
                                <span class="similar-name">
                                    {result.title || formatTimestamp(result.timestamp)}
                                </span>
                                <span class="similar-score">
                                    {Math.round(result.score * 100)}%
                                </span>
                            </button>
                        {/each}
                    </div>
                </div>
            {/if}
        </div>
    </div>

    <div class="detail-actions">
        <button class="action-btn" onclick={openFolder}>
            <span>📂</span> Open Folder
        </button>
        <div class="more-menu-container">
            <button
                class="action-btn"
                onclick={() => (moreMenuOpen = !moreMenuOpen)}
                onblur={() => setTimeout(() => (moreMenuOpen = false), 150)}
            >
                <span>⋯</span> More
            </button>
            {#if moreMenuOpen}
                <div class="more-menu">
                    <button class="more-menu-item danger" onclick={onDelete}>
                        <span>🗑</span> Delete
                    </button>
                </div>
            {/if}
        </div>
    </div>
</div>

<style>
    .session-detail {
        display: flex;
        flex-direction: column;
        height: 100%;
        padding: 1.5rem;
        min-height: 0; /* Allow flex container to shrink */
    }

    .detail-scrollable {
        flex: 1;
        overflow-y: auto;
        min-height: 0; /* Allow scrolling when content overflows */
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .detail-header {
        display: flex;
        justify-content: space-between;
        align-items: flex-start;
        padding-bottom: 1rem;
        border-bottom: 1px solid rgba(255, 255, 255, 0.06);
        flex-shrink: 0; /* Keep header fixed */
    }

    .title-measure {
        position: absolute;
        visibility: hidden;
        height: 0;
        overflow: hidden;
        white-space: pre;
        font-size: 1.25rem;
        font-weight: 400;
        font-family: inherit;
        padding: 0 0.375rem;
    }

    .title-input {
        font-size: 1.25rem;
        font-weight: 400;
        color: #fff;
        background: transparent;
        border: 1px solid transparent;
        border-radius: 0.25rem;
        padding: 0.25rem 0.375rem;
        margin: -0.25rem -0.375rem;
        font-family: inherit;
        transition: border-color 0.15s ease;
        min-width: 4ch;
        max-width: 100%;
    }

    .title-input::placeholder {
        color: #4a4a4a;
        font-weight: 400;
    }

    .title-input:hover {
        border-color: rgba(255, 255, 255, 0.08);
    }

    .title-input:focus {
        outline: none;
        border-color: rgba(201, 169, 98, 0.4);
    }

    .title-input:disabled {
        opacity: 0.5;
    }

    .title-readonly {
        display: block;
        width: 100%;
        font-size: 1.25rem;
        font-weight: 400;
        color: #fff;
        font-family: inherit;
    }

    .session-date {
        font-size: 0.8125rem;
        color: #6b6b6b;
        margin-top: 0.125rem;
    }

    /* Player Section */
    .player-section {
        background: rgb(15, 15, 15);
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 0.25rem;
        padding: 1rem;
        flex-shrink: 0;
    }

    .video-container {
        position: relative;
        width: 100%;
        max-width: 400px;
        margin: 0 auto 0.5rem;
        border-radius: 0.25rem;
        overflow: hidden;
        background: #0c0c0b;
        border: 2px solid rgba(255, 255, 255, 0.08);
    }

    .video-container video {
        width: 100%;
        display: block;
        min-height: 200px;
    }

    .video-switch {
        position: absolute;
        top: 0.5rem;
        right: 0.5rem;
    }

    .video-error-overlay {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background: rgba(10, 10, 10, 0.95);
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.5rem;
        color: #6b6b6b;
    }

    /* These overlays need their own dimensions since there's no video element behind them */
    .video-unsupported-overlay,
    .video-loading-overlay {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.5rem;
        color: #5a5a5a;
        min-height: 200px;
        width: 100%;
        aspect-ratio: 16 / 9;
    }

    .video-unsupported-overlay {
        background: #0c0c0b;
    }

    .video-loading-overlay {
        background: #0c0c0b;
    }

    .loading-text {
        font-size: 0.8125rem;
        color: #6b6b6b;
        animation: pulse 2s ease-in-out infinite;
    }

    @keyframes pulse {
        0%,
        100% {
            opacity: 0.5;
        }
        50% {
            opacity: 1;
        }
    }

    .error-icon {
        font-size: 1.5rem;
        opacity: 0.4;
    }

    .error-text {
        font-size: 0.8125rem;
        text-align: center;
        color: #6b6b6b;
    }

    .error-hint {
        font-size: 0.6875rem;
        color: #4a4a4a;
    }

    .source-label {
        text-align: center;
        font-size: 0.6875rem;
        color: #4a4a4a;
        letter-spacing: 0.02em;
        margin-bottom: 1rem;
    }

    .no-video {
        width: 100%;
        max-width: 400px;
        margin: 0 auto 1rem;
        aspect-ratio: 16/9;
        background: rgba(0, 0, 0, 0.5);
        border-radius: 0.25rem;
        display: flex;
        align-items: center;
        justify-content: center;
        color: #5a5a5a;
        font-size: 0.875rem;
    }

    /* Controls */
    .player-controls {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        margin-bottom: 1rem;
    }

    .play-btn {
        width: 40px;
        height: 40px;
        border-radius: 50%;
        background: rgba(239, 68, 68, 0.15);
        border: 1px solid rgba(239, 68, 68, 0.3);
        color: #ef4444;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }

    .play-btn:hover {
        background: rgba(239, 68, 68, 0.25);
    }

    .play-btn svg {
        width: 16px;
        height: 16px;
    }

    .time-display {
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.75rem;
        color: #6b6b6b;
        min-width: 40px;
        text-align: center;
    }

    .seek-bar {
        flex: 1;
        height: 4px;
        margin: 0 12px; /* Slightly more than half thumb width for full visual reach */
        -webkit-appearance: none;
        appearance: none;
        background: rgba(255, 255, 255, 0.1);
        border-radius: 2px;
        cursor: pointer;
    }

    .seek-bar::-webkit-slider-thumb {
        -webkit-appearance: none;
        width: 12px;
        height: 12px;
        border-radius: 50%;
        background: #ef4444;
        cursor: pointer;
    }

    .seek-bar::-moz-range-thumb {
        width: 12px;
        height: 12px;
        border-radius: 50%;
        background: #ef4444;
        cursor: pointer;
        border: none;
    }

    /* Track Controls */
    .track-controls {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    /* Notes Section */
    .notes-section {
        margin-top: 1rem;
        padding-top: 1rem;
        border-top: 1px solid rgba(255, 255, 255, 0.06);
    }

    .notes-input {
        width: 100%;
        padding: 0.75rem;
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.04);
        border-radius: 0.25rem;
        color: #e4e4e7;
        font-family: "Roboto", sans-serif;
        font-weight: 300;
        font-size: 0.875rem;
        line-height: 1.5;
        resize: none;
        min-height: 60px;
        transition:
            background 0.15s ease,
            border-color 0.15s ease;
    }

    .notes-input:hover {
        background: rgba(0, 0, 0, 0.15);
        border-color: rgba(255, 255, 255, 0.04);
    }

    .notes-input::placeholder {
        color: #4a4a4a;
    }

    .notes-input:focus {
        outline: none;
        background: rgba(0, 0, 0, 0.3);
        border-color: rgba(255, 255, 255, 0.08);
        resize: vertical;
    }

    .track-control {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem;
        background: rgba(255, 255, 255, 0.02);
        border-radius: 0.375rem;
    }

    .mute-btn {
        width: 28px;
        height: 28px;
        border-radius: 0.25rem;
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.08);
        color: #8a8a8a;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }

    .mute-btn:hover {
        background: rgba(255, 255, 255, 0.08);
        color: #e4e4e7;
    }

    .mute-btn.muted {
        color: #ef4444;
        background: rgba(239, 68, 68, 0.1);
        border-color: rgba(239, 68, 68, 0.2);
    }

    .mute-btn svg {
        width: 16px;
        height: 16px;
    }

    .track-label {
        font-size: 0.75rem;
        font-weight: 600;
        text-transform: uppercase;
        color: #7a9a6e;
        min-width: 40px;
    }

    .track-label.midi {
        color: #c9a962;
    }

    .track-info {
        flex: 1;
        font-size: 0.8125rem;
        color: #6b6b6b;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .volume-control {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-shrink: 0;
    }

    .volume-slider-wrapper {
        position: relative;
        width: 64px;
        height: 16px;
        display: flex;
        align-items: center;
    }

    .volume-slider-wrapper .meter-track {
        position: absolute;
        left: 0;
        right: 0;
        height: 6px;
        background: rgba(255, 255, 255, 0.06);
        border-radius: 3px;
        overflow: hidden;
        pointer-events: none;
    }

    .volume-slider-wrapper .meter-fill {
        height: 100%;
        background: rgba(80, 180, 80, 0.5);
        border-radius: 3px;
        transition: width 0.05s linear;
    }

    .volume-slider {
        position: relative;
        width: 100%;
        height: 16px;
        -webkit-appearance: none;
        appearance: none;
        background: transparent;
        cursor: pointer;
        z-index: 1;
    }

    .volume-slider::-webkit-slider-thumb {
        -webkit-appearance: none;
        width: 10px;
        height: 10px;
        border-radius: 50%;
        background: #8a8a8a;
        cursor: pointer;
    }

    .volume-slider::-moz-range-thumb {
        width: 10px;
        height: 10px;
        border-radius: 50%;
        background: #8a8a8a;
        cursor: pointer;
        border: none;
    }

    .volume-slider:hover::-webkit-slider-thumb {
        background: #e4e4e7;
    }

    .volume-slider:hover::-moz-range-thumb {
        background: #e4e4e7;
    }

    .volume-label {
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.625rem;
        color: #5a5a5a;
        width: 52px;
        text-align: right;
        flex-shrink: 0;
    }

    .switch-btn {
        padding: 0.25rem 0.5rem;
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: 0.25rem;
        color: #8a8a8a;
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        font-size: 0.6875rem;
        cursor: pointer;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }

    .switch-btn:hover {
        background: rgba(255, 255, 255, 0.1);
        color: #e4e4e7;
    }

    .switch-btn.video-switch {
        position: absolute;
        top: 0.5rem;
        right: 0.5rem;
        background: rgba(0, 0, 0, 0.7);
        border: 1px solid rgba(255, 255, 255, 0.2);
        color: #e8e6e3;
        font-size: 0.75rem;
        padding: 0.25rem 0.625rem;
        backdrop-filter: blur(4px);
        z-index: 5;
    }

    .switch-btn.video-switch:hover {
        background: rgba(0, 0, 0, 0.85);
        color: #fff;
    }

    /* Content */
    .detail-content {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    /* Actions */
    .detail-actions {
        display: flex;
        gap: 0.5rem;
        padding-top: 1rem;
        border-top: 1px solid rgba(255, 255, 255, 0.06);
        flex-shrink: 0; /* Prevent actions from being pushed out of view */
    }

    .action-btn {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.625rem 0.875rem;
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 0.25rem;
        color: #8a8a8a;
        font-family: inherit;
        font-size: 0.8125rem;
        cursor: pointer;
        transition: all 0.15s ease;
    }

    .action-btn:hover {
        background: rgba(255, 255, 255, 0.08);
        color: #e4e4e7;
    }

    .more-menu-container {
        position: relative;
    }

    .more-menu {
        position: absolute;
        bottom: 100%;
        right: 0;
        margin-bottom: 0.25rem;
        min-width: 160px;
        background: #1a1a1a;
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: 0.25rem;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
        z-index: 100;
        overflow: hidden;
    }

    .more-menu-item {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        width: 100%;
        padding: 0.625rem 0.75rem;
        background: transparent;
        border: none;
        color: #e4e4e7;
        font-family: inherit;
        font-size: 0.8125rem;
        cursor: pointer;
        transition: background 0.1s ease;
        text-align: left;
    }

    .more-menu-item:hover {
        background: rgba(255, 255, 255, 0.05);
    }

    .more-menu-item.danger {
        color: #ef4444;
    }

    .more-menu-item.danger:hover {
        background: rgba(239, 68, 68, 0.15);
    }

    /* Light mode overrides */
    :global(body.light-mode) .detail-header {
        border-bottom-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .title-input {
        color: #2a2a2a;
    }

    :global(body.light-mode) .title-input::placeholder {
        color: #8a8a8a;
    }

    :global(body.light-mode) .title-readonly {
        color: #2a2a2a;
    }

    :global(body.light-mode) .title-input:hover {
        border-color: rgba(0, 0, 0, 0.1);
    }

    :global(body.light-mode) .title-input:focus {
        border-color: rgba(160, 128, 48, 0.5);
    }

    :global(body.light-mode) .session-date {
        color: #5a5a5a;
    }

    :global(body.light-mode) .player-section {
        background: rgba(245, 245, 240, 1);
        border-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .video-container {
        background: #e8e8e8;
        border-color: rgba(0, 0, 0, 0.12);
    }

    :global(body.light-mode) .video-container video {
        background: #1a1a1a;
    }

    :global(body.light-mode) .video-error-overlay,
    :global(body.light-mode) .video-unsupported-overlay,
    :global(body.light-mode) .video-loading-overlay {
        background: #e8e8e8;
        color: #6a6a6a;
    }

    :global(body.light-mode) .error-icon {
        opacity: 0.5;
    }

    :global(body.light-mode) .error-text {
        color: #5a5a5a;
    }

    :global(body.light-mode) .error-hint {
        color: #7a7a7a;
    }

    :global(body.light-mode) .loading-text {
        color: #5a5a5a;
    }

    :global(body.light-mode) .source-label {
        color: #a7a7a7;
    }

    :global(body.light-mode) .no-video {
        background: rgba(0, 0, 0, 0.05);
        color: #7a7a7a;
    }

    :global(body.light-mode) .play-btn {
        background: rgba(200, 60, 60, 0.12);
        border-color: rgba(200, 60, 60, 0.3);
        color: #c04040;
    }

    :global(body.light-mode) .play-btn:hover {
        background: rgba(200, 60, 60, 0.2);
    }

    :global(body.light-mode) .time-display {
        color: #5a5a5a;
    }

    :global(body.light-mode) .seek-bar {
        background: rgba(0, 0, 0, 0.12);
    }

    :global(body.light-mode) .seek-bar::-webkit-slider-thumb {
        background: #c04040;
    }

    :global(body.light-mode) .seek-bar::-moz-range-thumb {
        background: #c04040;
    }

    :global(body.light-mode) .track-control {
        background: rgba(0, 0, 0, 0.03);
    }

    :global(body.light-mode) .mute-btn {
        background: rgba(0, 0, 0, 0.05);
        border-color: rgba(0, 0, 0, 0.1);
        color: #5a5a5a;
    }

    :global(body.light-mode) .mute-btn:hover {
        background: rgba(0, 0, 0, 0.08);
        color: #2a2a2a;
    }

    :global(body.light-mode) .mute-btn.muted {
        color: #c04040;
        background: rgba(200, 60, 60, 0.1);
        border-color: rgba(200, 60, 60, 0.2);
    }

    :global(body.light-mode) .track-label {
        color: #5a8a4a;
    }

    :global(body.light-mode) .track-label.midi {
        color: #8a6a20;
    }

    :global(body.light-mode) .track-info {
        color: #5a5a5a;
    }

    :global(body.light-mode) .volume-slider-wrapper .meter-track {
        background: rgba(0, 0, 0, 0.06);
    }

    :global(body.light-mode) .volume-slider-wrapper .meter-fill {
        background: rgba(60, 150, 60, 0.4);
    }

    :global(body.light-mode) .volume-slider {
        background: rgba(0, 0, 0, 0.1);
    }

    :global(body.light-mode) .volume-slider::-webkit-slider-thumb {
        background: #6a6a6a;
    }

    :global(body.light-mode) .volume-slider::-moz-range-thumb {
        background: #6a6a6a;
    }

    :global(body.light-mode) .volume-label {
        color: #7a7a7a;
    }

    :global(body.light-mode) .switch-btn {
        background: rgba(0, 0, 0, 0.05);
        border-color: rgba(0, 0, 0, 0.12);
        color: #5a5a5a;
    }

    :global(body.light-mode) .switch-btn:hover {
        background: rgba(0, 0, 0, 0.08);
        color: #2a2a2a;
    }

    :global(body.light-mode) .switch-btn.video-switch {
        background: rgba(255, 255, 255, 0.85);
        border-color: rgba(0, 0, 0, 0.2);
        color: #3a3a3a;
    }

    :global(body.light-mode) .switch-btn.video-switch:hover {
        background: rgba(255, 255, 255, 0.95);
        color: #1a1a1a;
    }

    :global(body.light-mode) .notes-section {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .notes-input {
        background: rgba(255, 255, 255, 0.5);
        border-color: rgba(0, 0, 0, 0.06);
        color: #2a2a2a;
    }

    :global(body.light-mode) .notes-input:hover {
        background: rgba(0, 0, 0, 0.03);
        border-color: rgba(0, 0, 0, 0.06);
    }

    :global(body.light-mode) .notes-input::placeholder {
        color: #8a8a8a;
    }

    :global(body.light-mode) .notes-input:focus {
        background: rgba(255, 255, 255, 0.8);
        border-color: rgba(0, 0, 0, 0.12);
    }

    :global(body.light-mode) .detail-actions {
        border-top-color: rgba(0, 0, 0, 0.08);
    }

    :global(body.light-mode) .action-btn {
        background: rgba(0, 0, 0, 0.04);
        border-color: rgba(0, 0, 0, 0.1);
        color: #5a5a5a;
    }

    :global(body.light-mode) .action-btn:hover {
        background: rgba(0, 0, 0, 0.08);
        color: #2a2a2a;
    }

    :global(body.light-mode) .more-menu {
        background: #ffffff;
        border-color: rgba(0, 0, 0, 0.12);
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
    }

    :global(body.light-mode) .more-menu-item {
        color: #3a3a3a;
    }

    :global(body.light-mode) .more-menu-item:hover {
        background: rgba(0, 0, 0, 0.04);
    }

    :global(body.light-mode) .more-menu-item.danger {
        color: #c04040;
    }

    :global(body.light-mode) .more-menu-item.danger:hover {
        background: rgba(200, 60, 60, 0.1);
    }

    /* Interrupted recording banner */
    .interrupted-banner {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 0.75rem;
        background: rgba(234, 179, 8, 0.1);
        border: 1px solid rgba(234, 179, 8, 0.25);
        border-radius: 0.375rem;
        margin-bottom: 0.5rem;
    }

    .interrupted-icon {
        width: 16px;
        height: 16px;
        flex-shrink: 0;
        color: #eab308;
    }

    .interrupted-text {
        font-size: 0.75rem;
        color: #eab308;
        flex: 1;
        line-height: 1.3;
    }

    .repair-btn {
        flex-shrink: 0;
        padding: 0.25rem 0.625rem;
        font-size: 0.7rem;
        font-weight: 500;
        background: rgba(234, 179, 8, 0.15);
        border: 1px solid rgba(234, 179, 8, 0.3);
        border-radius: 0.25rem;
        color: #eab308;
        cursor: pointer;
        transition: all 0.15s ease;
        white-space: nowrap;
    }

    .repair-btn:hover:not(:disabled) {
        background: rgba(234, 179, 8, 0.25);
        border-color: rgba(234, 179, 8, 0.4);
    }

    .repair-btn:disabled {
        opacity: 0.6;
        cursor: not-allowed;
    }

    :global(body.light-mode) .interrupted-banner {
        background: rgba(180, 130, 0, 0.08);
        border-color: rgba(180, 130, 0, 0.2);
    }

    :global(body.light-mode) .interrupted-text {
        color: #92700c;
    }

    :global(body.light-mode) .interrupted-icon {
        color: #92700c;
    }

    :global(body.light-mode) .repair-btn {
        background: rgba(180, 130, 0, 0.1);
        border-color: rgba(180, 130, 0, 0.25);
        color: #92700c;
    }

    :global(body.light-mode) .repair-btn:hover:not(:disabled) {
        background: rgba(180, 130, 0, 0.18);
        border-color: rgba(180, 130, 0, 0.35);
    }

    /* ---- Similar Recordings ---- */
    .similar-section {
        padding: 0.75rem 0 0;
    }

    .similar-title {
        font-size: 0.75rem;
        font-weight: 500;
        color: #6a6a6a;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        margin: 0 0 0.5rem;
    }

    .similar-list {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .similar-item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.5rem 0.625rem;
        background: rgba(255, 255, 255, 0.02);
        border: 1px solid rgba(255, 255, 255, 0.04);
        border-radius: 0.25rem;
        color: #8a8a8a;
        font-family: inherit;
        font-size: 0.8125rem;
        cursor: pointer;
        transition: all 0.15s ease;
        text-align: left;
    }

    .similar-item:hover {
        background: rgba(201, 169, 98, 0.06);
        border-color: rgba(201, 169, 98, 0.15);
        color: #b8b8b8;
    }

    .similar-name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        flex: 1;
        min-width: 0;
    }

    .similar-score {
        font-size: 0.75rem;
        color: #c9a962;
        font-family: "DM Mono", "SF Mono", Menlo, monospace;
        flex-shrink: 0;
        margin-left: 0.5rem;
    }

    :global(body.light-mode) .similar-title {
        color: #7a7a7a;
    }

    :global(body.light-mode) .similar-item {
        background: rgba(0, 0, 0, 0.02);
        border-color: rgba(0, 0, 0, 0.06);
        color: #5a5a5a;
    }

    :global(body.light-mode) .similar-item:hover {
        background: rgba(160, 128, 48, 0.08);
        border-color: rgba(160, 128, 48, 0.2);
        color: #3a3a3a;
    }

    :global(body.light-mode) .similar-score {
        color: #8a6a20;
    }
</style>
