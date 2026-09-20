use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::overrides::{Override, OverrideBuilder};

use crate::clock::Timestamp;
use crate::paths;
use crate::rank::Candidate;
use tracing::{debug, warn};

/// Depth walked under the current directory itself.
pub const CHILD_DEPTH: usize = 1;
/// How many ancestor levels above the current directory are climbed.
pub const ANCESTOR_LEVELS: usize = 1;

const EXCLUDED_NAMES: &[&str] = &["node_modules", "bin", "obj", ".git", "target"];

/// Discovers directories on disk when the database has none to offer
/// (SPEC section 11): children of `current_dir`, then of each ancestor.
pub fn discover(current_dir: &Path, respect_gitignore: bool) -> Vec<Candidate> {
    debug!(current = %current_dir.display(), respect_gitignore, "fallback walk");
    let mut found: Vec<PathBuf> = Vec::new();
    walk_into(&mut found, current_dir, CHILD_DEPTH, respect_gitignore);
    let mut ancestor = current_dir.to_path_buf();
    for _ in 0..ANCESTOR_LEVELS {
        let Some(parent) = ancestor.parent().map(Path::to_path_buf) else {
            break;
        };
        walk_into(&mut found, &parent, 1, respect_gitignore);
        ancestor = parent;
    }
    to_candidates(found, current_dir)
}

fn walk_into(found: &mut Vec<PathBuf>, root: &Path, depth: usize, respect_gitignore: bool) {
    let walker = WalkBuilder::new(root)
        .max_depth(Some(depth))
        .overrides(exclusion_overrides(root))
        .git_ignore(respect_gitignore)
        .git_global(respect_gitignore)
        .git_exclude(respect_gitignore)
        .ignore(respect_gitignore)
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(%error, "fallback walk skipped an unreadable path");
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            found.push(entry.path().to_path_buf());
        }
    }
}

fn exclusion_overrides(root: &Path) -> Override {
    let mut builder = OverrideBuilder::new(root);
    for name in EXCLUDED_NAMES {
        if builder.add(&format!("!{name}")).is_err() {
            warn!(name = %name, "exclusion override rejected; walking without exclusions");
            return Override::empty();
        }
    }
    match builder.build() {
        Ok(overrides) => overrides,
        Err(_) => {
            warn!("exclusion overrides could not be built; walking without exclusions");
            Override::empty()
        }
    }
}

fn to_candidates(found: Vec<PathBuf>, current_dir: &Path) -> Vec<Candidate> {
    let Ok(current) = paths::canonical(current_dir) else {
        warn!(
            current = %current_dir.display(),
            "the current directory does not canonicalize; no fallback candidates"
        );
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates = Vec::new();
    for path in found {
        let Ok(canonical) = paths::canonical(&path) else {
            warn!(path = %path.display(), "a discovered path does not canonicalize; skipped");
            continue;
        };
        if canonical.key == current.key || !seen.insert(canonical.key.clone()) {
            continue;
        }
        candidates.push(Candidate {
            path: canonical.path,
            name: canonical.name,
            folder: canonical.folder,
            last_visit: Timestamp::from_unix_seconds(0),
            missing: false,
        });
    }
    candidates
}

#[cfg(test)]
mod tests {
    // WHY: this whole module is layer-3-ish test code exercising real directories.
    #![allow(clippy::expect_used)]

    use super::discover;
    use assert_fs::TempDir;
    use std::path::{Path, PathBuf};

    fn make_dir(root: &Path, relative: &str) -> PathBuf {
        let target = root.join(relative);
        std::fs::create_dir_all(&target).expect("the fixture directory is created");
        target
    }

    #[test]
    fn finds_depth_one_children_of_the_current_directory() {
        let root = TempDir::new().expect("a fresh scratch root");
        let current = make_dir(root.path(), "parent/current");
        make_dir(root.path(), "parent/current/child");
        let found = discover(&current, true);
        let found_names: Vec<String> = found
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        assert!(found_names.contains(&"child".to_owned()), "{found_names:?}");
    }

    #[test]
    fn finds_depth_one_children_of_the_direct_parent() {
        let root = TempDir::new().expect("a fresh scratch root");
        let current = make_dir(root.path(), "parent/current");
        make_dir(root.path(), "parent/sibling");
        let found = discover(&current, true);
        let found_names: Vec<String> = found
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        assert!(
            found_names.contains(&"sibling".to_owned()),
            "{found_names:?}"
        );
    }

    #[test]
    fn a_grandparents_children_are_never_found() {
        let root = TempDir::new().expect("a fresh scratch root");
        let current = make_dir(root.path(), "parent/current");
        make_dir(root.path(), "unrelated_at_root");
        let found = discover(&current, true);
        let found_names: Vec<String> = found
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        assert!(
            !found_names.contains(&"unrelated_at_root".to_owned()),
            "{found_names:?}"
        );
    }

    #[test]
    fn default_exclusions_and_hidden_directories_are_filtered_without_any_gitignore() {
        let root = TempDir::new().expect("a fresh scratch root");
        let current = make_dir(root.path(), "current");
        for excluded in ["node_modules", "bin", "obj", ".git", "target", ".hidden"] {
            make_dir(&current, excluded);
        }
        make_dir(&current, "kept");
        let found = discover(&current, false);
        let found_names: Vec<String> = found
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        assert_eq!(found_names, vec!["kept".to_owned()]);
    }

    #[test]
    fn a_gitignored_directory_is_skipped_only_when_gitignore_is_respected() {
        let root = TempDir::new().expect("a fresh scratch root");
        let current = make_dir(root.path(), "current");
        make_dir(&current, "ignored");
        // WHY: gitignore semantics only apply inside a git working tree.
        make_dir(&current, ".git");
        std::fs::write(current.join(".gitignore"), "ignored\n")
            .expect("the fixture .gitignore is written");
        let respected = discover(&current, true);
        let respected_names: Vec<String> = respected
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        assert!(
            !respected_names.contains(&"ignored".to_owned()),
            "{respected_names:?}"
        );
        let ignored_off = discover(&current, false);
        let ignored_off_names: Vec<String> = ignored_off
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        assert!(
            ignored_off_names.contains(&"ignored".to_owned()),
            "{ignored_off_names:?}"
        );
    }

    #[test]
    fn the_current_directory_itself_is_never_in_the_result() {
        let root = TempDir::new().expect("a fresh scratch root");
        let current = make_dir(root.path(), "parent/current");
        let found = discover(&current, true);
        let current_key = current.file_name().expect("current has a name");
        assert!(
            found
                .iter()
                .all(|candidate| candidate.name != current_key.to_string_lossy()),
            "{found:?}"
        );
    }
}
