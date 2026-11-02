//! Native RAG (retrieval-augmented generation) foundation: real file
//! ingestion and real embedding-based retrieval over a `knowledge`
//! source directory.
//!
//! ## What's real
//!
//! Reading files from `source`, chunking them, embedding each chunk with
//! [`crate::embed`] (the real feature-hashing embedding — see that
//! module), and ranking chunks by cosine similarity against a query's
//! embedding are all real, deterministic, and exercised end-to-end by
//! this module's tests against real temp-directory files. What's a
//! placeholder: the embedding itself is the same simple hashing-trick
//! vector as everywhere else in this codebase, not a trained retrieval
//! model's embedding space — see `embedding.rs`'s doc comment for why
//! that's an honest, useful default rather than a stand-in.

use std::fs;
use std::path::Path;

use crate::embedding::{cosine_similarity, embed};

/// One retrieved chunk: its source file, its text, and its similarity
/// score against the query (higher is more relevant; range depends on
/// `embed`'s L2-normalization, roughly `[-1.0, 1.0]`).
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    pub source_file: String,
    pub text: String,
    pub score: f64,
}

/// Splits `text` into roughly `chunk_size`-word chunks (a simple, real
/// chunking strategy — no sentence-boundary awareness, which is a
/// reasonable v1.0 scope cut; see `docs/roadmap.md`).
fn chunk_text(text: &str, chunk_size: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![];
    }
    words
        .chunks(chunk_size.max(1))
        .map(|w| w.join(" "))
        .collect()
}

/// Reads every regular file directly inside `source_dir` (non-recursive —
/// see `docs/roadmap.md` for recursive ingestion as a follow-up), chunks
/// each into `chunk_size`-word pieces, and returns `(filename, chunk_text)`
/// pairs for every chunk across every file. Silently skips files it can't
/// read as UTF-8 text (binary files, permission errors) rather than
/// failing the whole ingestion — a `knowledge` source is expected to be a
/// directory of text/markdown files, and one bad file shouldn't block
/// retrieval over the rest.
pub fn ingest_directory(source_dir: &Path, chunk_size: usize) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(source_dir) else {
        return vec![];
    };
    let mut chunks = Vec::new();
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort(); // deterministic ordering regardless of filesystem iteration order
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for chunk in chunk_text(&content, chunk_size) {
            chunks.push((filename.clone(), chunk));
        }
    }
    chunks
}

/// Ingests `source_dir` and returns the top `k` chunks most similar to
/// `query`, ranked by cosine similarity of their real feature-hashing
/// embeddings, highest first.
pub fn retrieve(
    source_dir: &Path,
    query: &str,
    k: usize,
    chunk_size: usize,
) -> Vec<RetrievedChunk> {
    let chunks = ingest_directory(source_dir, chunk_size);
    let query_embedding = embed(query);

    let mut scored: Vec<RetrievedChunk> = chunks
        .into_iter()
        .map(|(source_file, text)| {
            let score = cosine_similarity(&query_embedding, &embed(&text));
            RetrievedChunk {
                source_file,
                text,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ailang-knowledge-test-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn chunk_text_splits_by_word_count() {
        let chunks = chunk_text("one two three four five", 2);
        assert_eq!(chunks, vec!["one two", "three four", "five"]);
    }

    #[test]
    fn chunk_text_handles_empty_input() {
        assert!(chunk_text("", 5).is_empty());
        assert!(chunk_text("   ", 5).is_empty());
    }

    #[test]
    fn ingest_directory_reads_real_files() {
        let dir = temp_dir("ingest");
        fs::write(
            dir.join("a.txt"),
            "the quick brown fox jumps over the lazy dog",
        )
        .unwrap();
        fs::write(dir.join("b.txt"), "rust is a systems programming language").unwrap();

        let chunks = ingest_directory(&dir, 100);
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().any(|(f, _)| f == "a.txt"));
        assert!(chunks.iter().any(|(f, _)| f == "b.txt"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ingest_directory_returns_empty_for_nonexistent_directory() {
        let chunks = ingest_directory(Path::new("/nonexistent/path/xyz"), 100);
        assert!(chunks.is_empty());
    }

    #[test]
    fn retrieve_ranks_the_most_relevant_chunk_first() {
        let dir = temp_dir("retrieve");
        fs::write(
            dir.join("animals.txt"),
            "cats and dogs are common household pets that many families love",
        )
        .unwrap();
        fs::write(
            dir.join("programming.txt"),
            "rust python and javascript are popular programming languages for software",
        )
        .unwrap();

        let results = retrieve(&dir, "programming languages software rust python", 2, 100);
        assert!(!results.is_empty());
        assert_eq!(results[0].source_file, "programming.txt");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn retrieve_respects_the_k_limit() {
        let dir = temp_dir("limit");
        for i in 0..5 {
            fs::write(
                dir.join(format!("doc{i}.txt")),
                format!("document number {i} content here"),
            )
            .unwrap();
        }
        let results = retrieve(&dir, "document content", 3, 100);
        assert_eq!(results.len(), 3);
        fs::remove_dir_all(&dir).ok();
    }
}
