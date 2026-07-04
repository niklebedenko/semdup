//! Baseline files: accepted duplicate pairs, keyed by content hash so entries
//! survive file moves and renames but expire the moment either function's
//! body changes.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::db::UnitRow;

#[derive(Serialize, Deserialize, Default)]
pub struct Baseline {
    /// Sorted (hash_a <= hash_b) content-hash pairs.
    pub pairs: Vec<(String, String)>,
}

pub fn pair_key(a: &UnitRow, b: &UnitRow) -> (String, String) {
    if a.hash <= b.hash {
        (a.hash.clone(), b.hash.clone())
    } else {
        (b.hash.clone(), a.hash.clone())
    }
}

impl Baseline {
    pub fn load(path: &Path) -> Result<Baseline> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading baseline {}", path.display()))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut sorted: Vec<(String, String)> = self.set().into_iter().collect();
        sorted.sort();
        let out = Baseline { pairs: sorted };
        std::fs::write(path, serde_json::to_string_pretty(&out)?)?;
        Ok(())
    }

    /// Membership set with each pair normalized to sorted order, so files
    /// written by hand (or by older versions) still match.
    pub fn set(&self) -> HashSet<(String, String)> {
        self.pairs
            .iter()
            .map(|(a, b)| {
                if a <= b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(hash: &str) -> UnitRow {
        UnitRow {
            path: "p".into(),
            name: "n".into(),
            hash: hash.into(),
            start_line: 1,
            end_line: 10,
            ignored: false,
            is_test: false,
        }
    }

    #[test]
    fn key_is_order_independent() {
        assert_eq!(
            pair_key(&unit("b"), &unit("a")),
            pair_key(&unit("a"), &unit("b"))
        );
    }

    #[test]
    fn roundtrip_and_membership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        let bl = Baseline {
            pairs: vec![("b".into(), "a".into()), ("a".into(), "b".into())],
        };
        bl.save(&path).unwrap();
        let loaded = Baseline::load(&path).unwrap();
        // save normalizes each pair to sorted order and dedups
        assert_eq!(loaded.pairs, vec![("a".to_string(), "b".to_string())]);
        let set = loaded.set();
        assert!(set.contains(&pair_key(&unit("a"), &unit("b"))));
        assert!(set.contains(&pair_key(&unit("b"), &unit("a"))));
        assert!(!set.contains(&pair_key(&unit("a"), &unit("c"))));
    }
}
