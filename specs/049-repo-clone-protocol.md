# 049 repo clone: `--protocol` override (OAuth-safe cloning)

Fixes #92.

## Goal / user story
`bb repo clone ws/slug` defaults to HTTPS, which prompts for a password and
fails for OAuth-authenticated users (no git credential helper). They need an
easy escape hatch: clone over SSH (which works with their key) without editing
config. Today the protocol is only selectable via the `git_protocol` config key
— there is no per-invocation flag.

Note: embedding the OAuth token into an HTTPS remote is intentionally **not**
done — OAuth access tokens expire (~2h), so a token baked into `.git/config`
would break later `git pull`. Making HTTPS "just work" via a credential helper
is the job of `bb auth setup-git` (#104). For now SSH is the recommended path.

## Command surface
`bb repo clone <ws/slug> [dir] [--protocol ssh|https]`

- `--protocol` overrides the `git_protocol` config for this invocation.
- Precedence: `--protocol` flag → `git_protocol` config → `https` default.
- Existing fallback to the other protocol (when the chosen one has no URL)
  is preserved.

## Bitbucket endpoint(s)
Unchanged — `GET /2.0/repositories/{ws}/{slug}` then `git clone <url>`.

## Behavior & edge cases
- `--protocol ssh` → SSH clone URL even when config says https (or is unset).
- `--protocol https` → HTTPS even when `git_protocol=ssh`.
- Invalid value rejected by clap (`value_parser`).
- No clone URL for either protocol → `FlagError` (unchanged).

## Test cases (red first)
- `clone_protocol_flag_overrides_config_to_ssh`: default config + `--protocol
  ssh` → ssh URL.
- `clone_protocol_flag_overrides_config_to_https`: `git_protocol=ssh` config +
  `--protocol https` → https URL.

## Out of scope
HTTPS credential helper / token auth (#104 `bb auth setup-git`). `--protocol`
on other commands.

## Next: spec 050 — #91 `bb pr edit`
