use crate::clock::Timestamp;
use crate::storage::DirEntry;
use tracing::{debug, warn};

/// Injectable view of the filesystem (SPEC section 13), so reconciliation
/// never touches `std::fs` or `std::path` itself.
pub trait Filesystem {
    /// Whether `path` is a directory that exists right now.
    fn exists(&self, path: &str) -> bool;
}

/// The production filesystem, checking real directories on disk.
pub struct RealFilesystem;

impl Filesystem for RealFilesystem {
    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }
}

/// The outcome of one reconciliation pass over the stored entries.
pub struct Reconciled {
    /// Every entry with its `missing` flag brought up to date.
    pub entries: Vec<DirEntry>,
    /// `(dir id, new missing_since)` for each entry whose soft-delete state
    /// changed; the caller persists these via `storage::set_missing_since`.
    pub updates: Vec<(i64, Option<Timestamp>)>,
}

/// Compares every entry against `fs` and returns the entries with accurate
/// `missing` flags plus the storage writes that make the flags durable.
pub fn reconcile(entries: Vec<DirEntry>, fs: &dyn Filesystem, now: Timestamp) -> Reconciled {
    debug!(count = entries.len(), "soft-delete reconcile");
    let mut updates = Vec::new();
    let entries = entries
        .into_iter()
        .map(|mut entry| {
            let on_disk = fs.exists(&entry.path);
            match (on_disk, entry.missing) {
                (true, true) => {
                    entry.missing = false;
                    updates.push((entry.id, None));
                }
                (false, false) => {
                    entry.missing = true;
                    updates.push((entry.id, Some(now)));
                    warn!(path = %entry.path, "directory flagged missing");
                }
                (true, false) | (false, true) => {}
            }
            entry
        })
        .collect();
    Reconciled { entries, updates }
}

#[cfg(test)]
mod tests {
    use super::{Filesystem, reconcile};
    use crate::clock::Timestamp;
    use crate::storage::DirEntry;
    use std::collections::HashSet;

    struct FakeFilesystem(HashSet<String>);

    impl FakeFilesystem {
        fn of(paths: &[&str]) -> Self {
            Self(paths.iter().map(|path| (*path).to_owned()).collect())
        }
    }

    impl Filesystem for FakeFilesystem {
        fn exists(&self, path: &str) -> bool {
            self.0.contains(path)
        }
    }

    fn entry(id: i64, path: &str, missing: bool) -> DirEntry {
        DirEntry {
            id,
            path: path.to_owned(),
            last_visit: Timestamp::from_unix_seconds(1_700_000_000),
            missing,
        }
    }

    fn now() -> Timestamp {
        Timestamp::from_unix_seconds(1_800_000_000)
    }

    #[test]
    fn a_present_not_missing_entry_passes_through_untouched() {
        let fs = FakeFilesystem::of(&["c:\\dev\\tokio"]);
        let result = reconcile(vec![entry(7, "c:\\dev\\tokio", false)], &fs, now());
        assert_eq!(result.entries, vec![entry(7, "c:\\dev\\tokio", false)]);
        assert!(result.updates.is_empty());
    }

    #[test]
    fn an_absent_not_missing_entry_becomes_missing_stamped_with_now() {
        let fs = FakeFilesystem::of(&[]);
        let result = reconcile(vec![entry(7, "c:\\dev\\tokio", false)], &fs, now());
        assert_eq!(result.entries, vec![entry(7, "c:\\dev\\tokio", true)]);
        assert_eq!(result.updates, vec![(7, Some(now()))]);
    }

    #[test]
    fn a_present_missing_entry_is_reactivated_with_its_history_intact() {
        let fs = FakeFilesystem::of(&["c:\\dev\\tokio"]);
        let result = reconcile(vec![entry(7, "c:\\dev\\tokio", true)], &fs, now());
        assert_eq!(result.entries, vec![entry(7, "c:\\dev\\tokio", false)]);
        assert_eq!(result.updates, vec![(7, None)]);
    }

    #[test]
    fn an_absent_already_missing_entry_is_left_alone() {
        let fs = FakeFilesystem::of(&[]);
        let result = reconcile(vec![entry(7, "c:\\dev\\tokio", true)], &fs, now());
        assert_eq!(result.entries, vec![entry(7, "c:\\dev\\tokio", true)]);
        assert!(result.updates.is_empty());
    }

    #[test]
    fn a_batch_emits_updates_only_for_entries_that_changed_state() {
        let fs = FakeFilesystem::of(&["c:\\dev\\helix"]);
        let result = reconcile(
            vec![
                entry(1, "c:\\dev\\helix", false),
                entry(2, "c:\\dev\\helix", true),
                entry(3, "c:\\dev\\vapor", false),
                entry(4, "c:\\dev\\vapor", true),
            ],
            &fs,
            now(),
        );
        let flags: Vec<(i64, bool)> = result
            .entries
            .iter()
            .map(|entry| (entry.id, entry.missing))
            .collect();
        assert_eq!(flags, [(1, false), (2, false), (3, true), (4, true)]);
        assert_eq!(result.updates, vec![(2, None), (3, Some(now()))]);
    }

    #[test]
    fn an_empty_batch_produces_an_empty_result() {
        let fs = FakeFilesystem::of(&[]);
        let result = reconcile(Vec::new(), &fs, now());
        assert!(result.entries.is_empty());
        assert!(result.updates.is_empty());
    }
}
