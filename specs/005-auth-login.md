# 005 bb auth login

## Goal
Authenticate against a Bitbucket host via Basic (token paste) or OAuth 2.0.

## Command surface
`bb auth login [--hostname H] [--web] [--with-token] [--username U] [--auth-type api_token|app_password]`
Exit: 0 success; 1 FlagError (bad/missing inputs, invalid creds); 2 CancelError; 4 propagated.

## Endpoints
- Validate: `GET /2.0/user` (Basic or Bearer).
- OAuth: authorize `https://bitbucket.org/site/oauth2/authorize`; exchange `POST .../access_token` (form, Basic consumer auth).

## Behavior & edge cases
- Basic: resolve auth_type/username/secret (flags or prompts; `--with-token` reads stdin). Reject empty secret. Validate via `/user` BEFORE persisting; 401 → "invalid credentials", nothing saved. Store auth_type/username/token; `save()`.
- OAuth (`--web`): Cloud-only (reject other hosts). Needs `BB_OAUTH_CLIENT_ID`/`BB_OAUTH_CLIENT_SECRET` (no shippable public consumer) — clear FlagError if unset. Local `127.0.0.1:0` callback; random hex `state` validated on redirect (CSRF); `client_id`/`redirect_uri` url-encoded consistently. Store oauth token + refresh_token + client_id.
- Non-interactive without inputs → FlagError.

## Tests
Basic happy (interactive, token persisted); invalid creds not saved; empty secret rejected; non-interactive missing → FlagError; OAuth exchange+store (FakeTransport) asserts form body + Basic header; state/code query parsing; non-Cloud `--web` rejected.

## VERIFY
OAuth consumer creds via env for MVP — revisit a bundled consumer later.

## Next: spec 006 — auth status (#15)
