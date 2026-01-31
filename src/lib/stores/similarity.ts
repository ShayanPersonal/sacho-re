// Similarity map state store

import { writable, derived } from 'svelte/store';
import type { SimilarityData, SimilarityPoint, ClusterInfo } from '$lib/api';
import { getSimilarityData, recalculateSimilarity as apiRecalculate } from '$lib/api';

// Store for similarity data
export const similarityData = writable<SimilarityData>({
  points: [],
  clusters: []
});

// Selected point
export const selectedPointId = writable<string | null>(null);

// Hover point
export const hoveredPointId = writable<string | null>(null);

// View state
export const viewTransform = writable({
  x: 0,
  y: 0,
  scale: 1
});

// Loading state
export const isCalculating = writable(false);

// Derived stores
export const points = derived(similarityData, $data => $data.points);
export const clusters = derived(similarityData, $data => $data.clusters);

export const selectedPoint = derived(
  [similarityData, selectedPointId],
  ([$data, $id]) => $id ? $data.points.find(p => p.id === $id) : null
);

export const hoveredPoint = derived(
  [similarityData, hoveredPointId],
  ([$data, $id]) => $id ? $data.points.find(p => p.id === $id) : null
);

// Get cluster color
export function getClusterColor(clusterId: number | null): string {
  if (clusterId === null) return '#6b7280'; // Gray for unclustered
  
  const colors = [
    '#ef4444', // red
    '#f97316', // orange
    '#eab308', // yellow
    '#22c55e', // green
    '#14b8a6', // teal
    '#3b82f6', // blue
    '#8b5cf6', // violet
    '#ec4899', // pink
    '#06b6d4', // cyan
    '#84cc16', // lime
  ];
  
  return colors[clusterId % colors.length];
}

// Actions
export async function refreshSimilarityData() {
  try {
    const data = await getSimilarityData();
    similarityData.set(data);
  } catch (error) {
    console.error('Failed to fetch similarity data:', error);
  }
}

export async function recalculateSimilarity() {
  isCalculating.set(true);
  try {
    const count = await apiRecalculate();
    console.log(`Recalculated similarity for ${count} sessions`);
    await refreshSimilarityData();
    return count;
  } catch (error) {
    console.error('Failed to recalculate similarity:', error);
    throw error;
  } finally {
    isCalculating.set(false);
  }
}

export function selectPoint(pointId: string | null) {
  selectedPointId.set(pointId);
}

export function hoverPoint(pointId: string | null) {
  hoveredPointId.set(pointId);
}

export function resetView() {
  viewTransform.set({ x: 0, y: 0, scale: 1 });
}

export function zoom(delta: number, centerX: number, centerY: number) {
  viewTransform.update(t => {
    const newScale = Math.max(0.1, Math.min(10, t.scale * (1 + delta * 0.1)));
    const scaleChange = newScale / t.scale;
    
    return {
      x: centerX - (centerX - t.x) * scaleChange,
      y: centerY - (centerY - t.y) * scaleChange,
      scale: newScale
    };
  });
}

export function pan(dx: number, dy: number) {
  viewTransform.update(t => ({
    ...t,
    x: t.x + dx,
    y: t.y + dy
  }));
}

// Initialize
refreshSimilarityData();
