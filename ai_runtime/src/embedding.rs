//! A real (if intentionally simple) text embedding: the "hashing trick"
//! (feature hashing), a genuine, well-known technique for turning text
//! into a fixed-length numeric vector without training a model. This is
//! not a stand-in for a trained embedding model's semantic space — two
//! synonyms will *not* generally embed near each other — but it is a real,
//! deterministic, inspectable algorithm, not a placeholder that returns
//! zeros or random numbers.

const DEFAULT_DIMS: usize = 8;

/// Embeds `text` into an 8-dimensional vector using feature hashing: each
/// whitespace-separated token is hashed (FNV-1a) to pick a dimension and a
/// sign, and its contribution is accumulated into that dimension. The
/// result is L2-normalized so embeddings of texts with different lengths
/// are still comparable by cosine similarity.
pub fn embed(text: &str) -> Vec<f64> {
    embed_with_dims(text, DEFAULT_DIMS)
}

pub fn embed_with_dims(text: &str, dims: usize) -> Vec<f64> {
    let mut vector = vec![0.0f64; dims.max(1)];
    for token in text.split_whitespace() {
        let normalized = token.to_lowercase();
        let hash = fnv1a(&normalized);
        let index = (hash as usize) % vector.len();
        // Use a second, independent bit of the hash to decide the sign,
        // which is what keeps unrelated tokens from all pushing the same
        // dimension in the same direction (the standard hashing-trick
        // construction).
        let sign = if (hash >> 32) & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }

    let norm = vector.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 0.0 {
        for x in &mut vector {
            *x /= norm;
        }
    }
    vector
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Cosine similarity between two equal-length embedding vectors, provided
/// as a convenience for anything (tests, future stdlib functions) that
/// wants to compare embeddings without re-deriving the formula.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_is_deterministic() {
        assert_eq!(embed("hello world"), embed("hello world"));
    }

    #[test]
    fn embedding_has_expected_dimensionality() {
        assert_eq!(embed("hello world").len(), DEFAULT_DIMS);
    }

    #[test]
    fn embedding_is_l2_normalized() {
        let v = embed("the quick brown fox jumps over the lazy dog");
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9 || norm == 0.0);
    }

    #[test]
    fn identical_texts_have_cosine_similarity_one() {
        let a = embed("machine learning is fun");
        let b = embed("machine learning is fun");
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn different_texts_usually_produce_different_embeddings() {
        let a = embed("machine learning");
        let b = embed("banana bread recipe");
        assert_ne!(a, b);
    }

    #[test]
    fn empty_text_is_the_zero_vector() {
        assert_eq!(embed(""), vec![0.0; DEFAULT_DIMS]);
    }
}
