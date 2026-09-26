use std::path::Path;

/// Injectable view of the `.git` entries that mark a project root
/// (SPEC section 19), following the model of `soft_delete::Filesystem`.
pub trait GitMarker {
    /// Whether `dir` itself holds a `.git` entry, directory or file.
    fn has_git_entry(&self, dir: &str) -> bool;
}

/// The production marker: true for a `.git` directory or file, so worktrees
/// and submodules count as project roots.
pub struct RealGitMarker;

impl GitMarker for RealGitMarker {
    fn has_git_entry(&self, dir: &str) -> bool {
        Path::new(dir).join(".git").exists()
    }
}

/// Returns the nearest of `current` and its ancestors (`Path::parent`,
/// up to the drive root) that holds a `.git` entry, or `None`.
pub fn root(current: &str, marker: &impl GitMarker) -> Option<String> {
    let mut dir = Path::new(current);
    loop {
        if marker.has_git_entry(&dir.to_string_lossy()) {
            return Some(dir.to_string_lossy().into_owned());
        }
        dir = dir.parent()?;
    }
}

/// Whether the lowercased key `key` is `root_key` itself or one of its
/// descendants; a `root_key` ending in `\` (a drive root) needs no separator.
pub fn within(key: &str, root_key: &str) -> bool {
    if root_key.ends_with('\\') {
        return key.starts_with(root_key);
    }
    match key.strip_prefix(root_key) {
        Some(rest) => rest.is_empty() || rest.starts_with('\\'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{GitMarker, RealGitMarker, root, within};
    use assert_fs::TempDir;
    use std::collections::HashSet;

    struct FakeMarker(HashSet<String>);

    impl FakeMarker {
        fn of(paths: &[&str]) -> Self {
            Self(paths.iter().map(|path| (*path).to_owned()).collect())
        }
    }

    impl GitMarker for FakeMarker {
        fn has_git_entry(&self, dir: &str) -> bool {
            self.0.contains(dir)
        }
    }

    #[test]
    fn the_current_directory_is_the_root_when_it_holds_git() {
        let marker = FakeMarker::of(&["c:\\dev\\furet"]);
        assert_eq!(
            root("c:\\dev\\furet", &marker),
            Some("c:\\dev\\furet".to_owned())
        );
    }

    #[test]
    fn the_nearest_ancestor_with_git_wins() {
        let marker = FakeMarker::of(&["c:\\", "c:\\dev"]);
        assert_eq!(
            root("c:\\dev\\furet\\src", &marker),
            Some("c:\\dev".to_owned())
        );
    }

    #[test]
    fn no_git_anywhere_yields_none() {
        let marker = FakeMarker::of(&[]);
        assert_eq!(root("c:\\dev\\furet", &marker), None);
    }

    #[test]
    fn a_drive_root_can_be_the_project_root() {
        let marker = FakeMarker::of(&["c:\\"]);
        assert_eq!(root("c:\\a\\b", &marker), Some("c:\\".to_owned()));
        assert!(within("c:\\x", "c:\\"));
    }

    #[test]
    fn within_rejects_a_sibling_sharing_a_prefix() {
        assert!(!within("c:\\dev\\furet2", "c:\\dev\\furet"));
        assert!(within("c:\\dev\\furet", "c:\\dev\\furet"));
        assert!(within("c:\\dev\\furet\\src", "c:\\dev\\furet"));
    }

    #[test]
    fn the_real_marker_accepts_a_git_directory_and_a_git_file() {
        let scratch = TempDir::new().expect("a fresh scratch root");
        let repo = scratch.path().join("repo");
        std::fs::create_dir_all(&repo).expect("the repo directory exists");
        std::fs::create_dir(repo.join(".git")).expect("the .git directory exists");
        assert!(RealGitMarker.has_git_entry(&repo.to_string_lossy()));
        std::fs::remove_dir_all(repo.join(".git")).expect("the .git directory is removed");
        assert!(!RealGitMarker.has_git_entry(&repo.to_string_lossy()));
        std::fs::write(repo.join(".git"), "gitdir: ../elsewhere\n")
            .expect("the .git file is written");
        assert!(RealGitMarker.has_git_entry(&repo.to_string_lossy()));
    }
}
