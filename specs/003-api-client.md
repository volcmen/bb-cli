# 003 API client foundation

## Goal
Typed Bitbucket REST client over a swappable transport, with body-based pagination.

## API
- `Transport` seam (bb-core): `execute(HttpRequest) -> Result<HttpResponse, ApiError>`.
- `ReqwestTransport` (blocking + rustls). `FakeTransport` (test-util) = in-process stub, Drop-asserts all stubs hit.
- `BitbucketClient::new(transport, auth_header: Option<String>)`: `get<T>`, `post<T,B>`, `send_empty`, `get_raw`, `paginate<T>(path, limit)`.
- Base URL `https://api.bitbucket.org/2.0`. Auth + Accept/Content-Type headers injected per request.

## Behavior & edge cases
- Non-2xx → `ApiError::Http{status,url,message,errors}`; message parsed from `{"error":{"message":...,"fields":...}}`, else generic.
- `paginate` follows body `next` (absolute URL) until `None`, stops early at `limit`.
- `get_raw` returns text (for diff/patch).

## Tests
get parses typed; 401/403/404/429/500 mapping + `is_unauthorized`/`is_not_found`; header injection; post body serialization; paginate multi-page + limit early-stop; get_raw text.

## Next: spec 004 — git remote → RepoId (#13)
