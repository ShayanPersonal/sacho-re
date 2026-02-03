<script lang="ts">
  import type { SessionMetadata } from '$lib/api';
  import { formatDuration, formatDate, readSessionFile, checkVideoCodec } from '$lib/api';
  import { toggleSessionFavorite, updateNotes } from '$lib/stores/sessions';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import * as Tone from 'tone';
  import { Midi } from '@tonejs/midi';
  import VideoPlayer from './VideoPlayer.svelte';
  
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
  let isCheckingCodec = $state(false); // Loading state for codec check
  
  // Fallback time tracking when no video/audio is playing
  let playStartTime = 0;
  let playStartOffset = 0;
  
  // Media elements
  let videoElement: HTMLVideoElement | null = $state(null);
  let audioElement: HTMLAudioElement | null = $state(null);
  
  // MIDI synth and data
  let synth: Tone.PolySynth | null = null;
  let midiData: Midi | null = null;
  let midiNotes: Array<{time: number, note: string, duration: number, velocity: number}> = [];
  
  // Notes editing state
  let notesValue = $state(session.notes);
  let saveTimeout: ReturnType<typeof setTimeout> | null = null;
  
  // More menu state
  let moreMenuOpen = $state(false);
  
  // Sync notes when session changes
  $effect(() => {
    notesValue = session.notes;
  });
  
  // Save notes with debounce
  function handleNotesChange(e: Event) {
    const target = e.target as HTMLTextAreaElement;
    notesValue = target.value;
    
    // Debounce save
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
      updateNotes(session.id, notesValue);
    }, 500);
  }
  
  // Check if a video file needs the custom player (MJPEG in MKV)
  function needsCustomPlayer(filename: string): boolean {
    return filename.toLowerCase().endsWith('.mkv');
  }
  
  // Check the video codec and determine if it's playable
  async function checkCurrentVideoCodec() {
    if (!currentVideoFile) {
      videoUnsupportedCodec = null;
      return;
    }
    
    isCheckingCodec = true;
    try {
      const result = await checkVideoCodec(session.path, currentVideoFile.filename);
      console.log('[Video] Codec check:', result);
      
      if (!result.is_playable) {
        videoUnsupportedCodec = result.codec.toUpperCase();
        videoError = null; // Don't show generic error, use specific unsupported message
      } else {
        videoUnsupportedCodec = null;
      }
    } catch (e) {
      console.error('[Video] Failed to check codec:', e);
      // If we can't probe, try to play anyway - native player will show error if needed
      videoUnsupportedCodec = null;
    } finally {
      isCheckingCodec = false;
    }
  }
  
  // Handle video error - switch to custom player for MKV files
  function handleVideoError(e: Event) {
    const video = e.target as HTMLVideoElement;
    if (video.error && currentVideoFile) {
      console.log('[handleVideoError] Video error:', video.error.code, video.error.message);
      // Only switch to custom player for MKV files (MJPEG)
      // MP4 and WebM should work with native player
      if (needsCustomPlayer(currentVideoFile.filename)) {
        useCustomPlayer = true;
        videoError = null;
      } else {
        // For other formats, show error
        videoError = 'Unsupported video format';
      }
    }
  }
  
  // Reset video error when switching videos
  function resetVideoError() {
    videoError = null;
    // Reset custom player flag if the new video doesn't need it
    if (currentVideoFile && !needsCustomPlayer(currentVideoFile.filename)) {
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
    const separator = basePath.includes('\\') ? '\\' : '/';
    const cleanBase = basePath.endsWith(separator) ? basePath.slice(0, -1) : basePath;
    return `${cleanBase}${separator}${filename}`;
  }
  
  // Current file sources
  let videoSrc = $derived(
    session.video_files.length > 0 && videoIndex < session.video_files.length
      ? convertFileSrc(buildFilePath(session.path, session.video_files[videoIndex].filename))
      : null
  );
  
  let audioSrc = $derived(
    session.audio_files.length > 0 && audioIndex < session.audio_files.length
      ? convertFileSrc(buildFilePath(session.path, session.audio_files[audioIndex].filename))
      : null
  );
  
  // Current file info
  let currentVideoFile = $derived(session.video_files[videoIndex] ?? null);
  let currentAudioFile = $derived(session.audio_files[audioIndex] ?? null);
  let currentMidiFile = $derived(session.midi_files[midiIndex] ?? null);
  
  // Load MIDI data for current file
  async function loadMidi() {
    // Clean up previous
    if (synth) {
      synth.dispose();
      synth = null;
    }
    midiData = null;
    midiNotes = [];
    
    if (!currentMidiFile) return;
    
    try {
      console.log('[MIDI] Loading file:', currentMidiFile.filename);
      const midiBytes = await readSessionFile(session.path, currentMidiFile.filename);
      console.log('[MIDI] File size:', midiBytes.length, 'bytes');
      console.log('[MIDI] First 20 bytes:', Array.from(midiBytes.slice(0, 20)));
      
      midiData = new Midi(midiBytes);
      console.log('[MIDI] Parsed - tracks:', midiData.tracks.length, 'duration:', midiData.duration);
      
      // Create synth - use a more piano-like sound
      synth = new Tone.PolySynth(Tone.Synth, {
        oscillator: { type: 'fmsine', modulationType: 'sine', modulationIndex: 2, harmonicity: 3 },
        envelope: { attack: 0.005, decay: 0.3, sustain: 0.2, release: 1.2 }
      }).toDestination();
      synth.volume.value = -8;
      
      // Extract notes
      if (midiData.tracks.length > 0) {
        midiNotes = midiData.tracks.flatMap(track => {
          console.log('[MIDI] Track notes:', track.notes.length, 'name:', track.name);
          return track.notes.map(note => ({
            time: note.time,
            note: note.name,
            duration: note.duration,
            velocity: note.velocity
          }));
        }).sort((a, b) => a.time - b.time);
        console.log('[MIDI] Total notes extracted:', midiNotes.length);
        if (midiNotes.length > 0) {
          console.log('[MIDI] First note:', midiNotes[0]);
          console.log('[MIDI] Last note:', midiNotes[midiNotes.length - 1]);
        }
      }
    } catch (e) {
      console.error('[MIDI] Failed to load:', e);
    }
  }
  
  // Calculate max duration from all sources
  $effect(() => {
    let maxDuration = session.duration_secs;
    for (const vf of session.video_files) {
      maxDuration = Math.max(maxDuration, vf.duration_secs);
    }
    for (const af of session.audio_files) {
      maxDuration = Math.max(maxDuration, af.duration_secs);
    }
    duration = maxDuration || 60; // Default to 60s if no duration
  });
  
  // Track session ID to detect actual session changes
  let previousSessionId = '';
  
  // Reset playback state when session changes
  $effect(() => {
    const currentSessionId = session.id;
    
    // Only reset if the session actually changed (different ID)
    if (currentSessionId === previousSessionId) {
      return;
    }
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
    useCustomPlayer = false; // Try native player first for new session
    playStartTime = 0;
    playStartOffset = 0;
    
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
  $effect(() => {
    if (currentMidiFile) {
      loadMidi();
    }
  });
  
  // Sync playback time from video, audio, or fallback timer
  function updateTime() {
    if (isPlaying) {
      if (videoElement && !videoElement.paused && !videoElement.error && !videoError) {
        currentTime = videoElement.currentTime;
      } else if (audioElement && !audioElement.paused && !audioElement.error) {
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
          console.log('Playing MIDI note:', note.note, 'at', note.time);
          synth.triggerAttackRelease(note.note, Math.max(0.1, note.duration), undefined, note.velocity);
        } catch (e) {
          console.error('MIDI note error:', e);
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
      console.error('Tone.js start failed:', e);
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
        console.error('Audio play failed:', e);
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
      const metadataPath = buildFilePath(session.path, 'metadata.json');
      await revealItemInDir(metadataPath);
    } catch (error) {
      console.error('Failed to open folder:', error);
    }
  }
  
  // Animation frame for time updates and MIDI playback
  let animationFrame: number;
  function tick() {
    updateTime();
    if (isPlaying) {
      playMidiNotes();
    }
    animationFrame = requestAnimationFrame(tick);
  }
  
  onMount(() => {
    loadMidi();
    animationFrame = requestAnimationFrame(tick);
  });
  
  onDestroy(() => {
    cancelAnimationFrame(animationFrame);
    synth?.dispose();
    pause();
    if (saveTimeout) clearTimeout(saveTimeout);
  });
</script>

<div class="session-detail">
  <div class="detail-header">
    <div class="header-info">
      <h2 class="session-title">
        {formatDate(session.timestamp)}
      </h2>
      <p class="session-duration">Duration: {formatDuration(session.duration_secs)}</p>
    </div>
    <button 
      class="favorite-btn" 
      class:active={session.is_favorite}
      onclick={() => toggleSessionFavorite(session.id, !session.is_favorite)}
      title={session.is_favorite ? 'Remove from favorites' : 'Add to favorites'}
    >
      {#if session.is_favorite}
        ★
      {:else}
        ☆
      {/if}
    </button>
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
            <span class="error-text">Unsupported video format</span>
            <span class="error-hint">Use an external player for this video</span>
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
              muted
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
              <span class="error-hint">Use an external player for this video</span>
            </div>
          {/if}
        {/if}
        {#if session.video_files.length > 1}
          <button class="switch-btn video-switch" onclick={nextVideo} title="Switch video source">
            {videoIndex + 1}/{session.video_files.length}
          </button>
        {/if}
      </div>
      {#if currentVideoFile}
        <p class="source-label">{currentVideoFile.device_name}{useCustomPlayer ? ' (frame player)' : ''}{videoUnsupportedCodec ? ' (unsupported)' : ''}</p>
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
            <rect x="6" y="4" width="4" height="16"/>
            <rect x="14" y="4" width="4" height="16"/>
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="currentColor">
            <polygon points="5,3 19,12 5,21"/>
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
    
    <!-- Track Controls -->
    <div class="track-controls">
      {#if session.audio_files.length > 0}
        <div class="track-control">
          <button 
            class="mute-btn" 
            class:muted={audioMuted}
            onclick={toggleAudioMute}
            title={audioMuted ? 'Unmute audio' : 'Mute audio'}
          >
            {#if audioMuted}
              <svg viewBox="0 0 24 24" fill="currentColor">
                <path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"/>
              </svg>
            {:else}
              <svg viewBox="0 0 24 24" fill="currentColor">
                <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/>
              </svg>
            {/if}
          </button>
          <span class="track-label">Audio</span>
          <span class="track-info">{currentAudioFile?.device_name ?? 'Unknown'}</span>
          {#if session.audio_files.length > 1}
            <button class="switch-btn" onclick={nextAudio} title="Switch audio source">
              {audioIndex + 1}/{session.audio_files.length}
            </button>
          {/if}
        </div>
        <!-- Hidden audio element -->
        {#key audioSrc}
          <audio 
            bind:this={audioElement}
            src={audioSrc}
            onended={handleEnded}
            muted={audioMuted}
            preload="metadata"
          ></audio>
        {/key}
      {/if}
      
      {#if session.midi_files.length > 0}
        <div class="track-control">
          <button 
            class="mute-btn" 
            class:muted={midiMuted}
            onclick={toggleMidiMute}
            title={midiMuted ? 'Unmute MIDI' : 'Mute MIDI'}
          >
            {#if midiMuted}
              <svg viewBox="0 0 24 24" fill="currentColor">
                <path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"/>
              </svg>
            {:else}
              <svg viewBox="0 0 24 24" fill="currentColor">
                <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/>
              </svg>
            {/if}
          </button>
          <span class="track-label midi">MIDI</span>
          <span class="track-info">{currentMidiFile?.device_name ?? 'Unknown'} ({currentMidiFile?.event_count ?? 0} events)</span>
          {#if session.midi_files.length > 1}
            <button class="switch-btn" onclick={nextMidi} title="Switch MIDI source">
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
        placeholder="Add notes..."
        value={notesValue}
        oninput={handleNotesChange}
        rows="3"
      ></textarea>
    </div>
  </div>
  
  <div class="detail-content">
    {#if session.tags.length > 0}
      <section class="detail-section">
        <h3>Tags</h3>
        <div class="tag-list">
          {#each session.tags as tag}
            <span class="tag">{tag}</span>
          {/each}
        </div>
      </section>
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
        onclick={() => moreMenuOpen = !moreMenuOpen}
        onblur={() => setTimeout(() => moreMenuOpen = false, 150)}
      >
        <span>⋯</span> More options...
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
  
  .session-title {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 1.25rem;
    font-weight: 600;
    color: #fff;
    margin-bottom: 0.25rem;
  }
  
  .favorite-btn {
    width: 36px;
    height: 36px;
    border-radius: 0.25rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: #5a5a5a;
    font-size: 1.25rem;
    line-height: 1;
    cursor: pointer;
    transition: all 0.15s ease;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0;
  }
  
  .favorite-btn:hover {
    background: rgba(234, 179, 8, 0.1);
    border-color: rgba(234, 179, 8, 0.2);
    color: #eab308;
  }
  
  .favorite-btn.active {
    background: rgba(234, 179, 8, 0.15);
    border-color: rgba(234, 179, 8, 0.3);
    color: #eab308;
  }
  
  .session-duration {
    font-size: 0.875rem;
    color: #6b6b6b;
  }
  
  /* Player Section */
  .player-section {
    background:rgb(15, 15, 15);
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
    0%, 100% { opacity: 0.5; }
    50% { opacity: 1; }
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
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
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
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.25rem;
    color: #e4e4e7;
    font-family: inherit;
    font-size: 0.875rem;
    line-height: 1.5;
    resize: vertical;
    min-height: 60px;
  }
  
  .notes-input::placeholder {
    color: #5a5a5a;
  }
  
  .notes-input:focus {
    outline: none;
    border-color: rgba(239, 68, 68, 0.4);
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
  
  .switch-btn {
    padding: 0.25rem 0.5rem;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 0.25rem;
    color: #8a8a8a;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
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
  
  .detail-section h3 {
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #6b6b6b;
    margin-bottom: 0.5rem;
  }
  
  .tag-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  
  .tag {
    padding: 0.25rem 0.625rem;
    background: rgba(201, 169, 98, 0.12);
    border: 1px solid rgba(201, 169, 98, 0.25);
    border-radius: 1rem;
    font-size: 0.75rem;
    color: #c9a962;
  }
  
  .notes {
    padding: 0.75rem;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 0.25rem;
    font-size: 0.875rem;
    color: #8a8a8a;
    line-height: 1.5;
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

  :global(body.light-mode) .session-title {
    color: #2a2a2a;
  }

  :global(body.light-mode) .session-duration {
    color: #5a5a5a;
  }

  :global(body.light-mode) .favorite-btn {
    background: rgba(0, 0, 0, 0.04);
    border-color: rgba(0, 0, 0, 0.1);
    color: #8a8a8a;
  }

  :global(body.light-mode) .favorite-btn:hover {
    background: rgba(180, 140, 40, 0.12);
    border-color: rgba(180, 140, 40, 0.25);
    color: #a08030;
  }

  :global(body.light-mode) .favorite-btn.active {
    background: rgba(180, 140, 40, 0.15);
    border-color: rgba(180, 140, 40, 0.3);
    color: #a08030;
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
    color: #7a7a7a;
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
    background: rgba(255, 255, 255, 0.8);
    border-color: rgba(0, 0, 0, 0.12);
    color: #2a2a2a;
  }

  :global(body.light-mode) .notes-input::placeholder {
    color: #8a8a8a;
  }

  :global(body.light-mode) .notes-input:focus {
    border-color: rgba(200, 60, 60, 0.4);
  }

  :global(body.light-mode) .detail-section h3 {
    color: #5a5a5a;
  }

  :global(body.light-mode) .tag {
    background: rgba(160, 128, 48, 0.12);
    border-color: rgba(160, 128, 48, 0.3);
    color: #8a6a20;
  }

  :global(body.light-mode) .notes {
    background: rgba(0, 0, 0, 0.03);
    color: #5a5a5a;
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
</style>
