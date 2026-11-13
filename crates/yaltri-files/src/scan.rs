//! Full and partial filesystem walks that fill a `FileIndex`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use jwalk::WalkDir;

use crate::index::{EntryId, FileIndex};
use crate::scope::Scope;

/// Progress and cancellation shared with the scanning thread.
#[derive(Default)]
pub struct ScanControl {
    pub found: AtomicUsize,
    pub cancel: AtomicBool,
}

/// Builds a fresh index of every root in `scope`.
pub fn full_scan(scope: &Scope, control: &ScanControl) -> FileIndex {
    let mut index = FileIndex::new();
    for root in &scope.roots {
        if control.cancel.load(Ordering::Relaxed) {
            break;
        }
        if !root.is_dir() {
            tracing::warn!(root = %root.display(), "index root missing, skipped");
            continue;
        }
        let id = index.add_root(root);
        walk_into(&mut index, id, root, scope, control);
    }
    index.shrink_to_fit();
    index
}

/// Recursively adds everything below `dir` (already present as `dir_id`).
pub fn walk_into(index: &mut FileIndex, dir_id: EntryId, dir: &Path, scope: &Scope, control: &ScanControl) {
    let filter = Arc::new(scope.clone());
    let walker = WalkDir::new(dir)
        .skip_hidden(false)
        .follow_links(false)
        .sort(false)
        .min_depth(1)
        .process_read_dir(move |_, _, _, children| {
            children.retain(|child| match child {
                Ok(e) => !(e.file_type.is_dir() && filter.excludes(&e.path())),
                Err(_) => false,
            });
        });

    // jwalk yields depth-first, parents before children; `stack[d]` is the
    // most recent directory seen at depth `d` (depth 0 = `dir` itself).
    let mut stack: Vec<EntryId> = vec![dir_id];
    for (n, entry) in walker.into_iter().enumerate() {
        if n % 4096 == 0 && control.cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(entry) = entry else { continue };
        let depth = entry.depth;
        if depth == 0 || depth > stack.len() {
            continue;
        }
        let parent = stack[depth - 1];
        let name = entry.file_name.to_string_lossy();
        // Symlinks/junctions are indexed as files and never followed.
        let is_dir = entry.file_type.is_dir();
        let id = index.push(parent, &name, is_dir);
        control.found.fetch_add(1, Ordering::Relaxed);
        if is_dir {
            stack.truncate(depth);
            stack.push(id);
        }
    }
}

/// Re-reads one directory's immediate children and reconciles them with the
/// index: unchanged names keep their ids (so their subtrees stay valid), gone
/// names are tombstoned, new names are added and new folders walked fully.
pub fn rescan_dir(index: &mut FileIndex, dir_id: EntryId, dir: &Path, scope: &Scope) {
    let mut existing: HashMap<String, EntryId> = HashMap::new();
    for (i, e) in index.entries.iter().enumerate() {
        if e.parent == dir_id && !e.is_deleted() && !e.is_root() {
            existing.insert(index.name(i as EntryId).to_owned(), i as EntryId);
        }
    }

    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => {
            // Folder vanished or became unreadable: drop it and its subtree.
            index.mark_deleted(dir_id);
            return;
        }
    };
    let control = ScanControl::default();
    for child in read.flatten() {
        let name = child.file_name().to_string_lossy().into_owned();
        let Ok(ft) = child.file_type() else { continue };
        let is_dir = ft.is_dir();
        if is_dir && scope.excludes(&child.path()) {
            continue;
        }
        match existing.remove(&name) {
            Some(id) => {
                let was_dir = index.entries[id as usize].is_dir();
                if was_dir != is_dir {
                    index.mark_deleted(id);
                    let id = index.push(dir_id, &name, is_dir);
                    if is_dir {
                        walk_into(index, id, &child.path(), scope, &control);
                    }
                }
            }
            None => {
                let id = index.push(dir_id, &name, is_dir);
                if is_dir {
                    walk_into(index, id, &child.path(), scope, &control);
                }
            }
        }
    }
    for (_, id) in existing {
        index.mark_deleted(id);
    }
}

/// Resolves absolute directory paths to entry ids in one pass over the index.
pub fn resolve_dirs(index: &FileIndex, paths: &[&Path]) -> Vec<Option<EntryId>> {
    // (parent, folded name) -> id, for directories only.
    let mut dirs: HashMap<(EntryId, String), EntryId> = HashMap::new();
    let mut roots: Vec<(std::path::PathBuf, EntryId)> = Vec::new();
    for (i, e) in index.entries.iter().enumerate() {
        if e.is_deleted() || !e.is_dir() {
            continue;
        }
        let id = i as EntryId;
        if e.is_root() {
            roots.push((index.path_of(id).unwrap_or_default(), id));
        } else {
            dirs.insert((e.parent, key(index.name(id))), id);
        }
    }

    paths
        .iter()
        .map(|path| {
            // Longest root that contains the path.
            let (root_path, root_id) = roots
                .iter()
                .filter(|(r, _)| starts_with(path, r))
                .max_by_key(|(r, _)| r.as_os_str().len())?;
            let rel = strip_prefix(path, root_path)?;
            let mut cur = *root_id;
            for comp in rel.components() {
                cur = *dirs.get(&(cur, key(&comp.as_os_str().to_string_lossy())))?;
            }
            Some(cur)
        })
        .collect()
}

fn key(name: &str) -> String {
    if cfg!(windows) { name.to_lowercase() } else { name.to_owned() }
}

fn starts_with(path: &Path, prefix: &Path) -> bool {
    if cfg!(windows) {
        let (p, q) = (path.to_string_lossy().to_lowercase(), prefix.to_string_lossy().to_lowercase());
        Path::new(&p).starts_with(Path::new(&q))
    } else {
        path.starts_with(prefix)
    }
}

fn strip_prefix(path: &Path, prefix: &Path) -> Option<std::path::PathBuf> {
    let n = prefix.components().count();
    Some(path.components().skip(n).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("yaltri-scan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("a/one.txt"), "").unwrap();
        fs::write(root.join("a/b/two.txt"), "").unwrap();
        fs::write(root.join("node_modules/pkg/index.js"), "").unwrap();
        root
    }

    fn scope(root: &Path) -> Scope {
        Scope::for_test(vec![root.to_path_buf()], vec![], &["node_modules"])
    }

    fn find(index: &FileIndex, q: &str) -> Vec<PathBuf> {
        index.search(q, 10, &|_| 0).into_iter().map(|h| h.path).collect()
    }

    #[test]
    fn full_scan_builds_tree_and_honors_excludes() {
        let root = tree("full");
        let index = full_scan(&scope(&root), &ScanControl::default());
        assert_eq!(find(&index, "two.txt"), vec![root.join("a").join("b").join("two.txt")]);
        assert!(find(&index, "index.js").is_empty());
        // root + a + b + one + two
        assert_eq!(index.len(), 5);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rescan_reconciles_changes_and_keeps_subtrees() {
        let root = tree("rescan");
        let scope = scope(&root);
        let mut index = full_scan(&scope, &ScanControl::default());

        fs::remove_file(root.join("a/one.txt")).unwrap();
        fs::create_dir_all(root.join("a/new/deep")).unwrap();
        fs::write(root.join("a/new/deep/three.txt"), "").unwrap();

        let a = resolve_dirs(&index, &[&root.join("a")])[0].unwrap();
        rescan_dir(&mut index, a, &root.join("a"), &scope);

        assert!(find(&index, "one.txt").is_empty());
        assert_eq!(find(&index, "three").len(), 1);
        // b was untouched: its child must still resolve.
        assert_eq!(find(&index, "two.txt").len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rescan_of_vanished_dir_removes_subtree() {
        let root = tree("vanish");
        let scope = scope(&root);
        let mut index = full_scan(&scope, &ScanControl::default());
        let b = resolve_dirs(&index, &[&root.join("a/b")])[0].unwrap();
        fs::remove_dir_all(root.join("a/b")).unwrap();
        rescan_dir(&mut index, b, &root.join("a/b"), &scope);
        assert!(find(&index, "two.txt").is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_unknown_dir_is_none() {
        let root = tree("resolve");
        let index = full_scan(&scope(&root), &ScanControl::default());
        let got = resolve_dirs(&index, &[&root.join("a"), &root.join("nope"), Path::new("/elsewhere")]);
        assert!(got[0].is_some());
        assert_eq!(&got[1..], &[None, None]);
        fs::remove_dir_all(root).unwrap();
    }
}
