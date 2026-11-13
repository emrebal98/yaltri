//! Launch history: items the user opens often or recently rank higher.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Entries beyond this are dropped, least valuable first.
const MAX_ENTRIES: usize = 2000;
const DAY: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub count: u32,
    /// Seconds since the Unix epoch.
    pub last_used: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    entries: HashMap<String, Usage>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    entries: Vec<(String, Usage)>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl History {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str::<OnDisk>(&text) {
            Ok(disk) => Self { entries: disk.entries.into_iter().collect() },
            Err(e) => {
                tracing::warn!(%e, "history file unreadable, starting fresh");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut entries: Vec<(String, Usage)> = self.entries.iter().map(|(k, v)| (k.clone(), *v)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let text = toml::to_string(&OnDisk { entries }).context("serializing history")?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn record(&mut self, key: &str) {
        self.record_at(key, now());
    }

    fn record_at(&mut self, key: &str, at: u64) {
        let usage = self.entries.entry(key.to_owned()).or_default();
        usage.count = usage.count.saturating_add(1);
        usage.last_used = at;
        if self.entries.len() > MAX_ENTRIES {
            self.prune(at);
        }
    }

    /// Score bonus (0..=400) for an item with this key.
    pub fn boost(&self, key: &str) -> u32 {
        self.boost_at(key, now())
    }

    fn boost_at(&self, key: &str, at: u64) -> u32 {
        let Some(usage) = self.entries.get(key) else {
            return 0;
        };
        let frequency = usage.count.min(20) * 15;
        let age = at.saturating_sub(usage.last_used);
        let recency = if age < DAY {
            100
        } else if age < 7 * DAY {
            50
        } else if age < 30 * DAY {
            20
        } else {
            0
        };
        frequency + recency
    }

    fn prune(&mut self, at: u64) {
        let mut ranked: Vec<(String, u32)> = self.entries.keys().map(|k| (k.clone(), self.boost_at(k, at))).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        for (key, _) in ranked.into_iter().skip(MAX_ENTRIES) {
            self.entries.remove(&key);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_items_get_no_boost() {
        assert_eq!(History::default().boost("x"), 0);
    }

    #[test]
    fn frequent_and_recent_items_rank_higher() {
        let mut h = History::default();
        let t = 1_000_000_000;
        h.record_at("often", t);
        h.record_at("often", t);
        h.record_at("often", t);
        h.record_at("once", t);
        assert!(h.boost_at("often", t) > h.boost_at("once", t));
        assert!(h.boost_at("once", t) > h.boost_at("once", t + 40 * DAY));
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("yaltri-history-{}", std::process::id()));
        let path = dir.join("history.toml");
        let mut h = History::default();
        h.record(r"open:C:\Users\me\file.txt");
        h.record("url:shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
        h.save(&path).unwrap();
        assert_eq!(History::load(&path), h);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prunes_to_capacity() {
        let mut h = History::default();
        for i in 0..MAX_ENTRIES + 10 {
            h.record_at(&format!("k{i}"), 1);
        }
        assert!(h.len() <= MAX_ENTRIES);
    }
}
