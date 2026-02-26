// Similarity scoring: cosine similarity with melodic and harmonic modes

use super::features::{ChunkFeatures, ChunkedFileFeatures, HarmonicFeatures, MelodicFeatures};
use rayon::prelude::*;

pub enum SimilarityMode {
    Melodic,
    Harmonic,
}

// --- Precomputed L2 norms ---

struct MelodicNorms {
    interval_bigrams: f32,
    contour_trigrams: f32,
    interval_histogram: f32,
    pitch_class_histogram: f32,
}

struct HarmonicNorms {
    chroma: f32,
    pc_transitions: f32,
}

struct ChunkNorms {
    melodic: Option<MelodicNorms>,
    harmonic: Option<HarmonicNorms>,
}

fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn compute_chunk_norms(chunk: &ChunkFeatures) -> ChunkNorms {
    ChunkNorms {
        melodic: chunk.melodic.as_ref().map(|m| MelodicNorms {
            interval_bigrams: l2_norm(&m.interval_bigrams),
            contour_trigrams: l2_norm(&m.contour_trigrams),
            interval_histogram: l2_norm(&m.interval_histogram),
            pitch_class_histogram: l2_norm(&m.pitch_class_histogram),
        }),
        harmonic: chunk.harmonic.as_ref().map(|h| HarmonicNorms {
            chroma: l2_norm(&h.chroma),
            pc_transitions: l2_norm(&h.pc_transitions),
        }),
    }
}

// --- Cosine similarity ---

/// Cosine similarity with precomputed L2 norms — only computes the dot product.
fn cosine_prenormed(a: &[f32], b: &[f32], norm_a: f32, norm_b: f32) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let denom = norm_a * norm_b;
    if denom > 0.0 {
        (dot / denom).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Melodic scoring — weighted cosine, transposition-invariant via intervals.
fn melodic_similarity(
    a: &MelodicFeatures,
    b: &MelodicFeatures,
    na: &MelodicNorms,
    nb: &MelodicNorms,
) -> f32 {
    0.4 * cosine_prenormed(&a.interval_bigrams, &b.interval_bigrams, na.interval_bigrams, nb.interval_bigrams)
        + 0.3 * cosine_prenormed(&a.contour_trigrams, &b.contour_trigrams, na.contour_trigrams, nb.contour_trigrams)
        + 0.2 * cosine_prenormed(&a.interval_histogram, &b.interval_histogram, na.interval_histogram, nb.interval_histogram)
        + 0.1 * cosine_prenormed(&a.pitch_class_histogram, &b.pitch_class_histogram, na.pitch_class_histogram, nb.pitch_class_histogram)
}

/// Harmonic scoring — transposition-invariant via circular chroma shift.
fn harmonic_similarity(
    a: &HarmonicFeatures,
    b: &HarmonicFeatures,
    na: &HarmonicNorms,
    nb: &HarmonicNorms,
) -> f32 {
    if a.chroma.len() != 12 || b.chroma.len() != 12 {
        return 0.0;
    }

    // Circular shift preserves L2 norm, so na.chroma is valid for all shifts
    let mut best_chroma_sim = 0.0f32;
    for shift in 0..12 {
        let shifted = circular_shift_12(&a.chroma, shift);
        let sim = cosine_prenormed(&shifted, &b.chroma, na.chroma, nb.chroma);
        if sim > best_chroma_sim {
            best_chroma_sim = sim;
        }
    }

    0.6 * best_chroma_sim
        + 0.4 * cosine_prenormed(
            &a.pc_transitions,
            &b.pc_transitions,
            na.pc_transitions,
            nb.pc_transitions,
        )
}

fn circular_shift_12(chroma: &[f32], shift: usize) -> Vec<f32> {
    let mut result = vec![0.0f32; 12];
    for i in 0..12 {
        result[(i + shift) % 12] = chroma[i];
    }
    result
}

// ---- Chunk-aware scoring ----

pub struct ChunkSimilarityResult {
    pub file_id: String,
    pub score: f32,
    pub match_offset_secs: f32,
}

/// Find best matching chunk pair between two chunked files.
/// Returns (best_score, candidate_chunk_offset_secs).
fn best_chunk_pair_score(
    target: &ChunkedFileFeatures,
    target_norms: &[ChunkNorms],
    candidate: &ChunkedFileFeatures,
    candidate_norms: &[ChunkNorms],
    mode: &SimilarityMode,
) -> (f32, f32) {
    let mut best_score = 0.0f32;
    let mut best_offset = 0.0f32;

    for (tc, tn) in target.chunks.iter().zip(target_norms.iter()) {
        for (cc, cn) in candidate.chunks.iter().zip(candidate_norms.iter()) {
            let score = match mode {
                SimilarityMode::Melodic => match (&tc.melodic, &cc.melodic, &tn.melodic, &cn.melodic)
                {
                    (Some(a), Some(b), Some(na), Some(nb)) => melodic_similarity(a, b, na, nb),
                    _ => 0.0,
                },
                SimilarityMode::Harmonic => {
                    match (&tc.harmonic, &cc.harmonic, &tn.harmonic, &cn.harmonic) {
                        (Some(a), Some(b), Some(na), Some(nb)) => {
                            harmonic_similarity(a, b, na, nb)
                        }
                        _ => 0.0,
                    }
                }
            };
            if score > best_score {
                best_score = score;
                best_offset = cc.offset_secs;
            }
        }
    }

    (best_score, best_offset)
}

/// Find the most similar files using chunk-based comparison.
pub fn find_most_similar_chunked(
    target_id: &str,
    all_files: &[(String, ChunkedFileFeatures)],
    mode: SimilarityMode,
    max_results: usize,
    threshold: f32,
) -> Vec<ChunkSimilarityResult> {
    // Precompute L2 norms for all chunks across all files (parallel)
    let all_norms: Vec<Vec<ChunkNorms>> = all_files
        .par_iter()
        .map(|(_, features)| features.chunks.iter().map(compute_chunk_norms).collect())
        .collect();

    let target_idx = match all_files.iter().position(|(id, _)| id == target_id) {
        Some(idx) => idx,
        None => return Vec::new(),
    };

    let target = &all_files[target_idx].1;
    let target_norms = &all_norms[target_idx];

    // Compare target against all other files (parallel)
    let mut scores: Vec<ChunkSimilarityResult> = all_files
        .par_iter()
        .enumerate()
        .filter(|(_, (id, _))| id != target_id)
        .filter_map(|(i, (id, features))| {
            let (score, offset) =
                best_chunk_pair_score(target, target_norms, features, &all_norms[i], &mode);
            if score >= threshold {
                Some(ChunkSimilarityResult {
                    file_id: id.clone(),
                    score,
                    match_offset_secs: offset,
                })
            } else {
                None
            }
        })
        .collect();

    scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(max_results);
    scores
}
