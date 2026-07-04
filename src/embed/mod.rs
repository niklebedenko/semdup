//! Embedding backends and the shared embed driver.
//!
//! A backend is a pure `texts -> unit-normalized vectors` function; everything
//! else (cache lookups, doc stripping, checkpointed DB writes) lives here so
//! all backends behave identically.

#[cfg(feature = "onnx")]
pub mod onnx;
pub mod sidecar;

use anyhow::Result;
use rusqlite::Connection;

use crate::db;

pub trait Backend {
    /// Embed each text; returns one vector per input, in order.
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// How many texts the driver should pass per `embed` call. In-process
    /// backends want small chunks (checkpointed DB writes); process-per-call
    /// backends want everything at once (one model load).
    fn preferred_chunk(&self) -> usize {
        256
    }
}

/// Embed every text referenced by some unit that lacks a vector for `model`.
/// Writes are checkpointed per chunk so an interrupted run resumes cheaply.
pub fn run(conn: &Connection, model: &str, backend: &mut dyn Backend) -> Result<()> {
    let pending = db::pending_texts(conn, model)?;
    if pending.is_empty() {
        eprintln!("nothing to embed for {model}");
        return Ok(());
    }
    eprintln!("embedding {} texts with {model}", pending.len());
    let mut done = 0usize;
    for chunk in pending.chunks(backend.preferred_chunk()) {
        let texts: Vec<String> = chunk
            .iter()
            .map(|(_, t)| {
                let stripped = strip_doc_comments(t);
                if stripped.trim().is_empty() {
                    t.clone()
                } else {
                    stripped
                }
            })
            .collect();
        let vecs = backend.embed(&texts)?;
        anyhow::ensure!(
            vecs.len() == chunk.len(),
            "backend returned {} vectors for {} texts",
            vecs.len(),
            chunk.len()
        );
        let rows: Vec<(String, Vec<f32>)> = chunk
            .iter()
            .zip(vecs)
            .map(|((h, _), v)| (h.clone(), v))
            .collect();
        db::insert_embeddings(conn, model, &rows)?;
        done += chunk.len();
        eprintln!("  {done}/{}", pending.len());
    }
    Ok(())
}

/// Strip doc comments (`///`, `//!`, `/** .. */`, `/*! .. */`) so shared doc
/// boilerplate doesn't inflate similarity. Non-doc comments are kept: they are
/// part of how the code reads.
pub fn strip_doc_comments(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let s = line.trim();
        if in_block {
            if s.contains("*/") {
                in_block = false;
            }
            continue;
        }
        if s.starts_with("/**") || s.starts_with("/*!") {
            if !s.contains("*/") {
                in_block = true;
            }
            continue;
        }
        if s.starts_with("///") || s.starts_with("//!") {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::strip_doc_comments;

    #[test]
    fn strips_doc_keeps_regular_comments() {
        let src = "/// doc\n//! inner\n/** block\n * more\n */\n// keep me\nfn f() {}\n/*! one-liner */\nlet x = 1;";
        let got = strip_doc_comments(src);
        assert_eq!(got, "// keep me\nfn f() {}\nlet x = 1;");
    }
}
