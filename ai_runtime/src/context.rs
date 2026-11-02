//! Intelligent Context Engine foundation: real, deterministic context
//! compression and chunk deduplication — the achievable slice of the
//! v2.0 brief's "automatic context compression, semantic chunking,
//! context deduplication, sliding windows, adaptive retrieval, context
//! prioritization, token optimization" for an offline-by-default runtime.
//!
//! What's real: word-count-based compression (genuinely shortens text,
//! deterministically) and exact/near-duplicate chunk removal (genuinely
//! removes duplicates). What's out of scope: anything requiring a real
//! LLM to judge semantic relevance (true "semantic chunking" beyond
//! `knowledge.rs`'s word-count chunking, importance-based prioritization)
//! — see `docs/roadmap.md`.

/// Compresses `text` to at most `max_words` words, appending `...` if
/// anything was cut. A real, simple, deterministic token-budget tool —
/// not semantic summarization (see `ailang-interpreter`'s `summarize` for
/// that).
pub fn compress_context(text: &str, max_words: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return text.trim().to_string();
    }
    format!("{}...", words[..max_words].join(" "))
}

/// Removes duplicate chunks (case-insensitive, whitespace-trimmed exact
/// match), keeping the first occurrence of each and preserving overall
/// order. Real deduplication; not fuzzy/semantic near-duplicate detection
/// (which would need embeddings-based clustering — a reasonable v2.1+
/// enhancement, not implemented here to keep this a fast, dependency-free
/// pass suitable for every call).
pub fn dedupe_chunks(chunks: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for chunk in chunks {
        let key = chunk.trim().to_lowercase();
        if seen.insert(key) {
            out.push(chunk.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_context_leaves_short_text_untouched() {
        assert_eq!(compress_context("short text here", 10), "short text here");
    }

    #[test]
    fn compress_context_truncates_long_text() {
        let text = "one two three four five six seven";
        assert_eq!(compress_context(text, 3), "one two three...");
    }

    #[test]
    fn dedupe_chunks_removes_exact_duplicates() {
        let chunks = vec![
            "hello".to_string(),
            "world".to_string(),
            "hello".to_string(),
        ];
        assert_eq!(
            dedupe_chunks(&chunks),
            vec!["hello".to_string(), "world".to_string()]
        );
    }

    #[test]
    fn dedupe_chunks_is_case_and_whitespace_insensitive() {
        let chunks = vec!["Hello".to_string(), " hello ".to_string()];
        assert_eq!(dedupe_chunks(&chunks), vec!["Hello".to_string()]);
    }

    #[test]
    fn dedupe_chunks_preserves_order() {
        let chunks = vec![
            "b".to_string(),
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ];
        assert_eq!(
            dedupe_chunks(&chunks),
            vec!["b".to_string(), "a".to_string(), "c".to_string()]
        );
    }
}
