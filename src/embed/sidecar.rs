//! External-process embedding backend.
//!
//! Protocol: JSON lines. We write `{"id": n, "text": "..."}` per input to the
//! child's stdin, close it, and read `{"id": n, "vec": [..]}` lines back from
//! stdout. The script owns model loading, batching, and device placement —
//! use it to trial models that have no ONNX export.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::Backend;

pub struct Sidecar {
    pub script: PathBuf,
    pub model: String,
    pub python: String,
}

#[derive(Serialize)]
struct Req<'a> {
    id: usize,
    text: &'a str,
}

#[derive(Deserialize)]
struct Resp {
    id: usize,
    vec: Vec<f32>,
}

impl Backend for Sidecar {
    /// One process per call: take everything at once so the model loads once.
    fn preferred_chunk(&self) -> usize {
        usize::MAX
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut child = Command::new(&self.python)
            .arg(&self.script)
            .args(["--model", &self.model])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning {} {}", self.python, self.script.display()))?;
        {
            let mut stdin = child.stdin.take().expect("piped stdin");
            for (id, text) in texts.iter().enumerate() {
                serde_json::to_writer(&mut stdin, &Req { id, text })?;
                stdin.write_all(b"\n")?;
            }
        } // drop closes stdin; the script sees EOF and starts embedding
        let stdout = child.stdout.take().expect("piped stdout");
        let mut out: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let resp: Resp =
                serde_json::from_str(&line).with_context(|| format!("bad sidecar line: {line}"))?;
            if resp.id >= out.len() {
                bail!("sidecar returned unknown id {}", resp.id);
            }
            out[resp.id] = Some(resp.vec);
        }
        let status = child.wait()?;
        if !status.success() {
            bail!("sidecar exited with {status}");
        }
        out.into_iter()
            .enumerate()
            .map(|(i, v)| v.with_context(|| format!("sidecar returned no vector for text {i}")))
            .collect()
    }
}
