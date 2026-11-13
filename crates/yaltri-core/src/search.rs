use std::path::PathBuf;

/// What happens when the user accepts a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemAction {
    /// Put the text on the clipboard (calculator results).
    CopyText(String),
    /// Open a file, folder or shortcut with the system handler.
    Open(PathBuf),
    /// Hand a URI to the system handler (`https://…`, `shell:AppsFolder\…`).
    OpenUrl(String),
    /// Start a program directly (Linux `.desktop` entries).
    Run {
        program: String,
        args: Vec<String>,
        /// Run inside a terminal emulator (`Terminal=true`).
        terminal: bool,
    },
}

impl ItemAction {
    /// Stable identity used for usage history ("frecency").
    pub fn history_key(&self) -> Option<String> {
        match self {
            ItemAction::CopyText(_) => None,
            ItemAction::Open(path) => Some(format!("open:{}", path.display())),
            ItemAction::OpenUrl(url) => Some(format!("url:{url}")),
            ItemAction::Run { program, args, .. } => Some(format!("run:{program} {}", args.join(" "))),
        }
    }

    /// The file this action points at, for "reveal in folder".
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            ItemAction::Open(path) => Some(path),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchItem {
    pub title: String,
    pub subtitle: String,
    /// Which provider produced this item (`calc`, `files`, `apps`, ...).
    pub provider: &'static str,
    /// Higher sorts first. Providers return 0..=1000; the launcher merges
    /// results from all providers by this score.
    pub score: u32,
    pub action: ItemAction,
    /// Image file (PNG/SVG) shown next to the result.
    pub icon: Option<PathBuf>,
}

/// A source of launcher results. Implementations must be cheap to call
/// repeatedly; the launcher runs every enabled provider on each keystroke on a
/// background thread and drops stale results.
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn query(&self, query: &str) -> Vec<SearchItem>;
}

/// Shared providers (kept alive across settings changes) are providers too.
impl<T: SearchProvider + ?Sized> SearchProvider for std::sync::Arc<T> {
    fn id(&self) -> &'static str {
        (**self).id()
    }
    fn query(&self, query: &str) -> Vec<SearchItem> {
        (**self).query(query)
    }
}

/// Runs all providers and returns their merged results, best first.
/// `boost` adds a per-item bonus (usage history) before ranking.
pub fn search_all(
    providers: &[Box<dyn SearchProvider>],
    query: &str,
    limit: usize,
    boost: &dyn Fn(&SearchItem) -> u32,
) -> Vec<SearchItem> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut items: Vec<SearchItem> = providers.iter().flat_map(|p| p.query(query)).collect();
    for item in &mut items {
        item.score += boost(item);
    }
    // Stable sort keeps each provider's own ordering for equal scores.
    items.sort_by(|a, b| b.score.cmp(&a.score));
    items.truncate(limit);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(&'static str, Vec<u32>);

    impl SearchProvider for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        fn query(&self, q: &str) -> Vec<SearchItem> {
            self.1
                .iter()
                .map(|&score| SearchItem {
                    title: format!("{q}-{score}"),
                    subtitle: String::new(),
                    provider: self.0,
                    score,
                    action: ItemAction::CopyText(String::new()),
                    icon: None,
                })
                .collect()
        }
    }

    #[test]
    fn merges_by_score_and_limits() {
        let providers: Vec<Box<dyn SearchProvider>> =
            vec![Box::new(Fixed("a", vec![10, 500])), Box::new(Fixed("b", vec![900, 20]))];
        let scores: Vec<u32> = search_all(&providers, "x", 3, &|_| 0).iter().map(|i| i.score).collect();
        assert_eq!(scores, vec![900, 500, 20]);
        let boosted: Vec<u32> =
            search_all(&providers, "x", 1, &|i| if i.score == 10 { 1000 } else { 0 }).iter().map(|i| i.score).collect();
        assert_eq!(boosted, vec![1010]);
    }

    #[test]
    fn blank_query_returns_nothing() {
        let providers: Vec<Box<dyn SearchProvider>> = vec![Box::new(Fixed("a", vec![1]))];
        assert!(search_all(&providers, "   ", 10, &|_| 0).is_empty());
    }
}
