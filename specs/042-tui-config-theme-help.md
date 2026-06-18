# 042 tui: config schema + theming + help overlay

## Goal
Make the dashboard configurable and discoverable: a `[dash]` config schema
(sections, columns, theme colors, keybinding overrides) and a `?` help overlay
generated from the live keymap. Zero-config must still work.

## Command surface
Internal. New key: `?` toggles a help overlay listing the current context's
bindings (built from `keymap.rs`, so it never drifts from reality). Config read via
the existing `ConfigProvider`/`config.toml`.

## Config schema (`config.toml`)
```toml
[dash]
default_tab = "pr"          # pr | issue | pipeline
refresh_secs = 5            # pipeline auto-refresh bound

[dash.theme]                # named colors → ratatui Style; all optional
selected = "cyan"
state_open = "green"
state_failed = "red"

[[dash.pr.sections]]        # override/extend built-in sections
name = "Needs my review"
filter = "reviewer=@me state=OPEN"

[dash.keys]                 # override default bindings
merge = "M"
```

## Behavior & edge cases
- **Invalid/unknown config → warn to the status line and fall back to defaults; never
  crash.** Unknown keys ignored (forward-compat).
- Keymap overrides merge over defaults; conflicts reported in the help overlay.
- Theme maps names → `ratatui::style::Color`; unknown color name → default + warning.

## Test cases
- Parsing a `[dash]` block yields the expected sections/keymap/theme.
- Missing `[dash]` → full defaults; malformed value → default + warning, no panic.
- `?` toggles the overlay; overlay lists the active (possibly overridden) bindings.

## Out of scope
Per-host config. Live config reload (restart picks up changes).

## Next: spec 043 — custom keybinding → external command (stretch)
