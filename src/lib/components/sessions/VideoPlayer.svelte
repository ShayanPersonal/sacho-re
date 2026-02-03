<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  
  interface Props {
    sessionPath: string;
    filename: string;
    currentTime: number;
    isPlaying: boolean;
    onTimeUpdate?: (time: number) => void;
  }
  
  let { sessionPath, filename, currentTime, isPlaying, onTimeUpdate }: Props = $props();
  
  // Video info
  let videoInfo = $state<{
    width: number;
    height: number;
    fps: number;
    duration_ms: number;
    frame_count: number;
    codec: string;
  } | null>(null);
  
  // Current frame data
  let currentFrame = $state<string | null>(null);
  let loading = $state(true);
  let loadingProgress = $state('');
  let error = $state<string | null>(null);
  
  // Chunk-based buffering
  interface BufferedFrame {
    dataUrl: string;
    timestamp_ms: number;
    duration_ms: number;
  }
  
  interface Chunk {
    startMs: number;
    endMs: number;
    frames: BufferedFrame[];
  }
  
  // Buffer configuration
  const CHUNK_DURATION_MS = 15000; // 15-second chunks
  const MAX_LOADED_CHUNKS = 2; // Keep current + next chunk loaded
  
  let chunks: Map<number, Chunk> = new Map(); // chunkIndex -> Chunk
  let loadingChunks: Set<number> = new Set(); // chunks currently being loaded
  let isBuffering = $state(false);
  
  // Get chunk index for a given timestamp
  function getChunkIndex(timeMs: number): number {
    return Math.floor(timeMs / CHUNK_DURATION_MS);
  }
  
  // Get chunk start/end times
  function getChunkBounds(chunkIndex: number): { startMs: number; endMs: number } {
    const startMs = chunkIndex * CHUNK_DURATION_MS;
    const endMs = startMs + CHUNK_DURATION_MS;
    return { startMs, endMs };
  }
  
  // Load a specific chunk
  async function loadChunk(chunkIndex: number): Promise<Chunk | null> {
    if (!videoInfo) return null;
    if (chunks.has(chunkIndex)) return chunks.get(chunkIndex)!;
    if (loadingChunks.has(chunkIndex)) return null; // Already loading
    
    const { startMs, endMs } = getChunkBounds(chunkIndex);
    
    // Don't load chunks beyond video duration
    if (startMs >= videoInfo.duration_ms) return null;
    
    loadingChunks.add(chunkIndex);
    isBuffering = true;
    
    try {
      const actualEndMs = Math.min(endMs, videoInfo.duration_ms);
      console.log(`[VideoPlayer] Loading chunk ${chunkIndex} (${startMs}ms - ${actualEndMs}ms)`);
      
      const frames = await invoke<Array<{
        data_base64: string;
        timestamp_ms: number;
        duration_ms: number;
      }>>('get_video_frames_batch', {
        sessionPath,
        filename,
        startMs,
        endMs: actualEndMs,
        maxFrames: 500 // ~15 seconds at 30fps
      });
      
      const chunk: Chunk = {
        startMs,
        endMs: actualEndMs,
        frames: frames.map(f => ({
          dataUrl: `data:image/jpeg;base64,${f.data_base64}`,
          timestamp_ms: f.timestamp_ms,
          duration_ms: f.duration_ms
        }))
      };
      
      chunks.set(chunkIndex, chunk);
      console.log(`[VideoPlayer] Loaded chunk ${chunkIndex} with ${chunk.frames.length} frames`);
      
      return chunk;
    } catch (e) {
      console.error(`[VideoPlayer] Failed to load chunk ${chunkIndex}:`, e);
      return null;
    } finally {
      loadingChunks.delete(chunkIndex);
      // Update buffering state
      isBuffering = loadingChunks.size > 0;
    }
  }
  
  // Ensure chunks are loaded for a given time position
  async function ensureChunksLoaded(timeMs: number) {
    const currentChunkIndex = getChunkIndex(timeMs);
    const nextChunkIndex = currentChunkIndex + 1;
    
    // Determine which chunks we need
    const neededChunks = [currentChunkIndex, nextChunkIndex];
    
    // Unload chunks that are no longer needed (keep memory low)
    for (const [index] of chunks) {
      if (!neededChunks.includes(index)) {
        console.log(`[VideoPlayer] Unloading chunk ${index}`);
        chunks.delete(index);
      }
    }
    
    // Load needed chunks
    const loadPromises: Promise<Chunk | null>[] = [];
    for (const chunkIndex of neededChunks) {
      if (!chunks.has(chunkIndex) && !loadingChunks.has(chunkIndex)) {
        loadPromises.push(loadChunk(chunkIndex));
      }
    }
    
    if (loadPromises.length > 0) {
      await Promise.all(loadPromises);
    }
  }
  
  // Find the best frame for a given time from loaded chunks
  function getFrameForTime(timeMs: number): BufferedFrame | null {
    const chunkIndex = getChunkIndex(timeMs);
    const chunk = chunks.get(chunkIndex);
    
    if (!chunk || chunk.frames.length === 0) {
      // Try adjacent chunks
      const prevChunk = chunks.get(chunkIndex - 1);
      const nextChunk = chunks.get(chunkIndex + 1);
      
      if (prevChunk && prevChunk.frames.length > 0) {
        return prevChunk.frames[prevChunk.frames.length - 1];
      }
      if (nextChunk && nextChunk.frames.length > 0) {
        return nextChunk.frames[0];
      }
      return null;
    }
    
    // Find the frame that contains this timestamp
    for (let i = chunk.frames.length - 1; i >= 0; i--) {
      if (chunk.frames[i].timestamp_ms <= timeMs) {
        return chunk.frames[i];
      }
    }
    
    return chunk.frames[0];
  }
  
  // Load video info on mount
  async function loadVideoInfo() {
    try {
      loading = true;
      loadingProgress = 'Analyzing video...';
      error = null;
      chunks.clear();
      loadingChunks.clear();
      
      videoInfo = await invoke('get_video_info', {
        sessionPath,
        filename
      });
      
      console.log('[VideoPlayer] Loaded info:', videoInfo);
      
      // Load first two chunks
      loadingProgress = 'Loading video...';
      await ensureChunksLoaded(0);
      
      // Display first frame
      const frame = getFrameForTime(0);
      if (frame) {
        currentFrame = frame.dataUrl;
      }
      
      loading = false;
    } catch (e) {
      console.error('[VideoPlayer] Failed to load video info:', e);
      error = String(e);
      loading = false;
    }
  }
  
  // Track last time to detect seeks
  let lastTimeMs = 0;
  
  // Update displayed frame based on currentTime
  $effect(() => {
    if (!videoInfo) return;
    
    const timeMs = currentTime * 1000;
    
    // Find and display the appropriate frame
    const frame = getFrameForTime(timeMs);
    if (frame) {
      currentFrame = frame.dataUrl;
    }
    
    // Ensure chunks are loaded for current position
    // Do this async to not block frame display
    ensureChunksLoaded(timeMs);
    
    lastTimeMs = timeMs;
  });
  
  // Reload when source changes
  $effect(() => {
    if (sessionPath && filename) {
      loadVideoInfo();
    }
  });
  
  onMount(() => {
    loadVideoInfo();
  });
</script>

<div class="video-player">
  {#if loading}
    <div class="loading-overlay">
      <div class="spinner"></div>
      <span>{loadingProgress || 'Loading video...'}</span>
    </div>
  {:else if error}
    <div class="error-overlay">
      <span class="error-icon">⚠</span>
      <span class="error-text">{error}</span>
    </div>
  {:else if currentFrame}
    <img 
      src={currentFrame} 
      alt="Video frame"
      class="frame"
      draggable="false"
    />
    {#if videoInfo}
      <div class="info-badge">
        {videoInfo.width}×{videoInfo.height}
      </div>
    {/if}
    {#if isBuffering}
      <div class="buffering-indicator">
        <div class="mini-spinner"></div>
      </div>
    {/if}
  {:else}
    <div class="no-frame">
      <span>No frame available</span>
    </div>
  {/if}
</div>

<style>
  .video-player {
    position: relative;
    width: 100%;
    background: #0a0a0a;
    border-radius: 0.25rem;
    overflow: hidden;
    aspect-ratio: 16/9;
  }
  
  .frame {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  
  .loading-overlay,
  .error-overlay,
  .no-frame {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    color: #5a5a5a;
    font-size: 0.8125rem;
  }
  
  .spinner {
    width: 20px;
    height: 20px;
    border: 1.5px solid rgba(255, 255, 255, 0.08);
    border-top-color: #c9a962;
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }
  
  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
  
  .error-icon {
    font-size: 1.5rem;
    opacity: 0.4;
  }
  
  .error-text {
    text-align: center;
    max-width: 80%;
  }
  
  .info-badge {
    position: absolute;
    top: 0.5rem;
    left: 0.5rem;
    padding: 0.1875rem 0.4375rem;
    background: rgba(0, 0, 0, 0.75);
    border-radius: 0.125rem;
    font-size: 0.625rem;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    color: #8a8a8a;
    letter-spacing: 0.02em;
  }
  
  .buffering-indicator {
    position: absolute;
    top: 0.5rem;
    right: 0.5rem;
    padding: 0.25rem;
    background: rgba(0, 0, 0, 0.75);
    border-radius: 0.125rem;
  }
  
  .mini-spinner {
    width: 12px;
    height: 12px;
    border: 1.5px solid rgba(255, 255, 255, 0.15);
    border-top-color: #c9a962;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Light mode overrides */
  :global(body.light-mode) .video-player {
    background: #e8e8e8;
  }

  :global(body.light-mode) .loading-overlay,
  :global(body.light-mode) .error-overlay,
  :global(body.light-mode) .no-frame {
    color: #6a6a6a;
  }

  :global(body.light-mode) .spinner {
    border-color: rgba(0, 0, 0, 0.1);
    border-top-color: #a08030;
  }

  :global(body.light-mode) .error-icon {
    opacity: 0.5;
  }

  :global(body.light-mode) .info-badge {
    background: rgba(255, 255, 255, 0.85);
    color: #5a5a5a;
  }

  :global(body.light-mode) .buffering-indicator {
    background: rgba(255, 255, 255, 0.85);
  }

  :global(body.light-mode) .mini-spinner {
    border-color: rgba(0, 0, 0, 0.15);
    border-top-color: #a08030;
  }
</style>
