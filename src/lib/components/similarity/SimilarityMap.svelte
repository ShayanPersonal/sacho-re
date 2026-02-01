<script lang="ts">
  import { onMount } from 'svelte';
  import { 
    similarityData, 
    selectedPointId, 
    hoveredPointId,
    viewTransform,
    isCalculating,
    getClusterColor,
    refreshSimilarityData,
    recalculateSimilarity,
    selectPoint,
    hoverPoint,
    resetView,
    zoom,
    pan
  } from '$lib/stores/similarity';
  import { formatDate } from '$lib/api';
  
  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let width = $state(800);
  let height = $state(600);
  let isDragging = $state(false);
  let lastMousePos = $state({ x: 0, y: 0 });
  
  onMount(() => {
    const resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        width = entry.contentRect.width;
        height = entry.contentRect.height;
      }
    });
    resizeObserver.observe(container);
    
    return () => resizeObserver.disconnect();
  });
  
  $effect(() => {
    if (!canvas) return;
    
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    
    // Clear
    ctx.fillStyle = '#0a0a0f';
    ctx.fillRect(0, 0, width, height);
    
    // Draw grid
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.03)';
    ctx.lineWidth = 1;
    const gridSize = 50 * $viewTransform.scale;
    const offsetX = $viewTransform.x % gridSize;
    const offsetY = $viewTransform.y % gridSize;
    
    for (let x = offsetX; x < width; x += gridSize) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
      ctx.stroke();
    }
    for (let y = offsetY; y < height; y += gridSize) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }
    
    // Draw points
    for (const point of $similarityData.points) {
      const screenX = (point.x + 1) / 2 * (width - 40) + 20 + $viewTransform.x;
      const screenY = (point.y + 1) / 2 * (height - 40) + 20 + $viewTransform.y;
      
      const isSelected = $selectedPointId === point.id;
      const isHovered = $hoveredPointId === point.id;
      const radius = (isSelected || isHovered) ? 8 : 5;
      
      ctx.beginPath();
      ctx.arc(screenX * $viewTransform.scale, screenY * $viewTransform.scale, radius, 0, Math.PI * 2);
      
      const color = getClusterColor(point.cluster_id);
      
      if (isSelected) {
        ctx.fillStyle = '#fff';
        ctx.shadowColor = color;
        ctx.shadowBlur = 15;
      } else if (isHovered) {
        ctx.fillStyle = color;
        ctx.shadowColor = color;
        ctx.shadowBlur = 10;
      } else {
        ctx.fillStyle = color;
        ctx.shadowBlur = 0;
      }
      
      ctx.fill();
      ctx.shadowBlur = 0;
    }
  });
  
  function screenToWorld(screenX: number, screenY: number): { x: number; y: number } {
    const worldX = (screenX / $viewTransform.scale - $viewTransform.x - 20) / (width - 40) * 2 - 1;
    const worldY = (screenY / $viewTransform.scale - $viewTransform.y - 20) / (height - 40) * 2 - 1;
    return { x: worldX, y: worldY };
  }
  
  function findPointAt(screenX: number, screenY: number): string | null {
    const world = screenToWorld(screenX, screenY);
    
    for (const point of $similarityData.points) {
      const dx = point.x - world.x;
      const dy = point.y - world.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      
      if (dist < 0.05) {
        return point.id;
      }
    }
    
    return null;
  }
  
  function handleMouseMove(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    
    if (isDragging) {
      const dx = x - lastMousePos.x;
      const dy = y - lastMousePos.y;
      pan(dx / $viewTransform.scale, dy / $viewTransform.scale);
    } else {
      const pointId = findPointAt(x, y);
      hoverPoint(pointId);
    }
    
    lastMousePos = { x, y };
  }
  
  function handleMouseDown(e: MouseEvent) {
    isDragging = true;
    const rect = canvas.getBoundingClientRect();
    lastMousePos = {
      x: e.clientX - rect.left,
      y: e.clientY - rect.top
    };
  }
  
  function handleMouseUp() {
    isDragging = false;
  }
  
  function handleClick(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    
    const pointId = findPointAt(x, y);
    selectPoint(pointId);
  }
  
  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    
    zoom(-e.deltaY * 0.01, x, y);
  }
  
  async function handleRecalculate() {
    try {
      const count = await recalculateSimilarity();
      console.log(`Processed ${count} sessions`);
    } catch (error) {
      console.error('Failed to recalculate:', error);
    }
  }
</script>

<div class="similarity-map">
  <div class="map-header">
    <h2>Similarity Map</h2>
    <div class="map-stats">
      <span>{$similarityData.points.length} sessions</span>
      <span>•</span>
      <span>{$similarityData.clusters.length} clusters</span>
    </div>
    <div class="map-actions">
      <button class="action-btn" onclick={resetView}>Reset View</button>
      <button 
        class="action-btn primary" 
        onclick={handleRecalculate}
        disabled={$isCalculating}
      >
        {$isCalculating ? 'Calculating...' : 'Recalculate'}
      </button>
    </div>
  </div>
  
  <div class="map-container" bind:this={container}>
    <canvas 
      bind:this={canvas}
      {width}
      {height}
      onmousemove={handleMouseMove}
      onmousedown={handleMouseDown}
      onmouseup={handleMouseUp}
      onmouseleave={handleMouseUp}
      onclick={handleClick}
      onwheel={handleWheel}
    ></canvas>
    
    {#if $hoveredPointId}
      {@const point = $similarityData.points.find(p => p.id === $hoveredPointId)}
      {#if point}
        <div 
          class="tooltip"
          style="left: {(point.x + 1) / 2 * width + 20}px; top: {(point.y + 1) / 2 * height}px"
        >
          <div class="tooltip-content">
            <div class="tooltip-date">{formatDate(point.timestamp)}</div>
            {#if point.cluster_id !== null}
              <div class="tooltip-cluster">
                Cluster {point.cluster_id + 1}
              </div>
            {/if}
          </div>
        </div>
      {/if}
    {/if}
  </div>
  
  <div class="cluster-legend">
    {#each $similarityData.clusters as cluster}
      <div class="cluster-item">
        <div 
          class="cluster-dot" 
          style="background-color: {getClusterColor(cluster.id)}"
        ></div>
        <span class="cluster-name">{cluster.name}</span>
        <span class="cluster-count">{cluster.count}</span>
      </div>
    {/each}
    <div class="cluster-item">
      <div class="cluster-dot" style="background-color: #6b7280"></div>
      <span class="cluster-name">Unclustered</span>
    </div>
  </div>
</div>

<style>
  .similarity-map {
    display: flex;
    flex-direction: column;
    height: 100%;
    gap: 1rem;
  }
  
  .map-header {
    display: flex;
    align-items: center;
    gap: 1rem;
  }
  
  .map-header h2 {
    font-size: 1.125rem;
    font-weight: 600;
    color: #fff;
  }
  
  .map-stats {
    display: flex;
    gap: 0.5rem;
    font-size: 0.875rem;
    color: #71717a;
  }
  
  .map-actions {
    margin-left: auto;
    display: flex;
    gap: 0.5rem;
  }
  
  .action-btn {
    padding: 0.5rem 0.875rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 0.5rem;
    color: #a1a1aa;
    font-family: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .action-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    color: #e4e4e7;
  }
  
  .action-btn.primary {
    background: rgba(239, 68, 68, 0.15);
    border-color: rgba(239, 68, 68, 0.3);
    color: #ef4444;
  }
  
  .action-btn.primary:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.25);
  }
  
  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  
  .map-container {
    flex: 1;
    position: relative;
    background: #0a0a0f;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.75rem;
    overflow: hidden;
  }
  
  canvas {
    display: block;
    cursor: grab;
  }
  
  canvas:active {
    cursor: grabbing;
  }
  
  .tooltip {
    position: absolute;
    transform: translate(-50%, -100%);
    margin-top: -10px;
    pointer-events: none;
    z-index: 10;
  }
  
  .tooltip-content {
    padding: 0.5rem 0.75rem;
    background: rgba(0, 0, 0, 0.9);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 0.5rem;
    backdrop-filter: blur(10px);
  }
  
  .tooltip-date {
    font-size: 0.8125rem;
    color: #fff;
    white-space: nowrap;
  }
  
  .tooltip-cluster {
    font-size: 0.75rem;
    color: #71717a;
    margin-top: 0.25rem;
  }
  
  .cluster-legend {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    padding: 0.75rem;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 0.5rem;
  }
  
  .cluster-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8125rem;
  }
  
  .cluster-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }
  
  .cluster-name {
    color: #a1a1aa;
  }
  
  .cluster-count {
    color: #52525b;
    font-family: 'DM Mono', 'SF Mono', Menlo, monospace;
    font-size: 0.75rem;
  }
</style>
