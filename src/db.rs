//! SQLite storage for extracted units, their source text, and cached
//! embeddings. Text and vector rows are keyed by content hash so changed files
//! only re-embed changed functions.

use std::path::Path;

use anyhow::{Result, ensure};
use rayon::prelude::*;
use rusqlite::Connection;

use crate::extract::{Unit, UnitKind};

pub type UnitEmbedding = (UnitRow, Vec<f32>);

const DB_USER_VERSION: i64 = 2;
const SQLITE_CACHE_KIB: i64 = -200_000;
const SQLITE_MMAP_BYTES: i64 = 256 * 1024 * 1024;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure_connection(&conn)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS units (
            id INTEGER PRIMARY KEY,
            corpus TEXT NOT NULL,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            lang TEXT NOT NULL,
            unit_kind TEXT NOT NULL DEFAULT 'function',
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
    migrate(&conn)?;
    Ok(conn)
}

fn configure_connection(conn: &Connection) -> Result<()> {
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    ensure!(
        mode.eq_ignore_ascii_case("wal"),
        "SQLite refused WAL journal mode, got {mode}"
    );
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", SQLITE_CACHE_KIB)?;
    conn.pragma_update(None, "mmap_size", SQLITE_MMAP_BYTES)?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < DB_USER_VERSION {
        if version < 1 {
            normalize_cached_embeddings(conn)?;
        }
        if version < 2 {
            add_unit_kind_column(conn)?;
        }
        conn.pragma_update(None, "user_version", DB_USER_VERSION)?;
    }
    Ok(())
}

fn add_unit_kind_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(units)")?;
    let columns = stmt.query_map([], |r| r.get::<_, String>(1))?;
    let has_unit_kind = columns
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|c| c == "unit_kind");
    if !has_unit_kind {
        conn.execute(
            "ALTER TABLE units ADD COLUMN unit_kind TEXT NOT NULL DEFAULT 'function'",
            [],
        )?;
    }
    Ok(())
}

fn normalize_cached_embeddings(conn: &Connection) -> Result<()> {
    let rows = {
        let mut stmt = conn.prepare("SELECT hash, model, vec FROM embeddings")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if rows.is_empty() {
        return Ok(());
    }
    eprintln!("normalizing {} cached embedding(s)", rows.len());
    let rows: Vec<(String, String, i64, Vec<u8>)> = rows
        .into_par_iter()
        .map(|(hash, model, blob)| {
            let mut vec = bytes_to_f32(&blob)?;
            normalize_vec(&mut vec);
            let blob = f32_to_bytes(&vec);
            Ok((hash, model, vec.len() as i64, blob))
        })
        .collect::<Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut update =
            tx.prepare("UPDATE embeddings SET dim = ?3, vec = ?4 WHERE hash = ?1 AND model = ?2")?;
        for (hash, model, dim, blob) in rows {
            update.execute(rusqlite::params![hash, model, dim, blob])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn replace_corpus(conn: &Connection, corpus: &str, units: &[Unit]) -> Result<()> {
    conn.execute("DELETE FROM units WHERE corpus = ?1", [corpus])?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins_unit = tx.prepare(
            "INSERT INTO units (corpus, path, name, lang, unit_kind, start_line, end_line, hash, ignored, is_test)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        let mut ins_text =
            tx.prepare("INSERT OR IGNORE INTO texts (hash, text) VALUES (?1, ?2)")?;
        for u in units {
            ins_unit.execute(rusqlite::params![
                corpus,
                u.path,
                u.name,
                u.lang,
                u.kind.as_str(),
                // usize lost its ToSql/FromSql impls in rusqlite 0.32
                // (platform-dependent width); go through i64 at the boundary.
                u.start_line as i64,
                u.end_line as i64,
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

pub fn pending_count(conn: &Connection, model: &str) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM texts t
         WHERE EXISTS (SELECT 1 FROM units u WHERE u.hash = t.hash)
           AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.hash = t.hash AND e.model = ?1)",
        [model],
        |r| r.get(0),
    )?;
    Ok(n as usize)
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
            let mut vec = vec.clone();
            normalize_vec(&mut vec);
            let blob = f32_to_bytes(&vec);
            ins.execute(rusqlite::params![hash, model, vec.len() as i64, blob])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Embedding for a single text hash, if cached.
pub fn embedding_for(conn: &Connection, model: &str, hash: &str) -> Result<Option<Vec<f32>>> {
    let mut stmt = conn.prepare("SELECT vec FROM embeddings WHERE hash = ?1 AND model = ?2")?;
    let mut rows = stmt.query([hash, model])?;
    match rows.next()? {
        Some(r) => {
            let blob: Vec<u8> = r.get(0)?;
            Ok(Some(bytes_to_f32(&blob)?))
        }
        None => Ok(None),
    }
}

#[derive(Clone)]
pub struct UnitRow {
    pub path: String,
    pub name: String,
    pub lang: String,
    pub kind: UnitKind,
    pub start_line: usize,
    pub end_line: usize,
    pub hash: String,
    pub ignored: bool,
    pub is_test: bool,
}

impl UnitRow {
    pub fn label(&self) -> String {
        let kind = match self.kind {
            UnitKind::Function => "",
            UnitKind::Block => " [block]",
        };
        format!(
            "{}:{}-{} {}{}",
            self.path, self.start_line, self.end_line, self.name, kind
        )
    }

    pub fn lines(&self) -> usize {
        self.end_line - self.start_line + 1
    }
}

/// Load units of a corpus together with their embedding for `model`.
/// Units whose text has no embedding are skipped.
pub fn load_units(conn: &Connection, corpus: &str, model: &str) -> Result<Vec<UnitEmbedding>> {
    let mut stmt = conn.prepare(
        "SELECT u.path, u.name, u.lang, u.unit_kind, u.start_line, u.end_line, u.hash, u.ignored, u.is_test, e.vec
         FROM units u JOIN embeddings e ON e.hash = u.hash AND e.model = ?1
         WHERE u.corpus = ?2",
    )?;
    let rows = stmt.query_map([model, corpus], |r| {
        let blob: Vec<u8> = r.get(9)?;
        Ok((
            UnitRow {
                path: r.get(0)?,
                name: r.get(1)?,
                lang: r.get(2)?,
                kind: parse_unit_kind(r.get::<_, String>(3)?)?,
                start_line: r.get::<_, i64>(4)? as usize,
                end_line: r.get::<_, i64>(5)? as usize,
                hash: r.get(6)?,
                ignored: r.get(7)?,
                is_test: r.get(8)?,
            },
            blob,
        ))
    })?;
    let rows: Vec<(UnitRow, Vec<u8>)> = rows.collect::<rusqlite::Result<_>>()?;
    rows.into_par_iter()
        .map(|(unit, blob)| {
            let vec = bytes_to_f32(&blob)?;
            Ok((unit, vec))
        })
        .collect::<Result<_>>()
}

pub fn load_scannable_units(
    conn: &Connection,
    corpus: &str,
    model: &str,
    min_lines: usize,
    skip_tests: bool,
    kind: Option<UnitKind>,
) -> Result<(Vec<UnitEmbedding>, usize)> {
    let kind = kind.map(UnitKind::as_str);
    let n_ignored: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM units u JOIN embeddings e ON e.hash = u.hash AND e.model = ?1
         WHERE u.corpus = ?2 AND u.ignored != 0
           AND (?3 IS NULL OR u.unit_kind = ?3)",
        rusqlite::params![model, corpus, kind],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT u.path, u.name, u.lang, u.unit_kind, u.start_line, u.end_line, u.hash, u.ignored, u.is_test, e.vec
         FROM units u JOIN embeddings e ON e.hash = u.hash AND e.model = ?1
         WHERE u.corpus = ?2
           AND u.ignored = 0
           AND (u.end_line - u.start_line + 1) >= ?3
           AND (?4 = 0 OR u.is_test = 0)
           AND (?5 IS NULL OR u.unit_kind = ?5)",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![model, corpus, min_lines as i64, skip_tests, kind],
        |r| {
            let blob: Vec<u8> = r.get(9)?;
            Ok((
                UnitRow {
                    path: r.get(0)?,
                    name: r.get(1)?,
                    lang: r.get(2)?,
                    kind: parse_unit_kind(r.get::<_, String>(3)?)?,
                    start_line: r.get::<_, i64>(4)? as usize,
                    end_line: r.get::<_, i64>(5)? as usize,
                    hash: r.get(6)?,
                    ignored: r.get(7)?,
                    is_test: r.get(8)?,
                },
                blob,
            ))
        },
    )?;
    let rows: Vec<(UnitRow, Vec<u8>)> = rows.collect::<rusqlite::Result<_>>()?;
    let units = rows
        .into_par_iter()
        .map(|(unit, blob)| Ok((unit, bytes_to_f32(&blob)?)))
        .collect::<Result<_>>()?;
    Ok((units, n_ignored as usize))
}

fn parse_unit_kind(kind: String) -> rusqlite::Result<UnitKind> {
    UnitKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown unit kind '{kind}'").into(),
        )
    })
}

pub fn text_for_hash(conn: &Connection, hash: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT text FROM texts WHERE hash = ?1")?;
    let mut rows = stmt.query([hash])?;
    Ok(match rows.next()? {
        Some(row) => Some(row.get(0)?),
        None => None,
    })
}

fn bytes_to_f32(blob: &[u8]) -> Result<Vec<f32>> {
    ensure!(
        blob.len().is_multiple_of(std::mem::size_of::<f32>()),
        "embedding blob has {} bytes, not a multiple of {}",
        blob.len(),
        std::mem::size_of::<f32>()
    );
    #[cfg(target_endian = "little")]
    {
        Ok(bytemuck::pod_collect_to_vec::<u8, f32>(blob))
    }
    #[cfg(not(target_endian = "little"))]
    {
        Ok(blob
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }
}

fn f32_to_bytes(vec: &[f32]) -> Vec<u8> {
    #[cfg(target_endian = "little")]
    {
        bytemuck::cast_slice(vec).to_vec()
    }
    #[cfg(not(target_endian = "little"))]
    {
        vec.iter().flat_map(|f| f.to_le_bytes()).collect()
    }
}

pub fn normalize_vec(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn print_status(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT corpus, lang, unit_kind, COUNT(*) FROM units GROUP BY corpus, lang, unit_kind",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (corpus, lang, kind, n): (String, String, String, i64) =
            (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        println!("units  {corpus:10} {lang:12} {kind:8} {n}");
    }
    let mut stmt = conn.prepare("SELECT model, COUNT(*) FROM embeddings GROUP BY model")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (model, n): (String, i64) = (r.get(0)?, r.get(1)?);
        println!("embeds {model} {n}");
    }
    Ok(())
}
