<script lang="ts">
  import { onMount } from 'svelte';
  import {
    importedFiles,
    selectedFileId,
    similarFiles,
    similarityMode,
    isImporting,
    isComputing,
    selectedFile,
    importProgress,
    importFolder,
    selectFile,
    switchMode,
    clearImports,
  } from '$lib/stores/similarity';
  import type { SimilarityMode, MidiImportInfo } from '$lib/api';
  import { settings } from '$lib/stores/settings';
  import MidiFileDetail from './MidiFileDetail.svelte';

  let isLightMode = $derived(!($settings?.dark_mode ?? false));

  let canvas: HTMLCanvasElement;
  let canvasContainer: HTMLDivElement;
  let canvasWidth = $state(600);
  let canvasHeight = $state(400);

  // Animation state
  let animationProgress = $state(0);
  let animationStart = $state(0);
  let animationId = $state(0);
  let hasAnimated = $state(false);
  const ANIMATION_DURATION = 800; // ms

  // Golden spiral: r = C·φ^(2θ/π), so θ = π/(2·ln(φ)) · ln(r/C)
  const PHI = (1 + Math.sqrt(5)) / 2;
  const SPIRAL_K = Math.PI / (2 * Math.log(PHI));
  const STAGGER = 0.5; // spread node starts over first 50% of animation

  function goldenSpiralPos(
    cx: number, cy: number,
    finalAngle: number, finalDist: number,
    progress: number, delay: number
  ): { x: number; y: number } {
    const nodeProgress = Math.max(0, (progress - delay) / (1 - delay));
    if (nodeProgress <= 0) return { x: cx, y: cy };
    const currentR = finalDist * nodeProgress;
    // Angle offset from golden spiral path: as r grows, angle winds toward final
    const spiralOffset = SPIRAL_K * Math.log(nodeProgress);
    const currentAngle = finalAngle + spiralOffset;
    return {
      x: cx + Math.cos(currentAngle) * currentR,
      y: cy + Math.sin(currentAngle) * currentR,
    };
  }

  // Hover state
  let hoveredIndex = $state<number | null>(null);
  let tooltipX = $state(0);
  let tooltipY = $state(0);

  // Detail panel state
  interface InspectedFile {
    file: MidiImportInfo;
    score: number | null;
    rank: number | null;
    matchOffsetSecs: number | null;
  }
  let inspectedResult = $state<InspectedFile | null>(null);
  let hoveredCenter = $state(false);

  // Search + virtual scroll for large file lists
  let searchQuery = $state('');
  let fileListEl = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let listHeight = $state(400);
  const ITEM_HEIGHT = 34; // px per file-item row
  const OVERSCAN = 10;

  let filteredFiles = $derived(
    searchQuery.trim()
      ? $importedFiles.filter(f => f.file_name.toLowerCase().includes(searchQuery.toLowerCase()))
      : $importedFiles
  );

  let virtualSlice = $derived.by(() => {
    const total = filteredFiles.length;
    const startIdx = Math.max(0, Math.floor(scrollTop / ITEM_HEIGHT) - OVERSCAN);
    const visibleCount = Math.ceil(listHeight / ITEM_HEIGHT) + OVERSCAN * 2;
    const endIdx = Math.min(total, startIdx + visibleCount);
    return {
      items: filteredFiles.slice(startIdx, endIdx),
      startIdx,
      totalHeight: total * ITEM_HEIGHT,
      offsetY: startIdx * ITEM_HEIGHT,
    };
  });

  function handleFileListScroll(e: Event) {
    const el = e.target as HTMLDivElement;
    scrollTop = el.scrollTop;
  }

  onMount(() => {
    const resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        if (entry.target === canvasContainer) {
          canvasWidth = entry.contentRect.width;
          canvasHeight = entry.contentRect.height;
        } else if (entry.target === fileListEl) {
          listHeight = entry.contentRect.height;
        }
      }
    });
    if (canvasContainer) resizeObserver.observe(canvasContainer);
    if (fileListEl) resizeObserver.observe(fileListEl);
    return () => {
      resizeObserver.disconnect();
      if (animationId) cancelAnimationFrame(animationId);
    };
  });

  // Restart animation when selection changes
  $effect(() => {
    // Access reactive deps
    const _ = $similarFiles;
    const __ = $selectedFileId;
    if (hasAnimated) {
      // Skip animation on subsequent selections — snap to final positions
      animationProgress = 1;
      return;
    }
    hasAnimated = true;
    animationProgress = 0;
    animationStart = performance.now();
    animate();
  });

  // Clear inspected result when the center file changes (sidebar click or double-click recenter)
  $effect(() => {
    const _ = $selectedFileId;
    inspectedResult = null;
  });

  function animate() {
    const elapsed = performance.now() - animationStart;
    const t = Math.min(elapsed / ANIMATION_DURATION, 1);
    animationProgress = 1 - Math.pow(1 - t, 3); // ease-out cubic

    if (t < 1) {
      animationId = requestAnimationFrame(animate);
    } else {
      animationProgress = 1;
    }
  }

  // Score to color: gold (high) -> muted (low)
  function scoreColor(score: number): string {
    if (isLightMode) {
      const r = Math.round(140 + score * 40);
      const g = Math.round(100 + score * 50);
      const b = Math.round(30 + score * 10);
      return `rgb(${r}, ${g}, ${b})`;
    }
    const r = Math.round(120 + score * 100);
    const g = Math.round(100 + score * 80);
    const b = Math.round(50 + score * 50);
    return `rgb(${r}, ${g}, ${b})`;
  }

  $effect(() => {
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const w = canvasWidth;
    const h = canvasHeight;
    const cx = w / 2;
    const cy = h / 2;

    // Clear
    ctx.fillStyle = isLightMode ? '#f0f0ee' : '#0a0a0a';
    ctx.fillRect(0, 0, w, h);

    // Subtle radial grid
    ctx.strokeStyle = isLightMode ? 'rgba(0, 0, 0, 0.04)' : 'rgba(255, 255, 255, 0.025)';
    ctx.lineWidth = 1;
    const maxR = Math.min(w, h) * 0.60;
    for (let i = 1; i <= 4; i++) {
      ctx.beginPath();
      ctx.arc(cx, cy, maxR * (i / 4), 0, Math.PI * 2);
      ctx.stroke();
    }

    if (!$selectedFileId || ($similarFiles.length === 0 && !$selectedFile)) {
      // Empty/no-selection state
      ctx.fillStyle = isLightMode ? '#8a8a8a' : '#4a4a4a';
      ctx.font = '14px Roboto, sans-serif';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      if ($importedFiles.length === 0) {
        ctx.fillText('Import a folder of MIDI files to get started', cx, cy);
      } else if (!$selectedFileId) {
        ctx.fillText('Select a file from the sidebar', cx, cy);
      } else if ($isComputing) {
        ctx.fillText('Computing similarity...', cx, cy);
      }
      return;
    }

    // Selected file with no features: show center node + message
    if ($selectedFile && !$selectedFile.has_features) {
      // Draw center node so user can click to preview
      ctx.shadowColor = isLightMode ? 'rgba(130, 130, 130, 0.4)' : 'rgba(130, 130, 130, 0.3)';
      ctx.shadowBlur = 10;
      ctx.beginPath();
      ctx.arc(cx, cy, 12, 0, Math.PI * 2);
      ctx.fillStyle = isLightMode ? '#9a9a9a' : '#6a6a6a';
      ctx.fill();
      ctx.shadowBlur = 0;

      // File name
      ctx.fillStyle = isLightMode ? '#2a2a2a' : '#e8e6e3';
      ctx.font = '11px Roboto, sans-serif';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      const name = $selectedFile.file_name.length > 20
        ? $selectedFile.file_name.slice(0, 18) + '...'
        : $selectedFile.file_name;
      ctx.fillText(name, cx, cy + 18);

      // Message
      ctx.fillStyle = isLightMode ? '#8a8a8a' : '#5a5a5a';
      ctx.font = '12px Roboto, sans-serif';
      ctx.textBaseline = 'middle';
      ctx.fillText('Track too short for similarity', cx, cy + 46);
      return;
    }

    if ($similarFiles.length === 0) {
      return;
    }

    const n = $similarFiles.length;
    const progress = animationProgress;

    // Draw satellite nodes
    for (let i = 0; i < n; i++) {
      const result = $similarFiles[i];
      const angle = (i / n) * Math.PI * 2 - Math.PI / 2;
      const dist = Math.min((result.rank / (n + 1)) * maxR, maxR * 0.75);
      const delay = (i / n) * STAGGER;
      const pos = goldenSpiralPos(cx, cy, angle, dist, progress, delay);

      const isHov = hoveredIndex === i;
      const radius = isHov ? 10 : 7;

      // Shadow
      ctx.shadowColor = scoreColor(result.score);
      ctx.shadowBlur = isHov ? 12 : 4;

      ctx.beginPath();
      ctx.arc(pos.x, pos.y, radius, 0, Math.PI * 2);
      ctx.fillStyle = scoreColor(result.score);
      ctx.fill();

      ctx.shadowBlur = 0;

      // Score label near node
      if (progress > 0.5) {
        const labelAlpha = (progress - 0.5) * 2;
        ctx.fillStyle = isLightMode
          ? `rgba(90, 90, 90, ${labelAlpha * 0.7})`
          : `rgba(150, 150, 150, ${labelAlpha * 0.6})`;
        ctx.font = '10px "DM Mono", monospace';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'bottom';
        ctx.fillText(`${Math.round(result.score * 100)}%`, pos.x, pos.y - radius - 4);
      }
    }

    // Draw center node (selected file)
    ctx.shadowColor = isLightMode ? 'rgba(160, 128, 48, 0.6)' : 'rgba(201, 169, 98, 0.5)';
    ctx.shadowBlur = 16;
    ctx.beginPath();
    ctx.arc(cx, cy, 12, 0, Math.PI * 2);
    ctx.fillStyle = isLightMode ? '#a08030' : '#c9a962';
    ctx.fill();
    ctx.shadowBlur = 0;

    // Center label
    if ($selectedFile) {
      ctx.fillStyle = isLightMode ? '#2a2a2a' : '#e8e6e3';
      ctx.font = '11px Roboto, sans-serif';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      const name = $selectedFile.file_name.length > 20
        ? $selectedFile.file_name.slice(0, 18) + '...'
        : $selectedFile.file_name;
      ctx.fillText(name, cx, cy + 18);
    }
  });

  function handleCanvasMove(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const cx = canvasWidth / 2;
    const cy = canvasHeight / 2;
    const maxR = Math.min(canvasWidth, canvasHeight) * 0.60;
    const n = $similarFiles.length;

    let found: number | null = null;
    for (let i = 0; i < n; i++) {
      const result = $similarFiles[i];
      const angle = (i / n) * Math.PI * 2 - Math.PI / 2;
      const dist = Math.min((result.rank / (n + 1)) * maxR, maxR * 0.75);
      const delay = (i / n) * STAGGER;
      const pos = goldenSpiralPos(cx, cy, angle, dist, animationProgress, delay);
      const dx = mx - pos.x;
      const dy = my - pos.y;
      if (Math.sqrt(dx * dx + dy * dy) < 14) {
        found = i;
        tooltipX = e.clientX - rect.left;
        tooltipY = e.clientY - rect.top;
        break;
      }
    }
    hoveredIndex = found;

    // Check center node (radius 12)
    if (found === null && $selectedFileId) {
      const dcx = mx - cx;
      const dcy = my - cy;
      hoveredCenter = Math.sqrt(dcx * dcx + dcy * dcy) < 16;
    } else {
      hoveredCenter = false;
    }

    canvas.style.cursor = (found !== null || hoveredCenter) ? 'pointer' : 'default';
  }

  function handleCanvasClick(e: MouseEvent) {
    if (hoveredIndex !== null) {
      const clicked = $similarFiles[hoveredIndex];
      inspectedResult = { file: clicked.file, score: clicked.score, rank: clicked.rank, matchOffsetSecs: clicked.match_offset_secs };
    } else if (hoveredCenter && $selectedFile) {
      inspectedResult = { file: $selectedFile, score: null, rank: null, matchOffsetSecs: null };
    } else {
      // Click on empty canvas background dismisses the panel
      inspectedResult = null;
    }
  }

  function handleCanvasDoubleClick(e: MouseEvent) {
    if (hoveredIndex !== null) {
      const clicked = $similarFiles[hoveredIndex];
      inspectedResult = null;
      selectFile(clicked.file.id);
    }
  }

  function handleCanvasLeave() {
    hoveredIndex = null;
    hoveredCenter = false;
  }
</script>

<div class="similarity-tab">
  <div class="sidebar">
    <div class="sidebar-header">
      <h2>Similarity</h2>
    </div>

    <div class="sidebar-actions-top">
      <div class="import-wrapper">
        <button
          class="action-btn primary"
          onclick={importFolder}
          disabled={$isImporting}
        >
          {#if $isImporting && $importProgress}
            Importing {$importProgress.current} / {$importProgress.total}
          {:else if $isImporting}
            Importing...
          {:else}
            Import Folder
          {/if}
        </button>
        {#if $isImporting && $importProgress}
          <div class="import-progress-track">
            <div
              class="import-progress-bar"
              style="width: {($importProgress.current / $importProgress.total) * 100}%"
            ></div>
          </div>
          <div class="import-file-name" title={$importProgress.file_name}>
            {$importProgress.file_name}
          </div>
        {/if}
      </div>

      <div class="mode-toggle">
        <button
          class="mode-btn"
          class:active={$similarityMode === 'melodic'}
          onclick={() => switchMode('melodic')}
        >Melodic</button>
        <button
          class="mode-btn"
          class:active={$similarityMode === 'harmonic'}
          onclick={() => switchMode('harmonic')}
        >Harmonic</button>
      </div>
    </div>

    {#if $importedFiles.length > 0}
      <div class="search-wrapper">
        <input
          class="search-input"
          type="text"
          placeholder="Search {$importedFiles.length.toLocaleString()} files..."
          bind:value={searchQuery}
        />
      </div>
    {/if}

    <div
      class="file-list"
      bind:this={fileListEl}
      onscroll={handleFileListScroll}
    >
      {#if $importedFiles.length === 0}
        <div class="empty-state">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="32" height="32">
            <path d="M9 18V5l12-2v13" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="16" r="3" />
          </svg>
          <span>No MIDI files imported</span>
        </div>
      {:else}
        <div style="height: {virtualSlice.totalHeight}px; position: relative;">
          <div style="position: absolute; top: {virtualSlice.offsetY}px; left: 0; right: 0;">
            {#each virtualSlice.items as file (file.id)}
              <button
                class="file-item"
                class:selected={$selectedFileId === file.id}
                class:muted={!file.has_features}
                onclick={() => selectFile(file.id)}
              >
                <svg class="file-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M9 18V5l12-2v13" />
                  <circle cx="6" cy="18" r="3" />
                  <circle cx="18" cy="16" r="3" />
                </svg>
                <span class="file-name" title={file.file_name}>{file.file_name}</span>
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>

    {#if $importedFiles.length > 0}
      <div class="sidebar-footer">
        <span class="file-count">{$importedFiles.length} files</span>
        <button class="action-btn danger-text" onclick={clearImports}>Clear</button>
      </div>
    {/if}
  </div>

  <div class="canvas-area" class:has-detail={inspectedResult !== null}>
    <div class="canvas-container" bind:this={canvasContainer}>
      <canvas
        bind:this={canvas}
        width={canvasWidth}
        height={canvasHeight}
        onmousemove={handleCanvasMove}
        onclick={handleCanvasClick}
        ondblclick={handleCanvasDoubleClick}
        onmouseleave={handleCanvasLeave}
      ></canvas>

      {#if hoveredIndex !== null && $similarFiles[hoveredIndex]}
        {@const result = $similarFiles[hoveredIndex]}
        <div
          class="tooltip"
          style="left: {tooltipX}px; top: {tooltipY - 10}px"
        >
          <div class="tooltip-content">
            <div class="tooltip-name">{result.file.file_name}</div>
            <div class="tooltip-score">{Math.round(result.score * 100)}% similar</div>
          </div>
        </div>
      {/if}

      {#if $isComputing}
        <div class="computing-overlay">
          <span>Computing...</span>
        </div>
      {/if}
    </div>
  </div>

  {#if inspectedResult}
    <MidiFileDetail
      file={inspectedResult.file}
      score={inspectedResult.score}
      rank={inspectedResult.rank}
      matchOffsetSecs={inspectedResult.matchOffsetSecs}
      onClose={() => inspectedResult = null}
    />
  {/if}
</div>

<style>
  .similarity-tab {
    display: flex;
    height: 100%;
    gap: 0;
  }

  /* ---- SIDEBAR ---- */
  .sidebar {
    width: 280px;
    min-width: 280px;
    display: flex;
    flex-direction: column;
    background: rgba(255, 255, 255, 0.015);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem 0 0 0.25rem;
    overflow: hidden;
  }

  .sidebar-header {
    padding: 0.875rem 1rem 0.5rem;
  }

  .sidebar-header h2 {
    font-family: "Roboto", -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 1.125rem;
    font-weight: 500;
    color: #e8e6e3;
    letter-spacing: 0.02em;
  }

  .sidebar-actions-top {
    padding: 0 0.75rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .action-btn {
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
    transition: all 0.2s ease;
  }

  .action-btn:hover:not(:disabled) {
    color: #a8a8a8;
    border-color: rgba(255, 255, 255, 0.1);
  }

  .action-btn.primary {
    border-color: rgba(201, 169, 98, 0.3);
    color: #c9a962;
    width: 100%;
  }

  .action-btn.primary:hover:not(:disabled) {
    background: rgba(201, 169, 98, 0.1);
    border-color: rgba(201, 169, 98, 0.4);
  }

  .action-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .import-wrapper {
    width: 100%;
  }

  .import-progress-track {
    width: 100%;
    height: 3px;
    background: rgba(255, 255, 255, 0.06);
    border-radius: 1.5px;
    margin-top: 0.375rem;
    overflow: hidden;
  }

  .import-progress-bar {
    height: 100%;
    background: #c9a962;
    border-radius: 1.5px;
    transition: width 0.15s ease;
  }

  .import-file-name {
    margin-top: 0.25rem;
    font-size: 0.6875rem;
    color: #5a5a5a;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
  }

  .action-btn.danger-text {
    border: none;
    color: #8a5a5a;
    padding: 0.25rem 0.5rem;
  }

  .action-btn.danger-text:hover {
    color: #c05050;
  }

  .search-wrapper {
    padding: 0 0.75rem 0.5rem;
  }

  .search-input {
    width: 100%;
    padding: 0.375rem 0.625rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    color: #c8c8c8;
    font-family: inherit;
    font-size: 0.75rem;
    outline: none;
    transition: border-color 0.15s ease;
    box-sizing: border-box;
  }

  .search-input::placeholder {
    color: #4a4a4a;
  }

  .search-input:focus {
    border-color: rgba(201, 169, 98, 0.4);
  }

  .mode-toggle {
    display: flex;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.25rem;
    overflow: hidden;
  }

  .mode-btn {
    flex: 1;
    padding: 0.375rem 0.5rem;
    background: transparent;
    border: none;
    color: #5a5a5a;
    font-family: inherit;
    font-size: 0.6875rem;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .mode-btn:first-child {
    border-right: 1px solid rgba(255, 255, 255, 0.06);
  }

  .mode-btn.active {
    background: rgba(201, 169, 98, 0.12);
    color: #c9a962;
  }

  .mode-btn:hover:not(.active) {
    color: #8a8a8a;
  }

  /* ---- FILE LIST ---- */
  .file-list {
    flex: 1;
    overflow-y: auto;
    padding: 0 0.5rem;
  }

  .file-list::-webkit-scrollbar {
    width: 4px;
  }

  .file-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .file-list::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.08);
    border-radius: 2px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
    padding: 2rem 1rem;
    color: #4a4a4a;
    font-size: 0.8125rem;
    text-align: center;
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    height: 34px;
    padding: 0 0.625rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 0.25rem;
    color: #8a8a8a;
    font-family: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
    transition: all 0.15s ease;
    text-align: left;
    box-sizing: border-box;
    flex-shrink: 0;
  }

  .file-item:hover {
    background: rgba(255, 255, 255, 0.03);
    color: #b8b8b8;
  }

  .file-item.selected {
    background: rgba(201, 169, 98, 0.1);
    border-color: rgba(201, 169, 98, 0.2);
    color: #c9a962;
  }

  .file-item.muted {
    opacity: 0.35;
  }

  .file-icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    opacity: 0.5;
  }

  .file-item.selected .file-icon {
    opacity: 1;
  }

  .file-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sidebar-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
  }

  .file-count {
    font-size: 0.75rem;
    color: #5a5a5a;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
  }

  /* ---- CANVAS ---- */
  .canvas-area {
    flex: 1;
    min-width: 0;
  }

  .canvas-container {
    width: 100%;
    height: 100%;
    position: relative;
    background: #0a0a0a;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-left: none;
    border-radius: 0 0.25rem 0.25rem 0;
    overflow: hidden;
  }

  .has-detail .canvas-container {
    border-radius: 0;
    border-right: none;
  }

  canvas {
    display: block;
  }

  .tooltip {
    position: absolute;
    transform: translate(-50%, -100%);
    pointer-events: none;
    z-index: 10;
  }

  .tooltip-content {
    padding: 0.5rem 0.75rem;
    background: rgba(10, 10, 10, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.25rem;
    backdrop-filter: blur(10px);
  }

  .tooltip-name {
    font-size: 0.8125rem;
    color: #e8e6e3;
    white-space: nowrap;
  }

  .tooltip-score {
    font-size: 0.75rem;
    color: #c9a962;
    margin-top: 0.125rem;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
  }

  .computing-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(10, 10, 10, 0.5);
    backdrop-filter: blur(2px);
    color: #8a8a8a;
    font-size: 0.875rem;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  /* ---- LIGHT MODE ---- */
  :global(body.light-mode) .sidebar {
    background: rgba(255, 255, 255, 0.7);
    border-color: rgba(0, 0, 0, 0.1);
  }

  :global(body.light-mode) .sidebar-header h2 {
    color: #2a2a2a;
  }

  :global(body.light-mode) .action-btn {
    border-color: rgba(0, 0, 0, 0.12);
    color: #5a5a5a;
  }

  :global(body.light-mode) .action-btn:hover:not(:disabled) {
    color: #3a3a3a;
    border-color: rgba(0, 0, 0, 0.2);
  }

  :global(body.light-mode) .action-btn.primary {
    border-color: rgba(160, 128, 48, 0.4);
    color: #8a6a20;
  }

  :global(body.light-mode) .action-btn.primary:hover:not(:disabled) {
    background: rgba(160, 128, 48, 0.1);
  }

  :global(body.light-mode) .import-progress-track {
    background: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .import-progress-bar {
    background: #a08030;
  }

  :global(body.light-mode) .import-file-name {
    color: #7a7a7a;
  }

  :global(body.light-mode) .action-btn.danger-text {
    color: #a06060;
  }

  :global(body.light-mode) .action-btn.danger-text:hover {
    color: #c04040;
  }

  :global(body.light-mode) .mode-toggle {
    border-color: rgba(0, 0, 0, 0.12);
  }

  :global(body.light-mode) .mode-btn {
    color: #7a7a7a;
  }

  :global(body.light-mode) .mode-btn:first-child {
    border-right-color: rgba(0, 0, 0, 0.12);
  }

  :global(body.light-mode) .mode-btn.active {
    background: rgba(160, 128, 48, 0.15);
    color: #8a6a20;
  }

  :global(body.light-mode) .mode-btn:hover:not(.active) {
    color: #4a4a4a;
  }

  :global(body.light-mode) .search-input {
    background: rgba(0, 0, 0, 0.03);
    border-color: rgba(0, 0, 0, 0.1);
    color: #2a2a2a;
  }

  :global(body.light-mode) .search-input::placeholder {
    color: #9a9a9a;
  }

  :global(body.light-mode) .search-input:focus {
    border-color: rgba(160, 128, 48, 0.5);
  }

  :global(body.light-mode) .empty-state {
    color: #8a8a8a;
  }

  :global(body.light-mode) .file-item {
    color: #5a5a5a;
  }

  :global(body.light-mode) .file-item:hover {
    background: rgba(0, 0, 0, 0.03);
    color: #3a3a3a;
  }

  :global(body.light-mode) .file-item.selected {
    background: rgba(160, 128, 48, 0.12);
    border-color: rgba(160, 128, 48, 0.25);
    color: #8a6a20;
  }

  :global(body.light-mode) .file-count {
    color: #7a7a7a;
  }

  :global(body.light-mode) .sidebar-footer {
    border-top-color: rgba(0, 0, 0, 0.08);
  }

  :global(body.light-mode) .canvas-container {
    background: #f0f0ee;
    border-color: rgba(0, 0, 0, 0.1);
  }

  :global(body.light-mode) .has-detail .canvas-container {
    border-right: none;
  }

  :global(body.light-mode) .tooltip-content {
    background: rgba(255, 255, 255, 0.95);
    border-color: rgba(0, 0, 0, 0.12);
  }

  :global(body.light-mode) .tooltip-name {
    color: #2a2a2a;
  }

  :global(body.light-mode) .tooltip-score {
    color: #8a6a20;
  }

  :global(body.light-mode) .computing-overlay {
    background: rgba(245, 245, 243, 0.6);
    color: #6a6a6a;
  }

  :global(body.light-mode) .file-list::-webkit-scrollbar-thumb {
    background: rgba(0, 0, 0, 0.15);
  }
</style>
