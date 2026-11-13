use std::path::PathBuf;

/// What happens when the user accepts a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemAction {
    /// Put the text on the clipboard (calculator results).
    CopyText(String),
    /// Open a file, folder or app with the system handler.
    Open(PathBuf),
    /// Open a URL in the default browser.
    OpenUrl(String),
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
}

/// A source of launcher results. Implementations must be cheap to call
/// repeatedly; the launcher runs every enabled provider on each keystroke on a
/// background thread and drops stale results.
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn query(&self, query: &str) -> Vec<SearchItem>;
}

/// Runs all providers and returns their merged results, best first.
pub fn search_all(providers: &[Box<dyn SearchProvider>], query: &str, limit: usize) -> Vec<SearchItem> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut items: Vec<SearchItem> = providers.iter().flat_map(|p| p.query(query)).collect();
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
                })
                .collect()
        }
    }

    #[test]
    fn merges_by_score_and_limits() {
        let providers: Vec<Box<dyn SearchProvider>> =
            vec![Box::new(Fixed("a", vec![10, 500])), Box::new(Fixed("b", vec![900, 20]))];
        let scores: Vec<u32> = search_all(&providers, "x", 3).iter().map(|i| i.score).collect();
        assert_eq!(scores, vec![900, 500, 20]);
    }

    #[test]
    fn blank_query_returns_nothing() {
        let providers: Vec<Box<dyn SearchProvider>> = vec![Box::new(Fixed("a", vec![1]))];
        assert!(search_all(&providers, "   ", 10).is_empty());
    }
}
