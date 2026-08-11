//! A private scratch directory, removed when it goes out of scope.
//!
//! Rendering a page and running an OCR binary both need a real path on disk
//! to hand to a subprocess. Rather than pull in a dependency for it, this is
//! the small subset of `tempfile` those two call sites use.

use std::fs::DirBuilder;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};

/// A directory that is deleted, with everything in it, on drop.
pub(super) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create a new directory under the system temp directory, readable only
    /// by the current user on Unix.
    ///
    /// `create_dir` fails when the path already exists, and never follows a
    /// symlink to create the target elsewhere, so a name an attacker guessed
    /// and pre-created is an error here rather than a directory we would go
    /// on to write into. That leaves guessing as a denial of service, which
    /// is what the 64 bits of entropy in the name are for: [`RandomState`]
    /// is seeded from the operating system, so the next name is not
    /// derivable from a name already on disk.
    pub(super) fn new(prefix: &str) -> io::Result<TempDir> {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let mut last_err = None;

        for _ in 0..16 {
            let entropy = RandomState::new().build_hasher().finish();
            let path = base.join(format!("{prefix}-{pid}-{entropy:016x}"));

            match dir_builder().create(&path) {
                Ok(()) => return Ok(TempDir { path }),
                Err(e) => last_err = Some(e),
            }
        }

        Err(last_err.unwrap_or_else(|| io::Error::other("no unique temp directory name")))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

/// A builder that creates the directory readable only by its owner.
///
/// Split by target rather than gated inline: the permission bits are the
/// only mutation, so a `let mut` covering both arms is an `unused_mut` on
/// every non-Unix target, and CI builds wasm with `-D warnings`.
#[cfg(unix)]
fn dir_builder() -> DirBuilder {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    builder
}

#[cfg(not(unix))]
fn dir_builder() -> DirBuilder {
    DirBuilder::new()
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Cleanup is best-effort: a failure here would mask whatever result
        // the caller is already returning.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temp_dir_exists_while_alive_and_is_gone_after_drop() {
        let path = {
            let dir = TempDir::new("anydoc-test").unwrap();
            let path = dir.path().to_path_buf();
            assert!(path.is_dir());
            std::fs::write(path.join("scratch"), b"content").unwrap();
            path
        };

        assert!(!path.exists());
    }

    #[test]
    fn two_temp_dirs_never_share_a_path() {
        let a = TempDir::new("anydoc-test").unwrap();
        let b = TempDir::new("anydoc-test").unwrap();

        assert_ne!(a.path(), b.path());
    }
}
