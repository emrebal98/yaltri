//! Compact in-memory file index.
//!
//! Every entry stores only its own name and its parent's id; full paths are
//! rebuilt on demand. Names live in one byte arena (`names`) and a same-length
//! lowercase copy (`lower`), each name terminated by `\0`. A search is a single
//! `memmem` pass over `lower`; each hit's offset maps back to its entry by
//! binary search, because entries are appended in arena order.

use std::path::{Path, PathBuf};

use memchr::memmem;

pub type EntryId = u32;
pub const NO_PARENT: EntryId = EntryId::MAX;

const FLAG_DIR: u8 = 1;
const FLAG_DELETED: u8 = 2;
const FLAG_ROOT: u8 = 4;
/// Inside a folder people rarely search (dot-folders, AppData, caches,
/// system trees). Inherited by children; such results rank lower.
const FLAG_NOISE: u8 = 8;
const NOISE_PENALTY: u32 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name_off: u32,
    pub parent: EntryId,
    pub name_len: u16,
    pub flags: u8,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.flags & FLAG_DIR != 0
    }
    pub fn is_deleted(&self) -> bool {
        self.flags & FLAG_DELETED != 0
    }
    pub fn is_root(&self) -> bool {
        self.flags & FLAG_ROOT != 0
    }
    pub fn is_noise(&self) -> bool {
        self.flags & FLAG_NOISE != 0
    }
}

/// Grows by ~12.5% instead of Vec's default doubling: after a scan the arenas
/// are shrunk to size, and one new file must not double a 100 MiB arena.
fn grow<T>(v: &mut Vec<T>, additional: usize) {
    if v.capacity() - v.len() < additional {
        v.reserve_exact((v.capacity() / 8).max(additional).max(1024));
    }
}

/// Folders whose contents are mostly machine-generated.
fn is_noise_dir(name: &str, parent_is_root: bool) -> bool {
    if name.starts_with('.') || name.starts_with('$') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "appdata" | "programdata" | "cache" | "caches" | "temp" | "tmp" | "__pycache__") {
        return true;
    }
    // Linux system trees directly under `/`.
    parent_is_root
        && cfg!(not(windows))
        && matches!(lower.as_str(), "usr" | "lib" | "lib32" | "lib64" | "libx32" | "var" | "etc" | "boot" | "opt" | "srv" | "sbin" | "bin")
}

#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    pub(crate) names: Vec<u8>,
    pub(crate) lower: Vec<u8>,
    pub(crate) entries: Vec<Entry>,
    deleted: usize,
}

/// One search result.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: EntryId,
    pub path: PathBuf,
    pub is_dir: bool,
    pub score: u32,
}

/// Lowercases char by char, keeping a char unchanged when its lowercase form
/// has a different UTF-8 length (e.g. `İ`). Output length == input length, so
/// `lower` offsets equal `names` offsets.
pub fn fold_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mut lower = c.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(l), None) if l.len_utf8() == c.len_utf8() => out.push(l),
            _ => out.push(c),
        }
    }
    out
}

impl FileIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_parts(names: Vec<u8>, lower: Vec<u8>, entries: Vec<Entry>) -> Self {
        let deleted = entries.iter().filter(|e| e.is_deleted()).count();
        Self { names, lower, entries, deleted }
    }

    /// Live (not deleted) entries.
    pub fn len(&self) -> usize {
        self.entries.len() - self.deleted
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Fraction of entries that are tombstones; a full rebuild compacts them.
    pub fn garbage_ratio(&self) -> f32 {
        if self.entries.is_empty() { 0.0 } else { self.deleted as f32 / self.entries.len() as f32 }
    }

    /// Approximate heap usage in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.names.capacity() + self.lower.capacity() + self.entries.capacity() * size_of::<Entry>()
    }

    fn push_raw(&mut self, parent: EntryId, name: &str, flags: u8) -> EntryId {
        // Names longer than u16::MAX bytes do not exist on real filesystems.
        let name = &name[..name.len().min(u16::MAX as usize)];
        let id = self.entries.len() as EntryId;
        grow(&mut self.names, name.len() + 1);
        grow(&mut self.lower, name.len() + 1);
        grow(&mut self.entries, 1);
        let name_off = self.names.len() as u32;
        self.names.extend_from_slice(name.as_bytes());
        self.names.push(0);
        self.lower.extend_from_slice(fold_case(name).as_bytes());
        self.lower.push(0);
        self.entries.push(Entry { name_off, parent, name_len: name.len() as u16, flags });
        id
    }

    /// Adds a top-level folder (`C:\`, `/`, `/home/me`). Its "name" is the full path.
    pub fn add_root(&mut self, path: &Path) -> EntryId {
        self.push_raw(NO_PARENT, &path.to_string_lossy(), FLAG_ROOT | FLAG_DIR)
    }

    pub fn push(&mut self, parent: EntryId, name: &str, is_dir: bool) -> EntryId {
        let mut flags = if is_dir { FLAG_DIR } else { 0 };
        if let Some(p) = self.entries.get(parent as usize) {
            if p.is_noise() || (is_dir && is_noise_dir(name, p.is_root())) {
                flags |= FLAG_NOISE;
            }
        }
        self.push_raw(parent, name, flags)
    }

    /// Releases growth slack after a bulk build.
    pub fn shrink_to_fit(&mut self) {
        self.names.shrink_to_fit();
        self.lower.shrink_to_fit();
        self.entries.shrink_to_fit();
    }

    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.entries.get(id as usize)
    }

    pub fn name(&self, id: EntryId) -> &str {
        let e = &self.entries[id as usize];
        let bytes = &self.names[e.name_off as usize..e.name_off as usize + e.name_len as usize];
        // Names are pushed from &str, so this never fails.
        std::str::from_utf8(bytes).unwrap_or_default()
    }

    fn lower_name(&self, id: EntryId) -> &[u8] {
        let e = &self.entries[id as usize];
        &self.lower[e.name_off as usize..e.name_off as usize + e.name_len as usize]
    }

    pub fn mark_deleted(&mut self, id: EntryId) {
        let e = &mut self.entries[id as usize];
        if !e.is_deleted() {
            e.flags |= FLAG_DELETED;
            self.deleted += 1;
        }
    }

    pub fn set_dir(&mut self, id: EntryId, is_dir: bool) {
        let e = &mut self.entries[id as usize];
        if is_dir { e.flags |= FLAG_DIR } else { e.flags &= !FLAG_DIR }
    }

    /// Full path, or `None` if the entry or any ancestor was deleted.
    pub fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        let mut chain = Vec::with_capacity(16);
        let mut cur = id;
        loop {
            let e = self.entries.get(cur as usize)?;
            if e.is_deleted() {
                return None;
            }
            chain.push(cur);
            if e.is_root() {
                break;
            }
            cur = e.parent;
        }
        let mut path = PathBuf::new();
        for id in chain.iter().rev() {
            path.push(self.name(*id));
        }
        Some(path)
    }

    pub fn roots(&self) -> impl Iterator<Item = EntryId> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_root() && !e.is_deleted())
            .map(|(i, _)| i as EntryId)
    }

    /// Entry whose name starts at `offset` in the arena, or that contains it.
    fn entry_at_offset(&self, offset: usize) -> EntryId {
        let idx = self.entries.partition_point(|e| e.name_off as usize <= offset);
        (idx - 1) as EntryId
    }

    /// Finds entries whose name contains every whitespace-separated term
    /// (case-insensitive). Returns up to `limit` hits, best first. `boost`
    /// adds a bonus per path (usage history).
    pub fn search(&self, query: &str, limit: usize, boost: &dyn Fn(&Path) -> u32) -> Vec<Hit> {
        let folded = fold_case(query);
        let mut terms: Vec<&str> = folded.split_whitespace().collect();
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        // Scan with the longest term: fewest candidates.
        terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
        let first = terms[0].as_bytes();
        let rest: Vec<memmem::Finder<'_>> = terms[1..].iter().map(|t| memmem::Finder::new(t.as_bytes())).collect();

        let mut scored: Vec<(u32, EntryId)> = Vec::new();
        let mut last: Option<EntryId> = None;
        for offset in memmem::find_iter(&self.lower, first) {
            let id = self.entry_at_offset(offset);
            if last == Some(id) {
                continue;
            }
            last = Some(id);
            let e = &self.entries[id as usize];
            if e.is_deleted() || e.is_root() {
                continue;
            }
            let name = self.lower_name(id);
            if !rest.iter().all(|f| f.find(name).is_some()) {
                continue;
            }
            let mut score = name_score(name, first, offset - e.name_off as usize, e.is_dir());
            if e.is_noise() {
                score = score.saturating_sub(NOISE_PENALTY);
            }
            scored.push((score, id));
        }

        // Rank cheaply by name first, then resolve paths only for the best few.
        let shortlist = (limit * 8).max(64);
        if scored.len() > shortlist {
            scored.select_nth_unstable_by(shortlist, |a, b| b.0.cmp(&a.0));
            scored.truncate(shortlist);
        }
        let mut hits: Vec<Hit> = scored
            .into_iter()
            .filter_map(|(score, id)| {
                let path = self.path_of(id)?;
                let depth = path.components().count() as u32;
                let score = score.saturating_sub(depth.saturating_sub(2) * 4) + boost(&path);
                Some(Hit { id, is_dir: self.entries[id as usize].is_dir(), path, score })
            })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
        hits.truncate(limit);
        hits
    }
}

/// 100..=500 based on where and how well `term` matched inside `name`.
fn name_score(name: &[u8], term: &[u8], at: usize, is_dir: bool) -> u32 {
    let stem_len = name.iter().rposition(|&b| b == b'.').filter(|&p| p > 0).unwrap_or(name.len());
    let mut score = 100;
    if name == term || &name[..stem_len] == term {
        score += 300;
    } else if at == 0 {
        score += 200;
    } else if matches!(name[at - 1], b' ' | b'_' | b'-' | b'.' | b'(' | b'[') {
        score += 100;
    }
    // Prefer short names: "chrome.exe" over "chrome_proxy_helper_x64.dll".
    score += 64u32.saturating_sub(name.len() as u32);
    if is_dir {
        score += 10;
    }
    score.min(500)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FileIndex {
        let mut idx = FileIndex::new();
        let root = idx.add_root(Path::new("/r"));
        let docs = idx.push(root, "Documents", true);
        idx.push(docs, "Report Final.docx", false);
        idx.push(docs, "notes.txt", false);
        let proj = idx.push(root, "projects", true);
        let yaltri = idx.push(proj, "yaltri", true);
        idx.push(yaltri, "Cargo.toml", false);
        idx.push(yaltri, "README.md", false);
        idx.push(root, "Şarkılar", true);
        idx
    }

    fn no_boost(_: &Path) -> u32 {
        0
    }

    fn names(hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn builds_paths_from_parents() {
        let idx = sample();
        let hits = idx.search("cargo", 10, &no_boost);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, Path::new("/r/projects/yaltri/Cargo.toml"));
        assert!(!hits[0].is_dir);
    }

    #[test]
    fn case_insensitive_including_turkish() {
        let idx = sample();
        assert_eq!(names(&idx.search("REPORT", 10, &no_boost)), ["Report Final.docx"]);
        assert_eq!(names(&idx.search("şark", 10, &no_boost)), ["Şarkılar"]);
    }

    #[test]
    fn all_terms_must_match() {
        let idx = sample();
        assert_eq!(names(&idx.search("final report", 10, &no_boost)), ["Report Final.docx"]);
        assert!(idx.search("final zzz", 10, &no_boost).is_empty());
    }

    #[test]
    fn exact_and_prefix_matches_rank_first() {
        let mut idx = FileIndex::new();
        let root = idx.add_root(Path::new("/r"));
        idx.push(root, "my-yaltri-notes.txt", false);
        idx.push(root, "yaltri", true);
        idx.push(root, "yaltri.exe", false);
        let got = names(&idx.search("yaltri", 10, &no_boost));
        assert_eq!(got[2], "my-yaltri-notes.txt");
    }

    #[test]
    fn deleted_entries_and_their_children_disappear() {
        let mut idx = sample();
        let projects = (0..idx.entries.len() as u32).find(|&i| idx.name(i) == "projects").unwrap();
        idx.mark_deleted(projects);
        assert!(idx.search("cargo", 10, &no_boost).is_empty());
        assert_eq!(idx.len(), 8);
    }

    #[test]
    fn boost_reorders() {
        let mut idx = FileIndex::new();
        let root = idx.add_root(Path::new("/r"));
        idx.push(root, "a-file.txt", false);
        idx.push(root, "file.txt", false);
        let boost = |p: &Path| if p.ends_with("a-file.txt") { 400 } else { 0 };
        assert_eq!(names(&idx.search("file", 10, &boost))[0], "a-file.txt");
    }

    #[test]
    fn noisy_folders_rank_below_user_files() {
        let mut idx = FileIndex::new();
        let root = idx.add_root(Path::new("/r"));
        let appdata = idx.push(root, "AppData", true);
        let deep = idx.push(appdata, "profile", true);
        idx.push(deep, "report.txt", false);
        let docs = idx.push(root, "Documents", true);
        idx.push(docs, "report-2026.txt", false);
        let hits = idx.search("report", 10, &no_boost);
        assert_eq!(names(&hits), ["report-2026.txt", "report.txt"]);
    }

    #[test]
    fn growth_after_shrink_is_bounded() {
        let mut idx = FileIndex::new();
        let root = idx.add_root(Path::new("/r"));
        for i in 0..100_000 {
            idx.push(root, &format!("file-{i}.txt"), false);
        }
        idx.shrink_to_fit();
        let before = idx.memory_bytes();
        idx.push(root, "one-more.txt", false);
        assert!(idx.memory_bytes() < before + before / 4, "{before} -> {}", idx.memory_bytes());
    }

    #[test]
    fn root_names_are_not_results() {
        let idx = sample();
        assert!(idx.search("/r", 10, &no_boost).is_empty());
    }

    #[test]
    fn fold_case_keeps_length() {
        for s in ["İstanbul", "ÇĞÖŞÜ", "ẞig", "Ǆ"] {
            assert_eq!(fold_case(s).len(), s.len(), "{s}");
        }
        assert_eq!(fold_case("ÇĞÖŞÜ"), "çğöşü");
    }
}
