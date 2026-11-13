//! What to index: roots and exclusions, from settings plus built-in system
//! folders that are never useful in a launcher.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use yaltri_core::settings::FilesSettings;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Scope {
    pub roots: Vec<PathBuf>,
    /// Compared case-insensitively on Windows.
    exclude_paths: Vec<PathBuf>,
    /// Lowercased folder names.
    exclude_names: Vec<String>,
}

impl Scope {
    pub fn from_settings(settings: &FilesSettings) -> Self {
        let roots = if settings.roots.is_empty() { default_roots() } else { settings.roots.clone() };
        let mut exclude_paths = builtin_excludes();
        exclude_paths.extend(settings.exclude_paths.iter().cloned());
        let exclude_names: HashSet<String> = settings.exclude_names.iter().map(|n| n.to_lowercase()).collect();
        let mut exclude_names: Vec<String> = exclude_names.into_iter().collect();
        exclude_names.sort();
        Self { roots, exclude_paths: exclude_paths.iter().map(|p| normalize(p)).collect(), exclude_names }
    }

    #[cfg(test)]
    pub fn for_test(roots: Vec<PathBuf>, exclude_paths: Vec<PathBuf>, exclude_names: &[&str]) -> Self {
        Self {
            roots,
            exclude_paths: exclude_paths.iter().map(|p| normalize(p)).collect(),
            exclude_names: exclude_names.iter().map(|n| n.to_lowercase()).collect(),
        }
    }

    /// Whether a directory entry named `name` at `path` should be skipped.
    pub fn excludes(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name() {
            let name = name.to_string_lossy().to_lowercase();
            if self.exclude_names.contains(&name) {
                return true;
            }
        }
        let path = normalize(path);
        self.exclude_paths.iter().any(|p| path == *p)
    }

    /// Whether `path` is inside an excluded folder (for filtering watcher events).
    pub fn is_inside_excluded(&self, path: &Path) -> bool {
        let normalized = normalize(path);
        if self.exclude_paths.iter().any(|p| normalized.starts_with(p)) {
            return true;
        }
        path.components().any(|c| {
            let name = c.as_os_str().to_string_lossy().to_lowercase();
            self.exclude_names.contains(&name)
        })
    }

    /// Stable fingerprint, stored with the on-disk index so a settings change
    /// invalidates it.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish()
    }
}

fn normalize(p: &Path) -> PathBuf {
    if cfg!(windows) { PathBuf::from(p.to_string_lossy().to_lowercase()) } else { p.to_path_buf() }
}

#[cfg(windows)]
fn default_roots() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    // SAFETY: plain Win32 queries; the path buffer is NUL-terminated.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
        })
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
fn default_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

#[cfg(windows)]
fn builtin_excludes() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(sys) = std::env::var_os("SystemRoot") {
        v.push(PathBuf::from(sys));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        v.push(Path::new(&local).join("Temp"));
    }
    v
}

#[cfg(not(windows))]
fn builtin_excludes() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        "/proc", "/sys", "/dev", "/run", "/tmp", "/var/tmp", "/var/cache", "/var/lib/docker",
        "/var/lib/containerd", "/var/lib/snapd", "/snap", "/lost+found",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        v.push(Path::new(&home).join(".cache"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_by_name_and_path() {
        let scope = Scope::for_test(vec!["/r".into()], vec!["/r/skip".into()], &["node_modules"]);
        assert!(scope.excludes(Path::new("/r/a/node_modules")));
        assert!(scope.excludes(Path::new("/r/a/Node_Modules")));
        assert!(scope.excludes(Path::new("/r/skip")));
        assert!(!scope.excludes(Path::new("/r/skipper")));
        assert!(scope.is_inside_excluded(Path::new("/r/skip/x/y.txt")));
        assert!(scope.is_inside_excluded(Path::new("/r/a/node_modules/x.js")));
        assert!(!scope.is_inside_excluded(Path::new("/r/a/b.txt")));
    }

    #[test]
    fn default_scope_has_roots() {
        let scope = Scope::from_settings(&FilesSettings::default());
        assert!(!scope.roots.is_empty());
    }
}
