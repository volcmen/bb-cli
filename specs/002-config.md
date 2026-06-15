# 002 Config storage

## Goal
Persist settings + per-host credentials; let env vars override.

## Command surface
No user command; infrastructure (`bb-config` crate) consumed by auth/api.

## Storage
- Dir resolution: `$BB_CONFIG_DIR` → `$XDG_CONFIG_HOME/bb` → `~/.config/bb`.
- `config.toml` — global (`default_host`, ...). `hosts.toml` — per-host map, written `0600` (unix).
- Host entry keys: `auth_type`, `username`, `token`, `refresh_token`, `oauth_client_id`.

## API
`ConfigProvider`: `get/set/unset_host/default_host/auth_token/hosts/save`.
`FileConfig` (file-backed, interior-mutable) + `EnvConfig` decorator (`BB_TOKEN`, `BB_HOST` win).
`load()` returns `Arc<dyn ConfigProvider>` = `EnvConfig(FileConfig)`.

## Behavior & edge cases
- Missing files → empty config (no error). `save()` on a blank (no-dir) config errors (never writes to CWD).
- Dotted host key `"bitbucket.org"` round-trips via quoted TOML key.

## Tests
Round-trip a host entry; `0600` perms; dir precedence (injected env getter); `BB_TOKEN`/`BB_HOST` override; dotted-key round-trip; multi-host isolation; `unset_host`.

## Next: spec 003 — API client (#12)
