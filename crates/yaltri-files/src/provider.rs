use std::sync::Arc;

use yaltri_core::{ItemAction, SearchItem, SearchProvider};

use crate::indexer::{FileIndexer, Shared};

/// Results per query; the launcher re-ranks them with usage history.
const CANDIDATES: usize = 24;
/// One-letter queries match most of the disk and are never what you want.
const MIN_QUERY_CHARS: usize = 2;

pub struct FilesProvider {
    shared: Arc<Shared>,
}

impl FilesProvider {
    pub fn new(indexer: &FileIndexer) -> Self {
        Self { shared: indexer.shared.clone() }
    }
}

impl SearchProvider for FilesProvider {
    fn id(&self) -> &'static str {
        "files"
    }

    fn query(&self, query: &str) -> Vec<SearchItem> {
        if query.trim().chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        let hits = {
            let index = self.shared.index.read().unwrap();
            index.search(query, CANDIDATES, &|_| 0)
        };
        hits.into_iter()
            .map(|hit| {
                let title = hit.path.file_name().map_or_else(
                    || hit.path.to_string_lossy().into_owned(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let parent = hit.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                SearchItem {
                    title,
                    subtitle: if hit.is_dir { format!("Klasör · {parent}") } else { parent },
                    provider: "files",
                    score: hit.score,
                    action: ItemAction::Open(hit.path),
                    icon: None,
                }
            })
            .collect()
    }
}
