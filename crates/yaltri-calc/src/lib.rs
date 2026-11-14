//! Calculator search provider backed by `fend-core`: arithmetic, functions and
//! unit conversion (`2^10`, `sqrt(2)`, `5 km to mi`, `0xff to decimal`).

use std::time::{Duration, Instant};

use yaltri_core::{ItemAction, ItemKind, SearchItem, SearchProvider};

/// Hard ceiling per evaluation so input like `10^10^10` cannot freeze search.
const TIMEOUT: Duration = Duration::from_millis(150);

/// Calculator results go above everything else when they exist.
const SCORE: u32 = 1000;

#[derive(Default)]
pub struct CalcProvider;

impl CalcProvider {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates `input`, returning the result text, or `None` when the input
    /// does not look like math or does not evaluate to anything useful.
    pub fn evaluate(&self, input: &str) -> Option<String> {
        let input = input.trim();
        let expr = input.strip_prefix('=').unwrap_or(input).trim();
        // Bare words like "notepad" or "c" (speed of light) would evaluate;
        // only treat input as math if it has a digit or was forced with '='.
        if expr.is_empty() || (!input.starts_with('=') && !expr.chars().any(|c| c.is_ascii_digit())) {
            return None;
        }

        let deadline = Deadline(Instant::now() + TIMEOUT);
        let context = fend_core::Context::new();
        let result = fend_core::evaluate_preview_with_interrupt(expr, &context, &deadline);
        let text = result.get_main_result().trim();
        if text.is_empty() || text == expr {
            None
        } else {
            Some(text.to_owned())
        }
    }
}

impl SearchProvider for CalcProvider {
    fn id(&self) -> &'static str {
        "calc"
    }

    fn query(&self, query: &str) -> Vec<SearchItem> {
        self.evaluate(query)
            .map(|result| SearchItem {
                // Copy the bare value: "approx. 3.10 mi" -> "3.10 mi".
                action: ItemAction::CopyText(result.trim_start_matches("approx. ").to_owned()),
                title: result,
                subtitle: query.trim().to_owned(),
                kind: ItemKind::Calc,
                score: SCORE,
                icon: None,
            })
            .into_iter()
            .collect()
    }
}

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() >= self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str) -> Option<String> {
        CalcProvider::new().evaluate(s)
    }

    #[test]
    fn arithmetic() {
        assert_eq!(eval("1 + 1").as_deref(), Some("2"));
        assert_eq!(eval("2^10").as_deref(), Some("1024"));
        assert_eq!(eval("(3+4)*2").as_deref(), Some("14"));
    }

    #[test]
    fn units() {
        let r = eval("1 km to m").unwrap();
        assert!(r.starts_with("1000"), "{r}");
    }

    #[test]
    fn ignores_plain_words() {
        assert_eq!(eval("notepad"), None);
        assert_eq!(eval("c"), None);
        assert_eq!(eval(""), None);
    }

    #[test]
    fn equals_prefix_forces_math() {
        assert!(eval("=pi").unwrap().starts_with("approx. 3.14"), "{:?}", eval("=pi"));
    }

    #[test]
    fn plain_number_is_not_a_result() {
        assert_eq!(eval("42"), None);
    }

    #[test]
    fn huge_input_returns_quickly() {
        let start = Instant::now();
        let _ = eval("10^10^10^10");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn provider_yields_copy_action() {
        let items = CalcProvider::new().query("6*7");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, ItemAction::CopyText("42".into()));
    }

    #[test]
    fn copy_value_drops_approx_prefix() {
        let items = CalcProvider::new().query("1/3");
        assert!(items[0].title.starts_with("approx. "), "{}", items[0].title);
        let ItemAction::CopyText(copied) = &items[0].action else { panic!() };
        assert!(copied.starts_with("0.33"), "{copied}");
    }
}
