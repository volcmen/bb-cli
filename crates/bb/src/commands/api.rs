//! `bb api` — make an authenticated Bitbucket API request and print the result.
//!
//! The `gh api` analog: an authenticated raw passthrough to the Bitbucket REST
//! API. Builds the `Authorization` header from stored config, sends the request
//! verbatim (no 2xx-only filtering), pretty-prints JSON responses, and maps an
//! HTTP status `>= 400` to a non-zero exit.

use crate::core::{Context, FlagError, Method, SilentError};
use clap::Args;
use serde_json::Value;

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// API path, e.g. `/user` or `/repositories/WS/SLUG` (a full URL also works)
    #[arg(value_name = "PATH")]
    pub path: String,
    /// HTTP method
    #[arg(short = 'X', long = "method", default_value = "GET")]
    pub method: String,
    /// Add a string field `key=value` to a JSON request body (repeatable)
    #[arg(short = 'f', long = "raw-field", value_name = "KEY=VALUE")]
    pub fields: Vec<String>,
    /// Add a typed field `key=value` (true/false/null/number parsed as JSON,
    /// else string) to the request body (repeatable)
    #[arg(short = 'F', long = "field", value_name = "KEY=VALUE")]
    pub typed: Vec<String>,
    /// Follow pagination, concatenating each page's `values` into one array
    #[arg(long)]
    pub paginate: bool,
    /// Filter the JSON response with a jq `expression`
    #[arg(short = 'q', long, value_name = "EXPRESSION")]
    pub jq: Option<String>,
    /// Format the JSON response with a `template`
    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,
}

impl ApiArgs {
    /// The output filter (jq/template) as a [`JsonFlags`], or `None` when neither
    /// is set. `bb api` has no `--json <fields>`, so the field list is empty and
    /// the filter operates on the full response.
    fn output_filter(&self) -> Option<crate::output::JsonFlags> {
        if self.jq.is_none() && self.template.is_none() {
            return None;
        }
        Some(crate::output::JsonFlags {
            json: Vec::new(),
            jq: self.jq.clone(),
            template: self.template.clone(),
        })
    }
}

/// Run `bb api`.
///
/// # Errors
/// Returns [`crate::core::AuthError`] when no credentials are stored,
/// [`FlagError`] for an unknown method / malformed `-f`/`-F` field / illegal flag
/// combination, and [`SilentError`] when the response status is `>= 400`.
pub fn run(ctx: &Context, args: ApiArgs) -> anyhow::Result<()> {
    let host = ctx.host();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(crate::core::AuthError::new(host).into());
    };
    let client = crate::api::BitbucketClient::new(ctx.transport.clone(), Some(header));

    let method = parse_method(&args.method)?;
    let body = build_body(&args.fields, &args.typed)?;

    if args.paginate {
        if method != Method::Get {
            return Err(FlagError::new("--paginate is only supported for GET requests").into());
        }
        if body.is_some() {
            return Err(FlagError::new("--paginate cannot be combined with -f/-F fields").into());
        }
        return run_paginate(ctx, &client, &args.path, args.output_filter().as_ref());
    }

    let resp = client.execute_raw(method, &args.path, body)?;
    emit_response(ctx, &resp.body, args.output_filter().as_ref())?;

    if resp.status >= 400 {
        return Err(SilentError.into());
    }
    Ok(())
}

/// Render a response body to stdout. With a jq/template `filter`, parse the body
/// as JSON and emit through it; otherwise pretty-print JSON (or print raw text).
fn emit_response(
    ctx: &Context,
    body: &[u8],
    filter: Option<&crate::output::JsonFlags>,
) -> anyhow::Result<()> {
    if let Some(filter) = filter {
        let value: Value = serde_json::from_slice(body)
            .map_err(|e| FlagError::new(format!("response is not JSON: {e}")))?;
        return filter.emit(&ctx.io, value);
    }
    // Pretty-print the body as JSON, falling back to the raw text.
    match serde_json::from_slice::<Value>(body) {
        Ok(value) => ctx.io.println(
            &serde_json::to_string_pretty(&value)
                .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned()),
        ),
        Err(_) => ctx.io.println(&String::from_utf8_lossy(body)),
    }
    Ok(())
}

/// Follow body-based pagination: GET each page, concatenate every page's
/// `values` array, and print the combined array once.
fn run_paginate(
    ctx: &Context,
    client: &crate::api::BitbucketClient,
    path: &str,
    filter: Option<&crate::output::JsonFlags>,
) -> anyhow::Result<()> {
    let mut all: Vec<Value> = Vec::new();
    // The first request uses the (possibly relative) `path`; subsequent ones use
    // the absolute `next` URL returned by Bitbucket.
    let resp = client.execute_raw(Method::Get, path, None)?;
    let mut next = collect_page(&resp.body, &mut all)?;
    while let Some(url) = next {
        let resp = client.execute_raw(Method::Get, &url, None)?;
        next = collect_page(&resp.body, &mut all)?;
    }
    let combined = Value::Array(all);
    if let Some(filter) = filter {
        return filter.emit(&ctx.io, combined);
    }
    ctx.io
        .println(&serde_json::to_string_pretty(&combined).unwrap_or_else(|_| combined.to_string()));
    Ok(())
}

/// Parse one page: append its `values` to `all` and return the `next` URL, if
/// any. A page that is not a JSON object with a `values` array is a hard error.
fn collect_page(body: &[u8], all: &mut Vec<Value>) -> anyhow::Result<Option<String>> {
    let page: Value = serde_json::from_slice(body)
        .map_err(|e| FlagError::new(format!("--paginate: response is not JSON: {e}")))?;
    let obj = page
        .as_object()
        .ok_or_else(|| FlagError::new("--paginate: response page is not a JSON object"))?;
    if let Some(values) = obj.get("values").and_then(Value::as_array) {
        all.extend(values.iter().cloned());
    }
    Ok(obj.get("next").and_then(Value::as_str).map(str::to_owned))
}

/// Map a case-insensitive method string onto a [`Method`].
fn parse_method(raw: &str) -> Result<Method, FlagError> {
    match raw.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::Get),
        "POST" => Ok(Method::Post),
        "PUT" => Ok(Method::Put),
        "DELETE" => Ok(Method::Delete),
        "PATCH" => Ok(Method::Patch),
        other => Err(FlagError::new(format!("unknown HTTP method: {other}"))),
    }
}

/// Build a JSON-object request body from `-f` (raw string) and `-F` (typed)
/// `key=value` repeats. Returns `None` when no fields were given. On a duplicate
/// key the typed value (inserted last) wins.
fn build_body(raw: &[String], typed: &[String]) -> Result<Option<Vec<u8>>, FlagError> {
    if raw.is_empty() && typed.is_empty() {
        return Ok(None);
    }
    let mut obj = serde_json::Map::with_capacity(raw.len() + typed.len());
    for field in raw {
        let (key, value) = split_field(field)?;
        obj.insert(key.to_owned(), Value::String(value.to_owned()));
    }
    for field in typed {
        let (key, value) = split_field(field)?;
        obj.insert(key.to_owned(), parse_typed_value(value));
    }
    let bytes = serde_json::to_vec(&Value::Object(obj))
        .map_err(|e| FlagError::new(format!("failed to encode request body: {e}")))?;
    Ok(Some(bytes))
}

/// Split a `KEY=VALUE` field, erroring if there is no `=`.
fn split_field(field: &str) -> Result<(&str, &str), FlagError> {
    field
        .split_once('=')
        .ok_or_else(|| FlagError::new(format!("invalid field (expected KEY=VALUE): {field}")))
}

/// Parse a `-F` value: JSON literals (`true`/`false`/`null`) and numbers become
/// the corresponding JSON value; anything else is sent as a string (the `gh -F`
/// rule). Arbitrary JSON objects/arrays are *not* parsed — pass those as raw.
fn parse_typed_value(raw: &str) -> Value {
    match raw {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }
    if let Ok(i) = raw.parse::<i64>() {
        return Value::Number(i.into());
    }
    if let Ok(u) = raw.parse::<u64>() {
        return Value::Number(u.into());
    }
    if let Ok(f) = raw.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(raw.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{AuthError, ConfigProvider, GitClient, Method, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "davidd").unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        Arc::new(cfg)
    }

    fn api_args(path: &str) -> ApiArgs {
        ApiArgs {
            path: path.to_owned(),
            method: "GET".to_owned(),
            fields: Vec::new(),
            typed: Vec::new(),
            paginate: false,
            jq: None,
            template: None,
        }
    }

    fn sent_body(h: &FakeTransport) -> Value {
        let reqs = h.requests.lock().unwrap();
        serde_json::from_slice(reqs[0].body.as_ref().expect("body present")).unwrap()
    }

    fn post_with(h: &Arc<FakeTransport>, raw: Vec<&str>, typed: Vec<&str>) -> ApiArgs {
        h.stub(
            "post typed",
            FakeTransport::rest(Method::Post, "/2.0/some/path"),
            FakeTransport::json(200, r#"{"ok":true}"#),
        );
        ApiArgs {
            path: "/some/path".to_owned(),
            method: "POST".to_owned(),
            fields: raw.into_iter().map(str::to_owned).collect(),
            typed: typed.into_iter().map(str::to_owned).collect(),
            paginate: false,
            jq: None,
            template: None,
        }
    }

    #[test]
    fn typed_field_parses_literals() {
        let h = Arc::new(FakeTransport::new());
        let args = post_with(
            &h,
            vec![],
            vec!["b=true", "f=false", "n=5", "z=null", "r=1.5"],
        );
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _b) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        run(&ctx, args).unwrap();
        assert_eq!(
            sent_body(&h),
            serde_json::json!({"b": true, "f": false, "n": 5, "z": null, "r": 1.5})
        );
    }

    #[test]
    fn typed_field_non_literal_is_string() {
        let h = Arc::new(FakeTransport::new());
        let args = post_with(&h, vec![], vec!["name=foo"]);
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _b) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        run(&ctx, args).unwrap();
        assert_eq!(sent_body(&h), serde_json::json!({"name": "foo"}));
    }

    #[test]
    fn raw_field_keeps_string_for_literal() {
        let h = Arc::new(FakeTransport::new());
        let args = post_with(&h, vec!["flag=true"], vec![]);
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _b) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        run(&ctx, args).unwrap();
        assert_eq!(sent_body(&h), serde_json::json!({"flag": "true"}));
    }

    #[test]
    fn raw_and_typed_merge() {
        let h = Arc::new(FakeTransport::new());
        let args = post_with(&h, vec!["a=x"], vec!["b=true"]);
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _b) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        run(&ctx, args).unwrap();
        assert_eq!(sent_body(&h), serde_json::json!({"a": "x", "b": true}));
    }

    #[test]
    fn malformed_typed_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _b) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        let args = ApiArgs {
            method: "POST".to_owned(),
            typed: vec!["novalue".to_owned()],
            ..api_args("/some/path")
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err:?}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn get_user_prints_pretty_json() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd","display_name":"David D"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, api_args("/user")).unwrap();

        let out = bufs.stdout_string();
        // Pretty-printed: indented, multi-line.
        assert!(out.contains("\"username\": \"davidd\""), "out: {out}");
        assert!(out.contains("\"display_name\": \"David D\""), "out: {out}");
        assert!(
            out.contains('\n'),
            "expected pretty (multi-line) output: {out}"
        );
    }

    #[test]
    fn fields_build_json_body_and_post_method() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "post fields",
            FakeTransport::rest(Method::Post, "/2.0/some/path"),
            FakeTransport::json(200, r#"{"ok":true}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        let args = ApiArgs {
            path: "/some/path".to_owned(),
            method: "POST".to_owned(),
            fields: vec!["a=b".to_owned(), "c=d".to_owned()],
            typed: Vec::new(),
            paginate: false,
            jq: None,
            template: None,
        };
        run(&ctx, args).unwrap();

        let reqs = h.requests.lock().unwrap();
        let req = &reqs[0];
        assert_eq!(req.method, Method::Post);
        let sent: Value = serde_json::from_slice(req.body.as_ref().expect("body present")).unwrap();
        assert_eq!(sent, serde_json::json!({"a": "b", "c": "d"}));
    }

    #[test]
    fn http_404_prints_body_and_returns_silent_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "404",
            FakeTransport::rest(Method::Get, "/2.0/missing"),
            FakeTransport::json(404, r#"{"type":"error","error":{"message":"not found"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        let err = run(&ctx, api_args("/missing")).unwrap_err();
        assert!(
            err.downcast_ref::<SilentError>().is_some(),
            "expected SilentError, got: {err:?}"
        );
        // The body is still shown despite the error.
        let out = bufs.stdout_string();
        assert!(out.contains("not found"), "out: {out}");
    }

    #[test]
    fn paginate_concatenates_values_across_pages() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "page 1",
            FakeTransport::rest(Method::Get, "/2.0/items"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":1},{"id":2}],"next":"https://api.bitbucket.org/2.0/items?page=2"}"#,
            ),
        );
        h.stub(
            "page 2",
            FakeTransport::rest(Method::Get, "items?page=2"),
            FakeTransport::json(200, r#"{"values":[{"id":3}]}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        let args = ApiArgs {
            paginate: true,
            ..api_args("/items")
        };
        run(&ctx, args).unwrap();

        let out = bufs.stdout_string();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let ids: Vec<u64> = parsed
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(h.request_count(), 2);
    }

    #[test]
    fn invalid_field_without_equals_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        // No stub registered: a malformed field must error before any request.
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        let args = ApiArgs {
            path: "/some/path".to_owned(),
            method: "POST".to_owned(),
            fields: vec!["novalue".to_owned()],
            typed: Vec::new(),
            paginate: false,
            jq: None,
            template: None,
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(
            err.downcast_ref::<FlagError>().is_some(),
            "expected FlagError, got: {err:?}"
        );
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn not_authenticated_returns_auth_error() {
        let h = Arc::new(FakeTransport::new());
        // No stub: AuthError must fire before any request.
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);

        let err = run(&ctx, api_args("/user")).unwrap_err();
        assert!(
            err.downcast_ref::<AuthError>().is_some(),
            "expected AuthError, got: {err:?}"
        );
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn unknown_method_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        let args = ApiArgs {
            method: "FETCH".to_owned(),
            ..api_args("/user")
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(
            err.downcast_ref::<FlagError>().is_some(),
            "expected FlagError, got: {err:?}"
        );
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn api_jq_filters_response() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "jq",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd","display_name":"David D"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, bufs) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        let args = ApiArgs {
            jq: Some(".username".to_owned()),
            ..api_args("/user")
        };
        run(&ctx, args).unwrap();
        assert_eq!(bufs.stdout_string(), "\"davidd\"\n");
    }

    #[test]
    fn api_template_renders_response() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "tmpl",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, bufs) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        let args = ApiArgs {
            template: Some("{username}".to_owned()),
            ..api_args("/user")
        };
        run(&ctx, args).unwrap();
        assert_eq!(bufs.stdout_string().trim_end(), "davidd");
    }

    #[test]
    fn api_jq_on_paginate() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "page 1",
            FakeTransport::rest(Method::Get, "/2.0/items"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":1},{"id":2}],"next":"https://api.bitbucket.org/2.0/items?page=2"}"#,
            ),
        );
        h.stub(
            "page 2",
            FakeTransport::rest(Method::Get, "items?page=2"),
            FakeTransport::json(200, r#"{"values":[{"id":3}]}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, bufs) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        let args = ApiArgs {
            paginate: true,
            jq: Some(".[].id".to_owned()),
            ..api_args("/items")
        };
        run(&ctx, args).unwrap();
        assert_eq!(bufs.stdout_string(), "1\n2\n3\n");
    }

    #[test]
    fn api_jq_non_json_body_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "non json",
            FakeTransport::rest(Method::Get, "/2.0/raw"),
            FakeTransport::json(200, "this is not json"),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _b) = test_context(
            transport,
            git(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        let args = ApiArgs {
            jq: Some(".".to_owned()),
            ..api_args("/raw")
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err:?}");
    }
}
