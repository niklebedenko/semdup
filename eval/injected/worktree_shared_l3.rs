// Derived from ripgrep (crates/ignore/src/dir.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Given a directory and the file type of its `.git` entry, find the
// git "common directory" that may contain the private ignore file
// `info/exclude`. When `.git` is not a regular file, the answer is simply
// `<dir>/.git`. Otherwise `.git` is a worktree pointer whose first line must
// read `gitdir: <path>`; the file `<path>/commondir` then names the common
// directory on its first line, taken relative to `<path>` when it starts
// with a dot and as-is otherwise. Failures reading the pointer file are
// reported as I/O errors; a malformed pointer or unusable commondir file
// yields an empty failure. Environment variables are never consulted.

use std::{
    fs::FileType,
    path::{Path, PathBuf},
};

fn worktree_common_root(
    base: &Path,
    kind: Option<FileType>,
) -> Result<PathBuf, Option<Error>> {
    let pointer_path = base.join(".git");
    if !kind.map_or(false, |k| k.is_file()) {
        return Ok(pointer_path);
    }
    let raw = match std::fs::read_to_string(&pointer_path) {
        Ok(s) => s,
        Err(e) => return Err(Some(Error::Io(e).with_path(pointer_path))),
    };
    let first = raw.lines().next().unwrap_or("");
    let gitdir = match first.strip_prefix("gitdir: ") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => return Err(None),
    };
    let listing = match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(s) => s,
        Err(_) => return Err(None),
    };
    let target = match listing.lines().next() {
        Some(line) if !line.is_empty() => line,
        _ => return Err(None),
    };
    Ok(if target.starts_with('.') {
        gitdir.join(target)
    } else {
        PathBuf::from(target)
    })
}
