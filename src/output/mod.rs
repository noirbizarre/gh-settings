//! Rendering.
//!
//! Output is a product surface. Plans are grouped by resource, colour-coded by
//! operation, and readable when colour is unavailable — the sigil carries the
//! meaning, colour only reinforces it.

pub mod human;
pub mod json;
pub mod theme;

pub use human::HumanRenderer;
pub use json::JsonRenderer;
pub use theme::Theme;

/// Output format selected on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// Human-readable, coloured when the terminal supports it.
    #[default]
    Text,
    /// Machine-readable JSON.
    Json,
}
