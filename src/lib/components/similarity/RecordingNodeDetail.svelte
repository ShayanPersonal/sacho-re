<script lang="ts">
  import type { SessionSimilarityResult } from '$lib/api';
  import { formatDuration, readSessionFile, getSessionDetail } from '$lib/api';
  import { selectSession } from '$lib/stores/sessions';
  import { activeTab } from '$lib/stores/navigation';
  import { onMount, onDestroy, untrack } from 'svelte';
  import * as Tone from 'tone';
  import { Midi } from '@tonejs/midi';

  interface Props {
    recording: SessionSimilarityResult;
    isCenter: boolean;
    onClose: () => void;
  }

  let { recording, isCenter, onClose }: Props = $props();

  // MIDI playback state
  let synth: Tone.PolySynth | null = null;
  let midiData: Midi | null = $state(null);
  let midiNotes = $state<Array<{
    time: number;
    note: string;
    duration: number;
    velocity: number;
  }>>([]);

  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let lastMidiTime = 0;
  let playStartTime = 0;
  let playStartOffset = 0;
  let animationFrame: number;
  let loadError = $state<string | null>(null);

  let displayTitle = $derived(recording.title || formatTimestamp(recording.timestamp));

  function formatTimestamp(ts: string): string {
    try {
      const d = new Date(ts);
      return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
        + ' ' + d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    } catch {
      return ts;
    }
  }

  // Load first MIDI file from the session
  $effect(() => {
    const currentRec = recording;
    untrack(() => cleanup());
    loadError = null;

    let cancelled = false;

    (async () => {
      try {
        const detail = await getSessionDetail(currentRec.session_id);
        if (cancelled) return;
        if (!detail || detail.midi_files.length === 0) {
          loadError = 'No MIDI file found';
          return;
        }

        const sessionPath = detail.path;
        const midiFilename = detail.midi_files[0].filename;
        const midiBytes = await readSessionFile(sessionPath, midiFilename);

        if (cancelled) return;

        midiData = new Midi(midiBytes);

        synth = new Tone.PolySynth(Tone.Synth, {
          oscillator: {
            type: 'fmsine',
            modulationType: 'sine',
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

        if (midiData.tracks.length > 0) {
          midiNotes = midiData.tracks
            .flatMap((track) =>
              track.notes.map((note) => ({
                time: note.time,
                note: note.name,
                duration: note.duration,
                velocity: note.velocity,
              })),
            )
            .sort((a, b) => a.time - b.time);
        }

        duration = midiData.duration || 0;

        // Seek to chunk offset if available
        const chunkStart = (!isCenter && recording.match_offset_secs > 0 && recording.match_offset_secs < duration)
          ? recording.match_offset_secs : 0;
        const firstNote = midiNotes.find(n => n.time >= chunkStart);
        const seekTo = firstNote ? Math.max(0, firstNote.time - 0.3) : chunkStart;
        if (seekTo > 0) {
          currentTime = seekTo;
          lastMidiTime = seekTo;
          playStartOffset = seekTo;
        }
      } catch (e) {
        if (cancelled) return;
        console.error('[RecordingNodeDetail] Failed to load:', e);
        loadError = 'Failed to load MIDI';
      }
    })();

    return () => {
      cancelled = true;
    };
  });

  function cleanup() {
    if (isPlaying) pause();
    if (synth) {
      synth.dispose();
      synth = null;
    }
    midiData = null;
    midiNotes = [];
    currentTime = 0;
    duration = 0;
    lastMidiTime = 0;
  }

  async function togglePlay() {
    if (isPlaying) {
      pause();
    } else {
      await play();
    }
  }

  async function play() {
    if (!synth || midiNotes.length === 0) return;

    if (duration > 0 && currentTime >= duration - 0.1) {
      currentTime = 0;
      lastMidiTime = 0;
    }

    try {
      await Tone.start();
    } catch (e) {
      console.error('Tone.js start failed:', e);
    }

    lastMidiTime = currentTime;
    playStartTime = performance.now();
    playStartOffset = currentTime;
    isPlaying = true;
  }

  function pause() {
    isPlaying = false;
    synth?.releaseAll();
  }

  function seek(e: Event) {
    const input = e.target as HTMLInputElement;
    const time = parseFloat(input.value);
    currentTime = time;
    lastMidiTime = time;
    playStartOffset = time;
    playStartTime = performance.now();
  }

  function tick() {
    if (isPlaying) {
      const elapsed = (performance.now() - playStartTime) / 1000;
      currentTime = playStartOffset + elapsed;

      if (currentTime >= duration) {
        currentTime = duration;
        isPlaying = false;
      } else {
        playMidiNotes();
      }
    }
    animationFrame = requestAnimationFrame(tick);
  }

  function playMidiNotes() {
    if (!synth || midiNotes.length === 0) return;

    const now = currentTime;
    for (const note of midiNotes) {
      if (note.time > lastMidiTime && note.time <= now) {
        try {
          synth.triggerAttackRelease(
            note.note,
            Math.max(0.1, note.duration),
            undefined,
            note.velocity,
          );
        } catch (e) {
          // Skip note errors
        }
      }
    }
    lastMidiTime = now;
  }

  function goToSession() {
    const sessionId = recording.session_id;
    cleanup();
    activeTab.set('sessions');
    selectSession(sessionId);
  }

  onMount(() => {
    animationFrame = requestAnimationFrame(tick);
  });

  onDestroy(() => {
    cancelAnimationFrame(animationFrame);
    cleanup();
  });
</script>

<div class="recording-detail">
  <div class="detail-header">
    <h3 class="rec-title" title={displayTitle}>{displayTitle}</h3>
    <button class="close-btn" onclick={onClose} title="Close">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M18 6L6 18M6 6l12 12" />
      </svg>
    </button>
  </div>

  {#if !isCenter && recording.score > 0}
    <div class="score-section">
      <span class="score-value">{Math.round(recording.score * 100)}%</span>
      <span class="score-label">similar</span>
      <span class="rank-badge">#{recording.rank}</span>
    </div>
  {:else}
    <div class="score-section">
      <span class="center-label">Selected recording</span>
    </div>
  {/if}

  <div class="meta-section">
    <span class="meta-item">{formatTimestamp(recording.timestamp)}</span>
    <span class="meta-item">{formatDuration(Math.floor(recording.duration_secs))}</span>
  </div>

  <div class="playback-section">
    {#if loadError}
      <div class="load-error">{loadError}</div>
    {:else if !midiData}
      <div class="loading">Loading MIDI...</div>
    {:else}
      <div class="midi-info">
        <span class="midi-stat">{midiData.tracks.length} track{midiData.tracks.length !== 1 ? 's' : ''}</span>
        <span class="midi-stat">{midiNotes.length} notes</span>
        <span class="midi-stat">{formatDuration(Math.floor(duration))}</span>
      </div>

      <div class="player-controls">
        <button class="play-btn" onclick={togglePlay} disabled={midiNotes.length === 0}>
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
    {/if}
  </div>

  <div class="action-section">
    <button class="goto-btn" onclick={goToSession}>
      Go to Session
    </button>
  </div>
</div>

<style>
  .recording-detail {
    width: 320px;
    min-width: 320px;
    display: flex;
    flex-direction: column;
    background: rgba(15, 15, 15, 0.98);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-left: none;
    border-radius: 0 0.25rem 0.25rem 0;
    overflow: hidden;
  }

  .detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.875rem 1rem 0.625rem;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }

  .rec-title {
    font-family: "Roboto", -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 1rem;
    font-weight: 500;
    color: #e8e6e3;
    letter-spacing: 0.02em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    border-radius: 0.25rem;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.06);
    color: #5a5a5a;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-left: 0.5rem;
    transition: all 0.15s ease;
  }

  .close-btn:hover {
    color: #e8e6e3;
    border-color: rgba(255, 255, 255, 0.12);
    background: rgba(255, 255, 255, 0.04);
  }

  .close-btn svg {
    width: 14px;
    height: 14px;
  }

  /* Score */
  .score-section {
    display: flex;
    align-items: baseline;
    gap: 0.375rem;
    padding: 0.625rem 1rem;
  }

  .score-value {
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 1.5rem;
    font-weight: 700;
    color: #c9a962;
    line-height: 1;
  }

  .score-label {
    font-size: 0.75rem;
    color: #6b6b6b;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .rank-badge {
    margin-left: auto;
    padding: 0.125rem 0.5rem;
    background: rgba(201, 169, 98, 0.12);
    border: 1px solid rgba(201, 169, 98, 0.25);
    border-radius: 1rem;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.75rem;
    color: #c9a962;
  }

  .center-label {
    font-size: 0.8125rem;
    color: #6b6b6b;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  /* Meta */
  .meta-section {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0 1rem 0.75rem;
  }

  .meta-item {
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.6875rem;
    color: #5a5a5a;
  }

  /* Playback */
  .playback-section {
    padding: 0.75rem 1rem 1rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
  }

  .midi-info {
    display: flex;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }

  .midi-stat {
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.6875rem;
    color: #5a5a5a;
  }

  .player-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .play-btn {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: rgba(201, 169, 98, 0.15);
    border: 1px solid rgba(201, 169, 98, 0.3);
    color: #c9a962;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .play-btn:hover:not(:disabled) {
    background: rgba(201, 169, 98, 0.25);
  }

  .play-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .play-btn svg {
    width: 14px;
    height: 14px;
  }

  .time-display {
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.6875rem;
    color: #5a5a5a;
    min-width: 32px;
    text-align: center;
  }

  .seek-bar {
    flex: 1;
    height: 4px;
    margin: 0 4px;
    -webkit-appearance: none;
    appearance: none;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 2px;
    cursor: pointer;
  }

  .seek-bar::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #c9a962;
    cursor: pointer;
  }

  .seek-bar::-moz-range-thumb {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #c9a962;
    cursor: pointer;
    border: none;
  }

  .loading {
    font-size: 0.75rem;
    color: #5a5a5a;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .load-error {
    font-size: 0.75rem;
    color: #8a5a5a;
  }

  /* Action */
  .action-section {
    padding: 0.75rem 1rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
  }

  .goto-btn {
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: transparent;
    border: 1px solid rgba(201, 169, 98, 0.3);
    border-radius: 0.25rem;
    color: #c9a962;
    font-family: inherit;
    font-size: 0.75rem;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .goto-btn:hover {
    background: rgba(201, 169, 98, 0.1);
    border-color: rgba(201, 169, 98, 0.4);
  }

  /* Light mode */
  :global(body.light-mode) .recording-detail {
    background: rgba(250, 250, 248, 0.98);
    border-color: rgba(0, 0, 0, 0.1);
  }

  :global(body.light-mode) .detail-header {
    border-bottom-color: rgba(0, 0, 0, 0.06);
  }

  :global(body.light-mode) .rec-title {
    color: #2a2a2a;
  }

  :global(body.light-mode) .close-btn {
    border-color: rgba(0, 0, 0, 0.1);
    color: #8a8a8a;
  }

  :global(body.light-mode) .close-btn:hover {
    color: #2a2a2a;
    border-color: rgba(0, 0, 0, 0.2);
    background: rgba(0, 0, 0, 0.04);
  }

  :global(body.light-mode) .score-value {
    color: #8a6a20;
  }

  :global(body.light-mode) .score-label {
    color: #7a7a7a;
  }

  :global(body.light-mode) .rank-badge {
    background: rgba(160, 128, 48, 0.12);
    border-color: rgba(160, 128, 48, 0.3);
    color: #8a6a20;
  }

  :global(body.light-mode) .center-label {
    color: #7a7a7a;
  }

  :global(body.light-mode) .meta-item {
    color: #7a7a7a;
  }

  :global(body.light-mode) .playback-section {
    border-top-color: rgba(0, 0, 0, 0.06);
  }

  :global(body.light-mode) .midi-stat {
    color: #7a7a7a;
  }

  :global(body.light-mode) .play-btn {
    background: rgba(160, 128, 48, 0.12);
    border-color: rgba(160, 128, 48, 0.3);
    color: #8a6a20;
  }

  :global(body.light-mode) .play-btn:hover:not(:disabled) {
    background: rgba(160, 128, 48, 0.2);
  }

  :global(body.light-mode) .time-display {
    color: #7a7a7a;
  }

  :global(body.light-mode) .seek-bar {
    background: rgba(0, 0, 0, 0.12);
  }

  :global(body.light-mode) .seek-bar::-webkit-slider-thumb {
    background: #8a6a20;
  }

  :global(body.light-mode) .seek-bar::-moz-range-thumb {
    background: #8a6a20;
  }

  :global(body.light-mode) .loading {
    color: #7a7a7a;
  }

  :global(body.light-mode) .load-error {
    color: #a06060;
  }

  :global(body.light-mode) .action-section {
    border-top-color: rgba(0, 0, 0, 0.06);
  }

  :global(body.light-mode) .goto-btn {
    border-color: rgba(160, 128, 48, 0.4);
    color: #8a6a20;
  }

  :global(body.light-mode) .goto-btn:hover {
    background: rgba(160, 128, 48, 0.1);
  }
</style>
