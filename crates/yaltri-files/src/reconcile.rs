//! Full rescan without building a second index.
//!
//! The walk compares the filesystem with the existing index (read access
//! only, so searches keep working) and produces a short list of edits, which
//! are then applied under a brief write lock. Unchanged entries keep their
//! ids. Extra memory is one sorted (parent, child) table plus the edits,
//! instead of a complete second copy of the index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use crate::index::{EntryId, FileIndex};
use crate::scan::{ScanControl, Speed, walker};
use crate::scope::Scope;

/// Where a new entry hangs: an entry already in the index, or one added by
/// an earlier edit in the same batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    Existing(EntryId),
    New(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    AddRoot(PathBuf),
    Add { parent: Parent, name: String, is_dir: bool },
    Delete(EntryId),
}

/// Live children grouped by parent: a sorted `(parent, child)` table.
struct Children(Vec<(EntryId, EntryId)>);

impl Children {
    fn build(index: &FileIndex) -> Self {
        // Sized exactly up front: collecting a filter would grow by doubling.
        let mut pairs: Vec<(EntryId, EntryId)> = Vec::with_capacity(index.len());
        pairs.extend(
            index
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.is_deleted() && !e.is_root())
                .map(|(i, e)| (e.parent, i as EntryId)),
        );
        pairs.sort_unstable();
        Self(pairs)
    }

    fn of(&self, parent: EntryId) -> impl Iterator<Item = EntryId> + '_ {
        let start = self.0.partition_point(|(p, _)| *p < parent);
        self.0[start..].iter().take_while(move |(p, _)| *p == parent).map(|(_, c)| *c)
    }
}

/// One open directory during the walk.
struct Open<'a> {
    node: Parent,
    /// Existing children not seen on disk yet; whatever remains is deleted.
    unseen: HashMap<&'a str, EntryId>,
}

impl<'a> Open<'a> {
    fn new(node: Parent, index: &'a FileIndex, children: &Children, edits: &mut Vec<Edit>) -> Self {
        let mut unseen = HashMap::new();
        if let Parent::Existing(id) = node {
            for child in children.of(id) {
                // A duplicate name (should not happen) is dropped here.
                if let Some(dup) = unseen.insert(index.name(child), child) {
                    edits.push(Edit::Delete(dup));
                }
            }
        }
        Self { node, unseen }
    }

    fn close(self, edits: &mut Vec<Edit>) {
        edits.extend(self.unseen.into_values().map(Edit::Delete));
    }
}

/// Walks every root in `scope` and returns the edits that make `index` match
/// the disk. `None` if cancelled.
pub fn diff(index: &FileIndex, scope: &Scope, control: &ScanControl) -> Option<Vec<Edit>> {
    let children = Children::build(index);
    let mut edits = Vec::new();

    let mut existing_roots: Vec<(PathBuf, EntryId)> =
        index.roots().map(|id| (index.path_of(id).unwrap_or_default(), id)).collect();

    for root in &scope.roots {
        if !root.is_dir() {
            tracing::warn!(root = %root.display(), "index root missing, skipped");
            continue;
        }
        let node = match existing_roots.iter().position(|(p, _)| p == root) {
            Some(i) => Parent::Existing(existing_roots.swap_remove(i).1),
            None => {
                edits.push(Edit::AddRoot(root.clone()));
                Parent::New(edits.len() - 1)
            }
        };
        if !diff_tree(index, &children, node, root, scope, control, &mut edits) {
            return None;
        }
    }
    // Roots no longer in scope.
    edits.extend(existing_roots.into_iter().map(|(_, id)| Edit::Delete(id)));
    Some(edits)
}

fn diff_tree<'a>(
    index: &'a FileIndex,
    children: &Children,
    root: Parent,
    dir: &Path,
    scope: &Scope,
    control: &ScanControl,
    edits: &mut Vec<Edit>,
) -> bool {
    // stack[d] is the open directory at depth d (0 = root).
    let mut stack: Vec<Open<'a>> = vec![Open::new(root, index, children, edits)];
    for (n, entry) in walker(dir, scope, Speed::Background).into_iter().enumerate() {
        if n % 4096 == 0 && control.cancel.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(entry) = entry else { continue };
        let depth = entry.depth;
        if depth == 0 || depth > stack.len() {
            continue;
        }
        // Leaving directories: everything they still list as unseen is gone.
        while stack.len() > depth {
            stack.pop().expect("non-empty").close(edits);
        }
        control.found.fetch_add(1, Ordering::Relaxed);

        let name = entry.file_name.to_string_lossy();
        let is_dir = entry.file_type.is_dir();
        let parent = stack.last_mut().expect("depth >= 1");
        let kept = match parent.unseen.remove(name.as_ref()) {
            Some(id) if index.entries[id as usize].is_dir() == is_dir => Some(id),
            Some(id) => {
                // File became a folder or vice versa: replace it.
                edits.push(Edit::Delete(id));
                None
            }
            None => None,
        };
        let node = match kept {
            Some(id) => Parent::Existing(id),
            None => {
                edits.push(Edit::Add { parent: parent.node, name: name.into_owned(), is_dir });
                Parent::New(edits.len() - 1)
            }
        };
        if is_dir {
            stack.push(Open::new(node, index, children, edits));
        }
    }
    while let Some(open) = stack.pop() {
        open.close(edits);
    }
    true
}

/// Applies `edits` in order.
pub fn apply(index: &mut FileIndex, edits: Vec<Edit>) {
    // Id assigned to each edit that created an entry.
    let mut created: Vec<EntryId> = vec![EntryId::MAX; edits.len()];
    for (i, edit) in edits.into_iter().enumerate() {
        match edit {
            Edit::AddRoot(path) => created[i] = index.add_root(&path),
            Edit::Add { parent, name, is_dir } => {
                let parent = match parent {
                    Parent::Existing(id) => id,
                    Parent::New(j) => created[j],
                };
                created[i] = index.push(parent, &name, is_dir);
            }
            Edit::Delete(id) => index.mark_deleted(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::full_scan;
    use std::collections::BTreeSet;
    use std::fs;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("yaltri-reconcile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join("keep/deep")).unwrap();
        fs::write(root.join("a/one.txt"), "").unwrap();
        fs::write(root.join("a/b/two.txt"), "").unwrap();
        fs::write(root.join("keep/deep/stay.txt"), "").unwrap();
        fs::write(root.join("turns-dir"), "").unwrap();
        root
    }

    /// Every live path in the index.
    fn paths(index: &FileIndex) -> BTreeSet<PathBuf> {
        (0..index.entries.len() as EntryId).filter_map(|i| index.path_of(i)).collect()
    }

    fn id_of(index: &FileIndex, path: &Path) -> EntryId {
        (0..index.entries.len() as EntryId).find(|&i| index.path_of(i).as_deref() == Some(path)).unwrap()
    }

    #[test]
    fn matches_a_fresh_scan_after_changes_and_keeps_ids() {
        let root = tree("changes");
        let scope = Scope::for_test(vec![root.clone()], vec![], &[]);
        let control = ScanControl::default();
        let mut index = full_scan(&scope, &control);
        let stay = root.join("keep/deep/stay.txt");
        let stay_id = id_of(&index, &stay);

        fs::remove_file(root.join("a/one.txt")).unwrap();
        fs::remove_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join("new/inner")).unwrap();
        fs::write(root.join("new/inner/fresh.txt"), "").unwrap();
        fs::remove_file(root.join("turns-dir")).unwrap();
        fs::create_dir_all(root.join("turns-dir/child")).unwrap();

        let edits = diff(&index, &scope, &control).unwrap();
        apply(&mut index, edits);

        assert_eq!(paths(&index), paths(&full_scan(&scope, &control)));
        assert_eq!(id_of(&index, &stay), stay_id, "unchanged entries keep their ids");
        assert_eq!(index.search("fresh", 5, &|_| 0).len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_changes_means_no_edits() {
        let root = tree("same");
        let scope = Scope::for_test(vec![root.clone()], vec![], &[]);
        let control = ScanControl::default();
        let index = full_scan(&scope, &control);
        assert_eq!(diff(&index, &scope, &control).unwrap(), vec![]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn handles_added_and_removed_roots() {
        let (r1, r2) = (tree("root1"), tree("root2"));
        let control = ScanControl::default();
        let mut index = full_scan(&Scope::for_test(vec![r1.clone()], vec![], &[]), &control);
        let scope = Scope::for_test(vec![r2.clone()], vec![], &[]);
        let edits = diff(&index, &scope, &control).unwrap();
        apply(&mut index, edits);
        assert_eq!(paths(&index), paths(&full_scan(&scope, &control)));
        fs::remove_dir_all(r1).unwrap();
        fs::remove_dir_all(r2).unwrap();
    }
}
