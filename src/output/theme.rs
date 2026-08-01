//! Colour handling.
//!
//! Colour is disabled when any of the usual signals says so, and the plan remains
//! unambiguous without it: `+`, `~` and `-` carry the meaning by themselves.

use owo_colors::{OwoColorize, Style};

/// Whether and how to colourise output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    enabled: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::auto()
    }
}

impl Theme {
    /// Colour on.
    pub fn colored() -> Self {
        Self { enabled: true }
    }

    /// Colour off.
    pub fn plain() -> Self {
        Self { enabled: false }
    }

    /// Decide from the environment.
    ///
    /// Honours `NO_COLOR`, `CLICOLOR_FORCE`, `TERM=dumb` and whether stdout is a
    /// terminal, in that order of precedence.
    pub fn auto() -> Self {
        Self {
            enabled: should_colorize(),
        }
    }

    /// Override from a `--color` flag, falling back to detection.
    pub fn from_flag(force: Option<bool>) -> Self {
        match force {
            Some(true) => Self::colored(),
            Some(false) => Self::plain(),
            None => Self::auto(),
        }
    }

    /// Whether colour is on.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Apply a style, or not.
    pub fn paint(&self, text: &str, style: Style) -> String {
        if self.enabled {
            text.style(style).to_string()
        } else {
            text.to_string()
        }
    }

    /// Style for a created item.
    pub fn create(&self, text: &str) -> String {
        self.paint(text, Style::new().green())
    }

    /// Style for an updated item.
    pub fn update(&self, text: &str) -> String {
        self.paint(text, Style::new().yellow())
    }

    /// Style for a deleted item.
    pub fn delete(&self, text: &str) -> String {
        self.paint(text, Style::new().red())
    }

    /// Style for a section heading.
    pub fn heading(&self, text: &str) -> String {
        self.paint(text, Style::new().bold())
    }

    /// Style for de-emphasised detail.
    pub fn dim(&self, text: &str) -> String {
        self.paint(text, Style::new().dimmed())
    }

    /// Style for a warning.
    pub fn warn(&self, text: &str) -> String {
        self.paint(text, Style::new().yellow().bold())
    }

    /// Style for an error.
    pub fn error(&self, text: &str) -> String {
        self.paint(text, Style::new().red().bold())
    }

    /// Style for a success marker.
    pub fn success(&self, text: &str) -> String {
        self.paint(text, Style::new().green().bold())
    }
}

/// Decide whether to colourise, following the usual conventions.
fn should_colorize() -> bool {
    // An explicit force wins over everything, including a non-tty, so that
    // piping into a pager or a log collector can still be coloured.
    if std::env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        return true;
    }
    // https://no-color.org: any value, including empty, disables colour.
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        return false;
    }
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_plain_theme_emits_no_escape_sequences() {
        let theme = Theme::plain();
        assert_eq!(theme.create("+ bug"), "+ bug");
        assert_eq!(theme.delete("- bug"), "- bug");
        assert_eq!(theme.heading("Labels"), "Labels");
    }

    #[test]
    fn a_coloured_theme_wraps_text() {
        let theme = Theme::colored();
        let painted = theme.create("bug");
        assert!(painted.contains("bug"));
        assert!(painted.len() > "bug".len(), "expected escape sequences");
    }

    #[test]
    fn an_explicit_flag_overrides_detection() {
        assert!(Theme::from_flag(Some(true)).is_enabled());
        assert!(!Theme::from_flag(Some(false)).is_enabled());
    }
}
