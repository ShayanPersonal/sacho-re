// Similarity scoring: cosine similarity with melodic and harmonic modes

use super::features::MidiFileFeatures;

pub enum SimilarityMode {
    Melodic,
    Harmonic,
}

/// Compute similarity between two MIDI files (0.0 = dissimilar, 1.0 = identical).
pub fn compute_similarity(a: &MidiFileFeatures, b: &MidiFileFeatures, mode: &SimilarityMode) -> f32 {
    match mode {
        SimilarityMode::Melodic => melodic_similarity(a, b),
        SimilarityMode::Harmonic => harmonic_similarity(a, b),
    }
}

/// Find the most similar files to a target, sorted by score descending.
pub fn find_most_similar(
    target_id: &str,
    all_files: &[(String, MidiFileFeatures)],
    mode: SimilarityMode,
    max_results: usize,
    threshold: f32,
) -> Vec<(String, f32)> {
    let target = match all_files.iter().find(|(id, _)| id == target_id) {
        Some((_, features)) => features,
        None => return Vec::new(),
    };

    let mut scores: Vec<(String, f32)> = all_files.iter()
        .filter(|(id, _)| id != target_id)
        .map(|(id, features)| {
            let score = compute_similarity(target, features, &mode);
            (id.clone(), score)
        })
        .filter(|(_, score)| *score >= threshold)
        .collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(max_results);
    scores
}

/// Melodic scoring — weighted cosine, ~90% transposition-invariant.
fn melodic_similarity(a: &MidiFileFeatures, b: &MidiFileFeatures) -> f32 {
    let (a_mel, b_mel) = match (&a.melodic, &b.melodic) {
        (Some(a), Some(b)) => (a, b),
        _ => return 0.0,
    };

    0.4 * cosine_similarity(&a_mel.interval_bigrams, &b_mel.interval_bigrams)
        + 0.3 * cosine_similarity(&a_mel.contour_trigrams, &b_mel.contour_trigrams)
        + 0.2 * cosine_similarity(&a_mel.interval_histogram, &b_mel.interval_histogram)
        + 0.1 * cosine_similarity(&a_mel.pitch_class_histogram, &b_mel.pitch_class_histogram)
}

/// Harmonic scoring — transposition-invariant via circular chroma shift.
fn harmonic_similarity(a: &MidiFileFeatures, b: &MidiFileFeatures) -> f32 {
    let (a_harm, b_harm) = match (&a.harmonic, &b.harmonic) {
        (Some(a), Some(b)) => (a, b),
        _ => return 0.0,
    };

    // Chroma must be exactly 12 bins for circular shift
    if a_harm.chroma.len() != 12 || b_harm.chroma.len() != 12 {
        return 0.0;
    }

    // Find best circular shift for chroma
    let mut best_chroma_sim = 0.0f32;
    for shift in 0..12 {
        let shifted = circular_shift_12(&a_harm.chroma, shift);
        let sim = cosine_similarity(&shifted, &b_harm.chroma);
        if sim > best_chroma_sim {
            best_chroma_sim = sim;
        }
    }

    0.6 * best_chroma_sim + 0.4 * cosine_similarity(&a_harm.pc_transitions, &b_harm.pc_transitions)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom > 0.0 {
        (dot / denom).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn circular_shift_12(chroma: &[f32], shift: usize) -> Vec<f32> {
    let mut result = vec![0.0f32; 12];
    for i in 0..12 {
        result[(i + shift) % 12] = chroma[i];
    }
    result
}
