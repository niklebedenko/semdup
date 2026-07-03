use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::extract::Unit;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS units (
            id INTEGER PRIMARY KEY,
            corpus TEXT NOT NULL,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            lang TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            hash TEXT NOT NULL,
            ignored INTEGER NOT NULL,
            is_test INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS units_corpus ON units(corpus);
        CREATE INDEX IF NOT EXISTS units_hash ON units(hash);
        CREATE TABLE IF NOT EXISTS texts (
            hash TEXT PRIMARY KEY,
            text TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS embeddings (
            hash TEXT NOT NULL,
            model TEXT NOT NULL,
            dim INTEGER NOT NULL,
            vec BLOB NOT NULL,
            PRIMARY KEY (hash, model)
        );",
    )?;
    Ok(conn)
}

pub fn replace_corpus(conn: &Connection, corpus: &str, units: &[Unit]) -> Result<()> {
    conn.execute("DELETE FROM units WHERE corpus = ?1", [corpus])?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins_unit = tx.prepare(
            "INSERT INTO units (corpus, path, name, lang, start_line, end_line, hash, ignored, is_test)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        let mut ins_text = tx.prepare("INSERT OR IGNORE INTO texts (hash, text) VALUES (?1, ?2)")?;
        for u in units {
            ins_unit.execute(rusqlite::params![
                corpus,
                u.path,
                u.name,
                u.lang,
                u.start_line,
                u.end_line,
                u.hash,
                u.ignored,
                u.is_test,
            ])?;
            ins_text.execute(rusqlite::params![u.hash, u.text])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// (hash, text) rows referenced by some unit but not yet embedded for `model`.
pub fn pending_texts(conn: &Connection, model: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT t.hash, t.text FROM texts t
         WHERE EXISTS (SELECT 1 FROM units u WHERE u.hash = t.hash)
           AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.hash = t.hash AND e.model = ?1)",
    )?;
    let rows = stmt.query_map([model], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn insert_embeddings(
    conn: &Connection,
    model: &str,
    rows: &[(String, Vec<f32>)],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT OR REPLACE INTO embeddings (hash, model, dim, vec) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (hash, vec) in rows {
            let blob: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
            ins.execute(rusqlite::params![hash, model, vec.len(), blob])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Embedding for a single text hash, if cached.
pub fn embedding_for(conn: &Connection, model: &str, hash: &str) -> Result<Option<Vec<f32>>> {
    let mut stmt =
        conn.prepare("SELECT vec FROM embeddings WHERE hash = ?1 AND model = ?2")?;
    let mut rows = stmt.query([hash, model])?;
    match rows.next()? {
        Some(r) => {
            let blob: Vec<u8> = r.get(0)?;
            let mut vec = bytes_to_f32(&blob);
            normalize(&mut vec);
            Ok(Some(vec))
        }
        None => Ok(None),
    }
}

#[derive(Clone)]
pub struct UnitRow {
    pub path: String,
    pub name: String,
    pub hash: String,
    pub start_line: usize,
    pub end_line: usize,
    pub ignored: bool,
    pub is_test: bool,
}

impl UnitRow {
    pub fn label(&self) -> String {
        format!(
            "{}:{}-{} {}",
            self.path, self.start_line, self.end_line, self.name
        )
    }

    pub fn lines(&self) -> usize {
        self.end_line - self.start_line + 1
    }
}

/// Load units of a corpus together with their embedding for `model`.
/// Units whose text has no embedding are skipped.
pub fn load_units(
    conn: &Connection,
    corpus: &str,
    model: &str,
) -> Result<Vec<(UnitRow, Vec<f32>)>> {
    let mut stmt = conn.prepare(
        "SELECT u.path, u.name, u.hash, u.start_line, u.end_line, u.ignored, u.is_test, e.vec
         FROM units u JOIN embeddings e ON e.hash = u.hash AND e.model = ?1
         WHERE u.corpus = ?2",
    )?;
    let rows = stmt.query_map([model, corpus], |r| {
        let blob: Vec<u8> = r.get(7)?;
        Ok((
            UnitRow {
                path: r.get(0)?,
                name: r.get(1)?,
                hash: r.get(2)?,
                start_line: r.get(3)?,
                end_line: r.get(4)?,
                ignored: r.get(5)?,
                is_test: r.get(6)?,
            },
            blob,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (unit, blob) = row?;
        let mut vec = bytes_to_f32(&blob);
        normalize(&mut vec);
        out.push((unit, vec));
    }
    Ok(out)
}

fn bytes_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn print_status(conn: &Connection) -> Result<()> {
    let mut stmt =
        conn.prepare("SELECT corpus, lang, COUNT(*) FROM units GROUP BY corpus, lang")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (corpus, lang, n): (String, String, usize) = (r.get(0)?, r.get(1)?, r.get(2)?);
        println!("units  {corpus:10} {lang:12} {n}");
    }
    let mut stmt = conn.prepare("SELECT model, COUNT(*) FROM embeddings GROUP BY model")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (model, n): (String, usize) = (r.get(0)?, r.get(1)?);
        println!("embeds {model} {n}");
    }
    Ok(())
}
