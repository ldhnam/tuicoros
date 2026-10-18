//! Key hints per context. The footer and the help overlay are generated from
//! these lists, so they can't drift from each other. The bindings themselves
//! live next to each screen's `map_key`; `tests::documented_keys_are_bound`
//! checks the two agree.

/// One documented binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyHint {
    pub keys: &'static str,
    pub action: &'static str,
    /// Shown in the footer (the most useful few), not just in help.
    pub footer: bool,
}

const fn hint(keys: &'static str, action: &'static str, footer: bool) -> KeyHint {
    KeyHint {
        keys,
        action,
        footer,
    }
}

/// Where keyboard focus is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Login,
    Activities,
    Search,
    Filter,
    Detail,
    Placeholder,
    Help,
}

pub const GLOBAL: &[KeyHint] = &[
    hint("1–5", "switch tab", false),
    hint("?", "help", true),
    hint("q", "back / quit", true),
    hint("Ctrl+C", "quit now", false),
];

pub const LOGIN: &[KeyHint] = &[
    hint("Enter", "sign in", true),
    hint("Tab ↓", "next field", true),
    hint("Shift+Tab ↑", "previous field", false),
    hint("← →", "change region", true),
    hint("Space", "toggle remember me", true),
    hint("Ctrl+U", "clear field", false),
    hint("Ctrl+C", "quit", true),
];

pub const ACTIVITIES: &[KeyHint] = &[
    hint("↑↓ j k", "move", true),
    hint("PgUp PgDn", "page", false),
    hint("g G", "first / last", false),
    hint("Enter", "open", true),
    hint("/", "search names", true),
    hint("f", "filter sport / dates", true),
    hint("s", "sort", true),
    hint("r", "refresh", true),
];

pub const SEARCH: &[KeyHint] = &[
    hint("type", "filter by name", true),
    hint("Enter", "keep filter", true),
    hint("Esc", "clear", true),
];

pub const FILTER: &[KeyHint] = &[
    hint("↑↓", "choose", true),
    hint("Tab ← →", "sport / dates", true),
    hint("Enter", "apply", true),
    hint("Esc", "cancel", true),
];

pub const DETAIL: &[KeyHint] = &[
    hint("Esc q", "back", true),
    hint("Tab ← →", "summary / laps", true),
    hint("↑↓ j k", "scroll laps", false),
    hint("c", "next lap grouping", true),
    hint("r", "reload", true),
];

pub const HELP: &[KeyHint] = &[hint("Esc ?", "close", true)];

pub fn for_context(context: Context) -> &'static [KeyHint] {
    match context {
        Context::Login => LOGIN,
        Context::Activities => ACTIVITIES,
        Context::Search => SEARCH,
        Context::Filter => FILTER,
        Context::Detail => DETAIL,
        Context::Placeholder => &[],
        Context::Help => HELP,
    }
}

/// Footer text: `key action · key action …`, global help/quit appended where they apply.
pub fn footer(context: Context) -> String {
    let own = for_context(context).iter().filter(|h| h.footer);
    let with_globals = !matches!(
        context,
        Context::Login | Context::Search | Context::Filter | Context::Help
    );
    // "? help" first: when the footer is cut short, it still leads to the rest.
    let help = GLOBAL.iter().filter(|h| with_globals && h.keys == "?");
    let rest = GLOBAL
        .iter()
        .filter(|h| with_globals && h.footer && h.keys != "?");
    help.chain(own)
        .chain(rest)
        .map(|h| format!("{} {}", h.keys, h.action))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footers_are_short_enough_to_be_useful() {
        for context in [
            Context::Activities,
            Context::Detail,
            Context::Search,
            Context::Filter,
        ] {
            let text = footer(context);
            assert!(!text.is_empty());
            assert!(text.chars().count() < 120, "{context:?}: {text}");
        }
        assert!(footer(Context::Activities).starts_with("? help"));
        assert!(footer(Context::Login).starts_with("Enter sign in"));
        assert!(
            !footer(Context::Search).contains("q back"),
            "q types into the search box"
        );
    }
}
