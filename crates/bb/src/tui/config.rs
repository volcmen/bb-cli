//! `bb dash` configuration + theming (spec 042).
//!
//! Read from the existing flat `config.toml` via [`ConfigProvider`] under
//! `dash_*` keys (the config layer stores a flat string map, so a nested
//! `[dash]` table isn't representable — flat keys keep `bb config set` working).
//! Everything is optional; an invalid value falls back to the default and adds a
//! warning (surfaced on the status line) — parsing never fails.

use ratatui::style::Color;

use super::app::Tab;
use crate::core::ConfigProvider;

/// Colors for PR/CI states (named → [`Color`]). All optional, with sane defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub open: Color,
    pub merged: Color,
    pub failed: Color,
    pub in_progress: Color,
    pub other: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            open: Color::Green,
            merged: Color::Cyan,
            failed: Color::Red,
            in_progress: Color::Yellow,
            other: Color::Gray,
        }
    }
}

impl Theme {
    /// The color for a PR/CI `state` name.
    #[must_use]
    pub fn state(&self, state: &str) -> Color {
        match state {
            "OPEN" | "SUCCESSFUL" => self.open,
            "MERGED" => self.merged,
            "INPROGRESS" => self.in_progress,
            "DECLINED" | "SUPERSEDED" | "FAILED" | "STOPPED" => self.failed,
            _ => self.other,
        }
    }
}

/// Parsed `bb dash` configuration.
#[derive(Debug, Clone)]
pub struct DashConfig {
    pub default_tab: Tab,
    pub refresh_secs: u64,
    pub theme: Theme,
}

impl Default for DashConfig {
    fn default() -> Self {
        Self {
            default_tab: Tab::PullRequests,
            refresh_secs: 5,
            theme: Theme::default(),
        }
    }
}

impl DashConfig {
    /// Load from config, collecting a warning for each malformed/unknown value
    /// (the rest fall back to defaults). Never fails.
    #[must_use]
    pub fn load(config: &dyn ConfigProvider) -> (Self, Vec<String>) {
        let mut cfg = DashConfig::default();
        let mut warnings = Vec::new();
        let get = |k: &str| config.get("", k).filter(|v| !v.is_empty());

        if let Some(v) = get("dash_default_tab") {
            match v.as_str() {
                "pr" => cfg.default_tab = Tab::PullRequests,
                "issue" => cfg.default_tab = Tab::Issues,
                "pipeline" => cfg.default_tab = Tab::Pipelines,
                other => warnings.push(format!("dash_default_tab: unknown tab {other:?}")),
            }
        }
        if let Some(v) = get("dash_refresh_secs") {
            match v.parse::<u64>() {
                // Bound it so it never hammers the API or spins too tight.
                Ok(n) if (2..=120).contains(&n) => cfg.refresh_secs = n,
                _ => warnings.push(format!("dash_refresh_secs: expected 2..=120, got {v:?}")),
            }
        }

        let mut color = |key: &str, slot: &mut Color| {
            if let Some(v) = get(key) {
                match parse_color(&v) {
                    Some(c) => *slot = c,
                    None => warnings.push(format!("{key}: unknown color {v:?}")),
                }
            }
        };
        color("dash_theme_state_open", &mut cfg.theme.open);
        color("dash_theme_state_merged", &mut cfg.theme.merged);
        color("dash_theme_state_failed", &mut cfg.theme.failed);
        color("dash_theme_state_in_progress", &mut cfg.theme.in_progress);

        (cfg, warnings)
    }
}

/// Map a color name to a [`Color`] (the common 16 names + a couple of aliases).
fn parse_color(name: &str) -> Option<Color> {
    Some(match name.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "white" => Color::White,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::config::FileConfig;

    use super::*;

    #[test]
    fn missing_config_is_all_defaults() {
        let cfg = Arc::new(FileConfig::blank());
        let (dash, warnings) = DashConfig::load(cfg.as_ref());
        assert_eq!(dash.default_tab, Tab::PullRequests);
        assert_eq!(dash.refresh_secs, 5);
        assert_eq!(dash.theme, Theme::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn valid_values_parse() {
        let cfg = FileConfig::blank();
        cfg.set("", "dash_default_tab", "pipeline").unwrap();
        cfg.set("", "dash_refresh_secs", "10").unwrap();
        cfg.set("", "dash_theme_state_open", "blue").unwrap();
        let (dash, warnings) = DashConfig::load(&cfg);
        assert_eq!(dash.default_tab, Tab::Pipelines);
        assert_eq!(dash.refresh_secs, 10);
        assert_eq!(dash.theme.open, Color::Blue);
        assert!(warnings.is_empty());
    }

    #[test]
    fn malformed_values_warn_and_default() {
        let cfg = FileConfig::blank();
        cfg.set("", "dash_default_tab", "bogus").unwrap();
        cfg.set("", "dash_refresh_secs", "9999").unwrap();
        cfg.set("", "dash_theme_state_failed", "chartreuse")
            .unwrap();
        let (dash, warnings) = DashConfig::load(&cfg);
        // Fell back to defaults.
        assert_eq!(dash.default_tab, Tab::PullRequests);
        assert_eq!(dash.refresh_secs, 5);
        assert_eq!(dash.theme.failed, Color::Red);
        assert_eq!(warnings.len(), 3, "warnings: {warnings:?}");
    }

    #[test]
    fn parse_color_known_and_unknown() {
        assert_eq!(parse_color("CYAN"), Some(Color::Cyan));
        assert_eq!(parse_color("grey"), Some(Color::Gray));
        assert_eq!(parse_color("octarine"), None);
    }
}
