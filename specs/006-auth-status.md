# 006 bb auth status

## Goal
Show which hosts are authenticated and as whom.

## Command surface
`bb auth status [--hostname H]`. Exit 0 if all checked hosts authenticate; 4 if none configured or any fails.

## Endpoint
`GET /2.0/user` per host using the stored credential header.

## Behavior & edge cases
- No hosts (or none with creds) → message + AuthError (exit 4).
- Per host: success → "✓ Logged in to {host} as {label}"; missing creds → "X {host}: not logged in"; 401 → "X {host}: authentication failed".
- Any failure → AuthError.

## Tests
Logged-in label; no-hosts → AuthError; host without creds; invalid token → AuthError.

## Next: spec 007 — pr create (#16)
