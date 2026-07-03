// Derived from ripgrep (crates/ignore/src/dir.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

use std::{
    fs::{File, FileType},
    io::{self, BufRead},
    path::{Path, PathBuf},
};

enum ReadFail {
    Open(io::Error),
    Read(io::Error),
    Empty,
}

/// First line of the file at `path`, distinguishing open/read/empty failures.
fn head_line(path: &Path) -> Result<String, ReadFail> {
    let file = File::open(path).map_err(ReadFail::Open)?;
    match io::BufReader::new(file).lines().next() {
        Some(Ok(line)) => Ok(line),
        Some(Err(e)) => Err(ReadFail::Read(e)),
        None => Err(ReadFail::Empty),
    }
}

/// Follow a worktree's `.git` pointer file to the shared metadata root that
/// may hold `info/exclude`.
fn shared_metadata_root(
    worktree: &Path,
    entry_kind: Option<FileType>,
) -> Result<PathBuf, Option<Error>> {
    let dotgit = worktree.join(".git");
    if !entry_kind.map_or(false, |k| k.is_file()) {
        return Ok(dotgit);
    }
    let pointer = match head_line(&dotgit) {
        Ok(line) => line,
        Err(ReadFail::Open(e)) | Err(ReadFail::Read(e)) => {
            return Err(Some(Error::Io(e).with_path(worktree.join(".git"))));
        }
        Err(ReadFail::Empty) => return Err(None),
    };
    let gitdir = match pointer.strip_prefix("gitdir: ") {
        Some(rest) => PathBuf::from(rest),
        None => return Err(None),
    };
    let link = gitdir.join("commondir");
    let shared = match head_line(&link) {
        Ok(line) => line,
        Err(ReadFail::Read(e)) => {
            return Err(Some(Error::Io(e).with_path(link)));
        }
        Err(ReadFail::Open(_)) | Err(ReadFail::Empty) => return Err(None),
    };
    if shared.starts_with('.') {
        Ok(gitdir.join(shared))
    } else {
        Ok(PathBuf::from(shared))
    }
}
