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

/// What a result is; the launcher picks its icon, wording and filter by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// Calculator result; `subtitle` is the expression.
    Calc,
    /// Installed app; `subtitle` is where it was found.
    App,
    /// Installed game; `subtitle` names its launcher.
    Game,
    /// File; `subtitle` is its folder.
    File,
    /// Folder; `subtitle` is its parent folder.
    Folder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchItem {
    pub title: String,
    /// Raw detail for the kind (see `ItemKind`); the UI words it.
    pub subtitle: String,
    pub kind: ItemKind,
    /// Higher sorts first. Providers return 0..=1000; the launcher merges
    /// results from all providers by this score.
    pub score: u32,
    pub action: ItemAction,
    /// Image file (PNG/SVG) shown next to the result.
    pub icon: Option<PathBuf>,
}

/// Narrows launcher results to one kind of item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    #[default]
    All,
    Apps,
    Files,
    Games,
}

impl Scope {
    /// In the order the launcher shows them.
    pub const ALL: [Scope; 4] = [Scope::All, Scope::Apps, Scope::Files, Scope::Games];

    /// Whether an item of `kind` belongs here.
    pub fn admits(self, kind: ItemKind) -> bool {
        match self {
            Scope::All => true,
            Scope::Apps => kind == ItemKind::App,
            Scope::Files => matches!(kind, ItemKind::File | ItemKind::Folder),
            Scope::Games => kind == ItemKind::Game,
        }
    }

    /// Whether the provider with this id can produce items for this scope.
    fn queries(self, provider_id: &str) -> bool {
        match self {
            Scope::All => true,
            Scope::Apps | Scope::Games => provider_id == "apps",
            Scope::Files => provider_id == "files",
        }
    }

    /// The next (or previous) scope, wrapping around.
    pub fn cycle(self, forward: bool) -> Scope {
        let ix = Scope::ALL.iter().position(|&s| s == self).unwrap_or(0) as isize;
        let len = Scope::ALL.len() as isize;
        Scope::ALL[(ix + if forward { 1 } else { -1 }).rem_euclid(len) as usize]
    }

    /// A `/x ` shorthand at the start of `query` (Turkish or English initial:
    /// `/u` `/a` apps, `/d` `/f` files, `/o` `/g` games) and the rest of it.
    pub fn from_prefix(query: &str) -> Option<(Scope, &str)> {
        let rest = query.strip_prefix('/')?;
        let mut chars = rest.chars();
        let scope = match chars.next()?.to_ascii_lowercase() {
            'u' | 'a' => Scope::Apps,
            'd' | 'f' => Scope::Files,
            'o' | 'g' => Scope::Games,
            _ => return None,
        };
        chars.as_str().strip_prefix(' ').map(|rest| (scope, rest))
    }
}

/// A source of launcher results. Implementations must be cheap to call
/// repeatedly; the launcher runs every enabled provider on each keystroke on a
/// background thread and drops stale results.
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn query(&self, query: &str) -> Vec<SearchItem>;

    /// Results for one scope. Providers that mix kinds (apps and games)
    /// override this to filter before they cut their result list, and may
    /// list everything of that kind for an empty query.
    fn query_scoped(&self, query: &str, _scope: Scope) -> Vec<SearchItem> {
        self.query(query)
    }
}

/// Shared providers (kept alive across settings changes) are providers too.
impl<T: SearchProvider + ?Sized> SearchProvider for std::sync::Arc<T> {
    fn id(&self) -> &'static str {
        (**self).id()
    }
    fn query(&self, query: &str) -> Vec<SearchItem> {
        (**self).query(query)
    }
    fn query_scoped(&self, query: &str, scope: Scope) -> Vec<SearchItem> {
        (**self).query_scoped(query, scope)
    }
}

/// Runs the providers of `scope` and returns their merged results, best
/// first. `boost` adds a per-item bonus (usage history) before ranking. A
/// blank query finds nothing, except inside a scope, where providers may
/// list what they have.
pub fn search_all(
    providers: &[Box<dyn SearchProvider>],
    query: &str,
    scope: Scope,
    limit: usize,
    boost: &dyn Fn(&SearchItem) -> u32,
) -> Vec<SearchItem> {
    let query = query.trim();
    if query.is_empty() && scope == Scope::All {
        return Vec::new();
    }
    let mut items: Vec<SearchItem> = providers
        .iter()
        .filter(|p| scope.queries(p.id()))
        .flat_map(|p| p.query_scoped(query, scope))
        .filter(|item| scope.admits(item.kind))
        .collect();
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

    fn kind_of(provider: &str) -> ItemKind {
        match provider {
            "files" => ItemKind::File,
            "apps" => ItemKind::App,
            _ => ItemKind::Calc,
        }
    }

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
                    kind: kind_of(self.0),
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
        let scores: Vec<u32> = search_all(&providers, "x", Scope::All, 3, &|_| 0).iter().map(|i| i.score).collect();
        assert_eq!(scores, vec![900, 500, 20]);
        let boosted: Vec<u32> = search_all(&providers, "x", Scope::All, 1, &|i| {
            if i.score == 10 { 1000 } else { 0 }
        })
        .iter()
        .map(|i| i.score)
        .collect();
        assert_eq!(boosted, vec![1010]);
    }

    #[test]
    fn blank_query_returns_nothing() {
        let providers: Vec<Box<dyn SearchProvider>> = vec![Box::new(Fixed("a", vec![1]))];
        assert!(search_all(&providers, "   ", Scope::All, 10, &|_| 0).is_empty());
    }

    #[test]
    fn scopes_keep_only_their_kind() {
        let providers: Vec<Box<dyn SearchProvider>> =
            vec![Box::new(Fixed("files", vec![900])), Box::new(Fixed("apps", vec![10]))];
        let only = |scope| search_all(&providers, "x", scope, 10, &|_| 0).iter().map(|i| i.kind).collect::<Vec<_>>();
        assert_eq!(only(Scope::Files), [ItemKind::File]);
        assert_eq!(only(Scope::Apps), [ItemKind::App]);
        assert!(only(Scope::Games).is_empty());
        assert_eq!(only(Scope::All), [ItemKind::File, ItemKind::App]);
    }

    #[test]
    fn prefixes_pick_a_scope() {
        assert_eq!(Scope::from_prefix("/o palw"), Some((Scope::Games, "palw")));
        assert_eq!(Scope::from_prefix("/G "), Some((Scope::Games, "")));
        assert_eq!(Scope::from_prefix("/u code"), Some((Scope::Apps, "code")));
        assert_eq!(Scope::from_prefix("/f cargo"), Some((Scope::Files, "cargo")));
        // Needs the space, so typing a path like "/usr" is left alone.
        assert_eq!(Scope::from_prefix("/usr"), None);
        assert_eq!(Scope::from_prefix("/x a"), None);
        assert_eq!(Scope::Games.cycle(true), Scope::All);
        assert_eq!(Scope::All.cycle(false), Scope::Games);
    }
}
