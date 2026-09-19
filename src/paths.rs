use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Failure to resolve a user-supplied path to a directory on disk.
#[derive(Debug, Error)]
#[error("cannot resolve '{input}' to a directory on disk: {source}")]
pub struct PathError {
    input: String,
    source: io::Error,
}

/// The canonical description of a directory (SPEC section 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalDir {
    /// Canonical displayable path: no `\\?\` prefix, no trailing separator.
    pub path: String,
    /// The canonical path lowercased, used as the storage comparison key.
    pub key: String,
    /// Last path segment; empty for a drive root.
    pub name: String,
    /// Everything before the last segment; `None` for a drive root.
    pub folder: Option<String>,
}

/// The name/folder split of a path string, derived without disk access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPath {
    /// Last path segment; empty for a root.
    pub name: String,
    /// Parent path; `None` when the path has no parent.
    pub folder: Option<String>,
}

/// Resolves a user-supplied path, either separator accepted, relative
/// paths anchored at `base`, into the canonical form it names.
pub fn resolve(input: &str, base: &Path) -> Result<CanonicalDir, PathError> {
    let unified = unify_separators(input);
    let path = Path::new(&unified);
    let joined = if path.is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };
    canonical(&joined)
}

/// Canonicalizes an existing absolute path with `dunce` (junctions and
/// symlinks resolved) and derives its key, name, and folder.
pub fn canonical(path: &Path) -> Result<CanonicalDir, PathError> {
    let raw = dunce::canonicalize(path).map_err(|source| PathError {
        input: path.display().to_string(),
        source,
    })?;
    let text = strip_trailing_separator(raw.to_string_lossy().into_owned());
    let split = split(&text);
    Ok(CanonicalDir {
        key: text.to_lowercase(),
        path: text,
        name: split.name,
        folder: split.folder,
    })
}

/// Splits a path string into its last segment and its parent through
/// `std::path` parsing, never manual slicing.
pub fn split(path: &str) -> SplitPath {
    let as_path = Path::new(path);
    SplitPath {
        name: as_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder: as_path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned()),
    }
}

fn unify_separators(input: &str) -> String {
    input.replace('/', "\\")
}

fn strip_trailing_separator(text: String) -> String {
    let mut stripped = text;
    // WHY: length 3 keeps a drive root like `c:\` from losing its separator.
    while stripped.len() > 3 && stripped.ends_with('\\') {
        stripped.pop();
    }
    stripped
}

#[cfg(test)]
mod tests {
    use super::{SplitPath, canonical, resolve, split, strip_trailing_separator, unify_separators};
    use assert_fs::TempDir;
    use std::path::PathBuf;

    fn scratch(name: &str) -> (TempDir, PathBuf) {
        let root = TempDir::new().expect("a fresh scratch root");
        let child = root.path().join(name);
        std::fs::create_dir(&child).expect("the scratch child directory exists");
        (root, child)
    }

    #[test]
    fn slash_and_backslash_inputs_resolve_to_one_canonical_form() {
        let (root, child) = scratch("tokio");
        let with_backslashes =
            resolve(&child.to_string_lossy(), root.path()).expect("the native input resolves");
        let forward = child.to_string_lossy().replace('\\', "/");
        let with_slashes =
            resolve(&forward, root.path()).expect("the forward-slash input resolves");
        assert_eq!(with_backslashes, with_slashes);
        let on_disk = dunce::canonicalize(&child).expect("the child canonicalizes");
        assert_eq!(with_backslashes.path, on_disk.to_string_lossy());
    }

    #[test]
    fn a_relative_input_resolves_against_its_base() {
        let (root, child) = scratch("tokio");
        let absolute =
            resolve(&child.to_string_lossy(), root.path()).expect("the absolute input resolves");
        assert_eq!(
            resolve("tokio", root.path()).expect("the relative input resolves"),
            absolute
        );
        assert_eq!(
            resolve("..\\tokio", &child).expect("the parent-relative input resolves"),
            absolute
        );
    }

    #[test]
    fn case_variants_share_one_key_and_one_on_disk_casing() {
        let (root, child) = scratch("Tokyo");
        let on_disk = dunce::canonicalize(&child).expect("the child canonicalizes");
        let lower = resolve(&child.to_string_lossy().to_lowercase(), root.path())
            .expect("the lowercase input resolves");
        let upper = resolve(&child.to_string_lossy().to_uppercase(), root.path())
            .expect("the uppercase input resolves");
        assert_eq!(lower.key, upper.key);
        assert_eq!(lower.path, on_disk.to_string_lossy());
        assert_eq!(upper.path, on_disk.to_string_lossy());
        assert_eq!(lower.name, "Tokyo");
        let folder = on_disk
            .parent()
            .expect("a non-root child has a parent")
            .to_string_lossy()
            .into_owned();
        assert_eq!(lower.folder.as_deref(), Some(folder.as_str()));
    }

    #[test]
    fn split_derives_name_and_folder_from_the_last_segment() {
        assert_eq!(
            split("c:\\dev\\nested\\deep"),
            SplitPath {
                name: "deep".to_owned(),
                folder: Some("c:\\dev\\nested".to_owned()),
            }
        );
        assert_eq!(
            split("c:\\dev"),
            SplitPath {
                name: "dev".to_owned(),
                folder: Some("c:\\".to_owned()),
            }
        );
        assert_eq!(
            split("c:\\"),
            SplitPath {
                name: String::new(),
                folder: None,
            }
        );
    }

    #[test]
    fn canonical_paths_never_keep_a_trailing_separator() {
        let (root, child) = scratch("tokio");
        let resolved = resolve("tokio\\", root.path()).expect("the trailing input resolves");
        assert!(!resolved.path.ends_with('\\'));
        assert_eq!(
            resolved.path,
            dunce::canonicalize(&child)
                .expect("the child canonicalizes")
                .to_string_lossy()
        );
    }

    #[test]
    fn strip_trailing_separator_keeps_a_drive_root_intact() {
        assert_eq!(strip_trailing_separator("c:\\dev\\".to_owned()), "c:\\dev");
        assert_eq!(strip_trailing_separator("c:\\".to_owned()), "c:\\");
        assert_eq!(strip_trailing_separator("c:\\dev".to_owned()), "c:\\dev");
    }

    #[test]
    fn forward_slashes_become_backslashes_before_resolution() {
        assert_eq!(unify_separators("C:/dev/x/"), "C:\\dev\\x\\");
        assert_eq!(unify_separators("C:\\dev\\x"), "C:\\dev\\x");
    }

    #[test]
    fn resolving_a_missing_directory_is_an_error() {
        let (root, _child) = scratch("tokio");
        let missing = root.path().join("nope");
        let outcome = resolve(&missing.to_string_lossy(), root.path());
        let message = outcome
            .expect_err("a missing directory cannot resolve")
            .to_string();
        assert!(message.contains("nope"));
    }

    #[test]
    fn canonicalizing_a_missing_path_is_an_error() {
        let (root, _child) = scratch("tokio");
        let missing = root.path().join("nope");
        assert!(canonical(&missing).is_err());
    }
}
