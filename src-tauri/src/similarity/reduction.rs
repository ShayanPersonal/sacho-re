// Dimensionality reduction using UMAP

use crate::session::{MidiFeatures, SimilarityCoords};
use ndarray::{Array2, ArrayView2};

/// UMAP parameters
#[derive(Debug, Clone)]
pub struct UmapParams {
    pub n_neighbors: usize,
    pub min_dist: f32,
    pub n_components: usize,
    pub random_seed: u64,
}

impl Default for UmapParams {
    fn default() -> Self {
        Self {
            n_neighbors: 15,
            min_dist: 0.1,
            n_components: 2,
            random_seed: 42,
        }
    }
}

/// Reduce high-dimensional feature vectors to 2D coordinates
/// 
/// Note: This is a simplified implementation. For production use,
/// consider using a proper UMAP implementation via linfa-reduction
/// or calling out to Python/umap-learn via a subprocess.
pub fn reduce_to_2d(features: &[MidiFeatures], params: &UmapParams) -> Vec<SimilarityCoords> {
    if features.is_empty() {
        return Vec::new();
    }
    
    if features.len() == 1 {
        return vec![SimilarityCoords { x: 0.0, y: 0.0 }];
    }
    
    // Convert features to matrix
    let n_samples = features.len();
    let feature_vecs: Vec<Vec<f32>> = features.iter()
        .map(|f| f.to_vector())
        .collect();
    
    let n_features = feature_vecs[0].len();
    
    // Build feature matrix
    let mut data = Vec::with_capacity(n_samples * n_features);
    for vec in &feature_vecs {
        data.extend(vec.iter().map(|&v| v as f64));
    }
    
    let matrix = Array2::from_shape_vec((n_samples, n_features), data)
        .expect("Failed to create feature matrix");
    
    // Apply PCA first to reduce to ~10 dimensions (for efficiency)
    let pca_result = simple_pca(&matrix.view(), 10.min(n_features));
    
    // Then apply a simple neighbor-based projection
    let coords = neighbor_projection(&pca_result, params.n_neighbors);
    
    coords.into_iter()
        .map(|(x, y)| SimilarityCoords { x, y })
        .collect()
}

/// Simple PCA implementation for dimensionality reduction
fn simple_pca(data: &ArrayView2<f64>, n_components: usize) -> Array2<f64> {
    let n_samples = data.nrows();
    let n_features = data.ncols();
    
    if n_samples <= n_components {
        // Just center the data
        let mean: Vec<f64> = (0..n_features)
            .map(|j| data.column(j).sum() / n_samples as f64)
            .collect();
        
        let mut centered = data.to_owned();
        for i in 0..n_samples {
            for j in 0..n_features {
                centered[[i, j]] -= mean[j];
            }
        }
        return centered;
    }
    
    // Center the data
    let mean: Vec<f64> = (0..n_features)
        .map(|j| data.column(j).sum() / n_samples as f64)
        .collect();
    
    let mut centered = data.to_owned();
    for i in 0..n_samples {
        for j in 0..n_features {
            centered[[i, j]] -= mean[j];
        }
    }
    
    // Use power iteration to find principal components
    let mut result = Array2::zeros((n_samples, n_components));
    let mut used_vecs: Vec<Vec<f64>> = Vec::new();
    
    for comp in 0..n_components {
        // Random initial vector
        let mut v: Vec<f64> = (0..n_features)
            .map(|i| ((i * 17 + comp * 31) % 100) as f64 / 100.0 - 0.5)
            .collect();
        
        // Orthogonalize against previous components
        for prev in &used_vecs {
            let dot: f64 = v.iter().zip(prev.iter()).map(|(&a, &b)| a * b).sum();
            for (i, p) in prev.iter().enumerate() {
                v[i] -= dot * p;
            }
        }
        
        // Normalize
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in &mut v {
                *x /= norm;
            }
        }
        
        // Power iteration
        for _ in 0..50 {
            // Compute centered * v
            let mut proj = vec![0.0; n_samples];
            for i in 0..n_samples {
                for j in 0..n_features {
                    proj[i] += centered[[i, j]] * v[j];
                }
            }
            
            // Compute centered.T * proj
            let mut new_v = vec![0.0; n_features];
            for j in 0..n_features {
                for i in 0..n_samples {
                    new_v[j] += centered[[i, j]] * proj[i];
                }
            }
            
            // Orthogonalize
            for prev in &used_vecs {
                let dot: f64 = new_v.iter().zip(prev.iter()).map(|(&a, &b)| a * b).sum();
                for (i, p) in prev.iter().enumerate() {
                    new_v[i] -= dot * p;
                }
            }
            
            // Normalize
            let norm: f64 = new_v.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 1e-10 {
                for x in &mut new_v {
                    *x /= norm;
                }
            }
            
            v = new_v;
        }
        
        used_vecs.push(v.clone());
        
        // Project data onto this component
        for i in 0..n_samples {
            let mut proj = 0.0;
            for j in 0..n_features {
                proj += centered[[i, j]] * v[j];
            }
            result[[i, comp]] = proj;
        }
    }
    
    result
}

/// Simple neighbor-based projection to 2D
fn neighbor_projection(data: &Array2<f64>, n_neighbors: usize) -> Vec<(f32, f32)> {
    let n_samples = data.nrows();
    let n_features = data.ncols();
    
    if n_samples <= 2 {
        return (0..n_samples)
            .map(|i| (i as f32, 0.0))
            .collect();
    }
    
    // Compute pairwise distances
    let mut distances: Vec<Vec<f64>> = vec![vec![0.0; n_samples]; n_samples];
    for i in 0..n_samples {
        for j in i+1..n_samples {
            let mut dist = 0.0;
            for k in 0..n_features {
                let diff = data[[i, k]] - data[[j, k]];
                dist += diff * diff;
            }
            dist = dist.sqrt();
            distances[i][j] = dist;
            distances[j][i] = dist;
        }
    }
    
    // Initialize 2D positions using first two principal components if available
    let mut positions: Vec<(f64, f64)> = (0..n_samples)
        .map(|i| {
            let x = if n_features > 0 { data[[i, 0]] } else { 0.0 };
            let y = if n_features > 1 { data[[i, 1]] } else { 0.0 };
            (x, y)
        })
        .collect();
    
    // Force-directed layout refinement
    let k_neighbors = n_neighbors.min(n_samples - 1);
    
    for _ in 0..100 {
        let mut forces: Vec<(f64, f64)> = vec![(0.0, 0.0); n_samples];
        
        for i in 0..n_samples {
            // Find k nearest neighbors
            let mut neighbor_dists: Vec<(usize, f64)> = (0..n_samples)
                .filter(|&j| j != i)
                .map(|j| (j, distances[i][j]))
                .collect();
            neighbor_dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            
            // Attractive forces from neighbors
            for &(j, hd_dist) in neighbor_dists.iter().take(k_neighbors) {
                let dx = positions[j].0 - positions[i].0;
                let dy = positions[j].1 - positions[i].1;
                let ld_dist = (dx * dx + dy * dy).sqrt().max(0.01);
                
                // Pull towards neighbors proportional to high-D distance
                let force = (hd_dist - ld_dist) * 0.01;
                forces[i].0 += dx / ld_dist * force;
                forces[i].1 += dy / ld_dist * force;
            }
            
            // Repulsive forces from non-neighbors
            for &(j, _) in neighbor_dists.iter().skip(k_neighbors) {
                let dx = positions[j].0 - positions[i].0;
                let dy = positions[j].1 - positions[i].1;
                let ld_dist = (dx * dx + dy * dy).sqrt().max(0.01);
                
                // Push away from non-neighbors
                let force = -0.001 / (ld_dist * ld_dist);
                forces[i].0 += dx / ld_dist * force;
                forces[i].1 += dy / ld_dist * force;
            }
        }
        
        // Apply forces
        for i in 0..n_samples {
            positions[i].0 += forces[i].0;
            positions[i].1 += forces[i].1;
        }
    }
    
    // Normalize to [-1, 1] range
    let (min_x, max_x) = positions.iter()
        .fold((f64::MAX, f64::MIN), |(min, max), &(x, _)| (min.min(x), max.max(x)));
    let (min_y, max_y) = positions.iter()
        .fold((f64::MAX, f64::MIN), |(min, max), &(_, y)| (min.min(y), max.max(y)));
    
    let range_x = (max_x - min_x).max(0.001);
    let range_y = (max_y - min_y).max(0.001);
    
    positions.iter()
        .map(|&(x, y)| {
            let nx = ((x - min_x) / range_x * 2.0 - 1.0) as f32;
            let ny = ((y - min_y) / range_y * 2.0 - 1.0) as f32;
            (nx, ny)
        })
        .collect()
}
