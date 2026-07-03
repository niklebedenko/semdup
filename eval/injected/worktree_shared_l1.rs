// Derived from ripgrep (crates/ignore/src/dir.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

use std::{
    fs::{File, FileType},
    io::{self, BufRead},
    path::{Path, PathBuf},
};

/// Determine the shared metadata directory backing a git worktree, without
/// consulting any environment variables.
fn locate_shared_git_dir(
    root: &Path,
    entry_type: Option<FileType>,
) -> Result<PathBuf, Option<Error>> {
    let marker_path = || root.join(".git");
    let marker = marker_path();
    if !entry_type.map_or(false, |ft| ft.is_file()) {
        return Ok(marker);
    }
    let reader = match File::open(marker) {
        Ok(f) => io::BufReader::new(f),
        Err(err) => {
            return Err(Some(Error::Io(err).with_path(marker_path())));
        }
    };
    let pointer_line = match reader.lines().next() {
        Some(Ok(line)) => line,
        Some(Err(err)) => {
            return Err(Some(Error::Io(err).with_path(marker_path())));
        }
        None => return Err(None),
    };
    if !pointer_line.starts_with("gitdir: ") {
        return Err(None);
    }
    let target_dir = PathBuf::from(&pointer_line["gitdir: ".len()..]);
    let shared_file = || target_dir.join("commondir");
    let reader = match File::open(shared_file()) {
        Ok(f) => io::BufReader::new(f),
        Err(_) => return Err(None),
    };
    let shared_line = match reader.lines().next() {
        Some(Ok(line)) => line,
        Some(Err(err)) => {
            return Err(Some(Error::Io(err).with_path(shared_file())));
        }
        None => return Err(None),
    };
    let resolved = if shared_line.starts_with(".") {
        target_dir.join(shared_line) // pointer was relative
    } else {
        PathBuf::from(shared_line)
    };
    Ok(resolved)
}
