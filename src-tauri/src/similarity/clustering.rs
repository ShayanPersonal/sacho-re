// Clustering for similarity groups

use crate::session::SimilarityCoords;

/// Cluster assignment result
#[derive(Debug, Clone)]
pub struct ClusterResult {
    pub labels: Vec<Option<i32>>,
    pub n_clusters: usize,
}

/// Simple density-based clustering (simplified DBSCAN)
/// 
/// For production, consider using linfa-clustering's DBSCAN or HDBSCAN
pub fn cluster_points(coords: &[SimilarityCoords], eps: f32, min_samples: usize) -> ClusterResult {
    let n = coords.len();
    
    if n == 0 {
        return ClusterResult { labels: Vec::new(), n_clusters: 0 };
    }
    
    if n < min_samples {
        return ClusterResult { 
            labels: vec![None; n], 
            n_clusters: 0 
        };
    }
    
    // Compute pairwise distances
    let distances: Vec<Vec<f32>> = coords.iter()
        .map(|a| {
            coords.iter()
                .map(|b| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt())
                .collect()
        })
        .collect();
    
    // Find core points (points with >= min_samples neighbors within eps)
    let core_points: Vec<bool> = (0..n)
        .map(|i| {
            distances[i].iter().filter(|&&d| d <= eps).count() >= min_samples
        })
        .collect();
    
    let mut labels: Vec<Option<i32>> = vec![None; n];
    let mut current_cluster: i32 = 0;
    
    for i in 0..n {
        if labels[i].is_some() || !core_points[i] {
            continue;
        }
        
        // Start a new cluster
        let mut stack = vec![i];
        
        while let Some(current) = stack.pop() {
            if labels[current].is_some() {
                continue;
            }
            
            labels[current] = Some(current_cluster);
            
            // Find neighbors within eps
            for (j, &dist) in distances[current].iter().enumerate() {
                if dist <= eps && labels[j].is_none() {
                    if core_points[j] {
                        stack.push(j);
                    } else {
                        // Border point - assign to cluster but don't expand
                        labels[j] = Some(current_cluster);
                    }
                }
            }
        }
        
        current_cluster += 1;
    }
    
    ClusterResult {
        labels,
        n_clusters: current_cluster as usize,
    }
}

/// Automatically determine good clustering parameters based on data
pub fn auto_cluster(coords: &[SimilarityCoords]) -> ClusterResult {
    if coords.len() < 5 {
        return ClusterResult {
            labels: vec![None; coords.len()],
            n_clusters: 0,
        };
    }
    
    // Compute average nearest neighbor distance
    let mut avg_nn_dist = 0.0;
    for (i, a) in coords.iter().enumerate() {
        let mut min_dist = f32::MAX;
        for (j, b) in coords.iter().enumerate() {
            if i != j {
                let dist = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
                min_dist = min_dist.min(dist);
            }
        }
        avg_nn_dist += min_dist;
    }
    avg_nn_dist /= coords.len() as f32;
    
    // Use 2x average NN distance as eps
    let eps = avg_nn_dist * 2.0;
    
    // Min samples scales with data size
    let min_samples = (coords.len() / 20).max(3).min(10);
    
    cluster_points(coords, eps, min_samples)
}
