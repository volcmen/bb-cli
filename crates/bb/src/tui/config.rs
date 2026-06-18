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

/// A user-defined key that runs a templated external command (#90).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomKey {
    pub key: char,
    pub name: String,
    pub command: String,
    /// Which section it applies to (`None` = all).
    pub context: Option<Tab>,
}

impl CustomKey {
    /// Whether this binding is active on `tab`.
    #[must_use]
    pub fn applies(&self, tab: Tab) -> bool {
        self.context.map_or(true, |c| c == tab)
    }
}

/// The JSON shape stored under the `dash_custom_keys` config key.
#[derive(serde::Deserialize)]
struct RawCustomKey {
    key: String,
    name: String,
    command: String,
    #[serde(default)]
    context: Option<String>,
}

/// Expand `{{id}}`/`{{url}}`/`{{branch}}`/`{{repo}}`/`{{workspace}}`/`{{slug}}`
/// (and any provided var) in `template` from `vars`.
#[must_use]
pub fn expand_template(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = template.to_owned();
    for (name, value) in vars {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

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
    pub custom_keys: Vec<CustomKey>,
}

impl Default for DashConfig {
    fn default() -> Self {
        Self {
            default_tab: Tab::PullRequests,
            refresh_secs: 5,
            theme: Theme::default(),
            custom_keys: Vec::new(),
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

        if let Some(raw) = get("dash_custom_keys") {
            match serde_json::from_str::<Vec<RawCustomKey>>(&raw) {
                Ok(entries) => {
                    for e in entries {
                        let Some(key) = e.key.chars().next().filter(|_| e.key.chars().count() == 1)
                        else {
                            warnings.push(format!("custom key {:?}: must be a single char", e.key));
                            continue;
                        };
                        if super::keymap::is_reserved(key) {
                            warnings.push(format!(
                                "custom key '{key}' collides with a built-in binding; ignored"
                            ));
                            continue;
                        }
                        let context = match e.context.as_deref() {
                            None => None,
                            Some("pr") => Some(Tab::PullRequests),
                            Some("issue") => Some(Tab::Issues),
                            Some("pipeline") => Some(Tab::Pipelines),
                            Some(other) => {
                                warnings
                                    .push(format!("custom key '{key}': unknown context {other:?}"));
                                continue;
                            }
                        };
                        cfg.custom_keys.push(CustomKey {
                            key,
                            name: e.name,
                            command: e.command,
                            context,
                        });
                    }
                }
                Err(e) => warnings.push(format!("dash_custom_keys: invalid JSON ({e})")),
            }
        }

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
    fn custom_keys_parse_and_reject_collisions() {
        let cfg = FileConfig::blank();
        cfg.set(
            "",
            "dash_custom_keys",
            r#"[
                {"key":"v","name":"editor","command":"$EDITOR {{branch}}","context":"pr"},
                {"key":"m","name":"bad","command":"x"},
                {"key":"L","name":"lazygit","command":"lazygit"}
            ]"#,
        )
        .unwrap();
        let (dash, warnings) = DashConfig::load(&cfg);
        // 'v' and 'L' register; 'm' collides with the built-in merge binding.
        assert_eq!(dash.custom_keys.len(), 2);
        assert_eq!(dash.custom_keys[0].key, 'v');
        assert_eq!(dash.custom_keys[0].context, Some(Tab::PullRequests));
        assert_eq!(dash.custom_keys[1].key, 'L');
        assert!(dash.custom_keys[1].context.is_none());
        assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
        assert!(warnings[0].contains("collides"));
    }

    #[test]
    fn custom_keys_invalid_json_warns() {
        let cfg = FileConfig::blank();
        cfg.set("", "dash_custom_keys", "not json").unwrap();
        let (dash, warnings) = DashConfig::load(&cfg);
        assert!(dash.custom_keys.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn expand_template_fills_vars() {
        let out = expand_template(
            "$EDITOR {{branch}} # {{repo}}#{{id}} {{url}}",
            &[
                ("branch", "feat/x".to_owned()),
                ("repo", "acme/widgets".to_owned()),
                ("id", "7".to_owned()),
                ("url", "https://bb/7".to_owned()),
            ],
        );
        assert_eq!(out, "$EDITOR feat/x # acme/widgets#7 https://bb/7");
    }

    #[test]
    fn custom_key_applies_respects_context() {
        let ck = CustomKey {
            key: 'v',
            name: "x".to_owned(),
            command: "x".to_owned(),
            context: Some(Tab::Issues),
        };
        assert!(ck.applies(Tab::Issues));
        assert!(!ck.applies(Tab::PullRequests));
        let any = CustomKey {
            context: None,
            ..ck
        };
        assert!(any.applies(Tab::Pipelines));
    }

    #[test]
    fn parse_color_known_and_unknown() {
        assert_eq!(parse_color("CYAN"), Some(Color::Cyan));
        assert_eq!(parse_color("grey"), Some(Color::Gray));
        assert_eq!(parse_color("octarine"), None);
    }
}
