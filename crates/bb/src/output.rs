//! Machine-readable output: `--json <fields>`, `--jq <expr>`, `--template`.
//!
//! Commands flatten [`JsonFlags`] into their args. When `--json` is requested
//! they build a `serde_json::Value` of the full result and call
//! [`JsonFlags::emit`], which validates the requested fields against the
//! command's allowlist, projects to those fields, and then applies `--jq` (the
//! analog of `gh`'s `--jq`) or pretty-prints.

use crate::core::{FlagError, IoStreams};
use serde_json::{Map, Value};

/// Shared output flags, flattened into list/view commands.
#[derive(clap::Args, Debug, Default)]
pub struct JsonFlags {
    /// Output JSON with the given comma-separated `fields`
    #[arg(long, value_name = "FIELDS", value_delimiter = ',')]
    pub json: Vec<String>,
    /// Filter JSON output with a jq `expression` (implies `--json`; all fields)
    #[arg(long, short = 'q', value_name = "EXPRESSION")]
    pub jq: Option<String>,
    /// Format JSON output with a `template` (implies `--json`; all fields)
    ///
    /// Uses tinytemplate syntax, NOT Go templates: interpolate a value with
    /// single braces `{ field }`; control blocks use double braces
    /// `{{ for x in items }}…{{ endfor }}` / `{{ if … }}…{{ endif }}`. A
    /// top-level JSON array is exposed under the `items` key (tinytemplate
    /// cannot iterate a bare array), so iterate with
    /// `{{ for p in items }}…{{ endfor }}`. Example:
    /// `--template '{{ for p in items }}{ p.id } { p.title }
    /// {{ endfor }}'` (the newline inside the loop becomes a real line break).
    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,
}

impl JsonFlags {
    /// Whether JSON output was requested. `--jq` and `--template` imply
    /// `--json`, so any of the three selects JSON mode.
    #[must_use]
    pub fn requested(&self) -> bool {
        !self.json.is_empty() || self.jq.is_some() || self.template.is_some()
    }

    /// Validate the requested `--json` fields against `allowed`. `--jq` /
    /// `--template` need no explicit fields (they operate on the full object).
    ///
    /// # Errors
    /// Returns [`FlagError`] for an unknown `--json` field.
    pub fn validate(&self, allowed: &[&str]) -> Result<(), FlagError> {
        for field in &self.json {
            if !allowed.contains(&field.as_str()) {
                let mut valid = allowed.to_vec();
                valid.sort_unstable();
                return Err(FlagError::new(format!(
                    "unknown JSON field {field:?}; valid fields: {}",
                    valid.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// Project `value` to the requested fields and write the result (jq-filtered
    /// or pretty-printed) to stdout.
    ///
    /// # Errors
    /// Returns an error from jq evaluation or JSON serialization.
    pub fn emit(&self, io: &IoStreams, value: Value) -> anyhow::Result<()> {
        let projected = project(value, &self.json);

        if let Some(expr) = &self.jq {
            let out = apply_jq(&projected, expr)?;
            io.print(&out);
            if !out.ends_with('\n') {
                io.println("");
            }
            return Ok(());
        }

        if let Some(tmpl) = &self.template {
            let out = render_template(&projected, tmpl)?;
            io.print(&out);
            if !out.ends_with('\n') {
                io.println("");
            }
            return Ok(());
        }

        io.println(&serde_json::to_string_pretty(&projected)?);
        Ok(())
    }
}

/// Project a JSON value (array of objects, or a single object) down to `fields`.
/// An empty `fields` list means "no projection" — the full value is returned
/// unchanged, so `--jq`/`--template` without `--json` see every field.
fn project(value: Value, fields: &[String]) -> Value {
    if fields.is_empty() {
        return value;
    }
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|i| project_object(i, fields))
                .collect(),
        ),
        other => project_object(other, fields),
    }
}

fn project_object(value: Value, fields: &[String]) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for field in fields {
                if let Some(v) = map.get(field) {
                    out.insert(field.clone(), v.clone());
                }
            }
            Value::Object(out)
        }
        other => other,
    }
}

/// Render `value` through a [`tinytemplate`] `tmpl`, returning the output.
///
/// tinytemplate renders a Serialize context and supports `{ field }`,
/// `{{ for x in field }}…{{ endfor }}`, and `{{ if … }}`. It cannot iterate a
/// bare top-level array, so a projected **list** result is exposed to the
/// template under the key `items` (i.e. `{{ for x in items }}…`); an object is
/// rendered against directly. HTML-escaping is disabled so URLs and quotes pass
/// through unchanged.
///
/// # Errors
/// Returns a [`FlagError`] (`invalid template: …`) for a malformed template or
/// a render-time failure.
fn render_template(value: &Value, tmpl: &str) -> anyhow::Result<String> {
    let ctx = template_context(value);
    let mut tt = tinytemplate::TinyTemplate::new();
    tt.set_default_formatter(&tinytemplate::format_unescaped);
    tt.add_template("bb", tmpl)
        .map_err(|e| FlagError::new(format!("invalid template: {e}")))?;
    tt.render("bb", &ctx)
        .map_err(|e| FlagError::new(format!("invalid template: {e}")).into())
}

/// Build the context [`Value`] for [`render_template`]: arrays are wrapped as
/// `{ "items": <array> }` (tinytemplate can't iterate a bare top-level array);
/// any other value is passed through unchanged.
fn template_context(value: &Value) -> Value {
    match value {
        Value::Array(_) => {
            let mut map = Map::new();
            map.insert("items".to_owned(), value.clone());
            Value::Object(map)
        }
        other => other.clone(),
    }
}

/// Apply a jq `expression` to `value`, returning the rendered output.
///
/// Uses the pure-Rust [`jaq`](https://github.com/01mf02/jaq) engine. The
/// expression is parsed and compiled (parse/compile errors surface as
/// `invalid jq expression: ...`), then run against `value`. Each output value
/// is rendered as compact JSON on its own line, matching jq's default.
///
/// # Errors
/// Returns an error for an invalid expression or a runtime evaluation failure.
fn apply_jq(value: &Value, expr: &str) -> anyhow::Result<String> {
    use jaq_core::load::{Arena, File, Loader};
    use jaq_core::{data, unwrap_valr, Compiler, Ctx, Vars};
    use jaq_json::{read, Val};

    // Convert the serde_json input into jaq's value type by round-tripping
    // through compact JSON bytes (avoids the optional `serde` feature).
    let input_bytes = serde_json::to_vec(value)?;
    let input: Val = read::parse_single(&input_bytes)
        .map_err(|e| anyhow::anyhow!("could not read JSON input for jq: {e}"))?;

    let program = File {
        code: expr,
        path: (),
    };

    // Named filters (`keys`, `map`, …) from core + std + json.
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs());

    let loader = Loader::new(defs);
    let arena = Arena::default();

    let modules = loader
        .load(&arena, program)
        .map_err(|errs| anyhow::anyhow!("invalid jq expression: {}", format_load_errors(&errs)))?;

    let filter = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| {
            anyhow::anyhow!("invalid jq expression: {}", format_compile_errors(&errs))
        })?;

    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));

    let mut out = String::new();
    for result in filter.id.run((ctx, input)).map(unwrap_valr) {
        let val = result.map_err(|e| anyhow::anyhow!("jq evaluation failed: {e}"))?;
        if !out.is_empty() {
            out.push('\n');
        }
        // `Val`'s `Display` renders compact, uncolored JSON (jq's default).
        out.push_str(&val.to_string());
    }
    Ok(out)
}

/// Render jaq's per-file load (parse) errors into a single readable line.
fn format_load_errors<S, P>(errs: &jaq_core::load::Errors<S, P>) -> String
where
    S: core::fmt::Debug,
    P: core::fmt::Debug,
{
    errs.iter()
        .map(|(_file, err)| format!("{err:?}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Render jaq's per-file compile errors into a single readable line.
fn format_compile_errors<S, P>(
    errs: &jaq_core::load::Errors<S, P, Vec<jaq_core::compile::Error<S>>>,
) -> String
where
    S: core::fmt::Debug,
    P: core::fmt::Debug,
{
    errs.iter()
        .flat_map(|(_file, file_errs)| file_errs.iter().map(|e| format!("{e:?}")))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flags(fields: &[&str]) -> JsonFlags {
        JsonFlags {
            json: fields.iter().map(|s| (*s).to_owned()).collect(),
            jq: None,
            template: None,
        }
    }

    #[test]
    fn validate_rejects_unknown_field() {
        let f = flags(&["id", "bogus"]);
        let err = f.validate(&["id", "title"]).unwrap_err();
        assert!(err.to_string().contains("bogus"));
        assert!(err.to_string().contains("valid fields"));
    }

    #[test]
    fn requested_true_when_only_jq() {
        let f = JsonFlags {
            json: vec![],
            jq: Some(".".to_owned()),
            template: None,
        };
        assert!(f.requested(), "--jq alone should request JSON output");
    }

    #[test]
    fn requested_true_when_only_template() {
        let f = JsonFlags {
            json: vec![],
            jq: None,
            template: Some("{id}".to_owned()),
        };
        assert!(f.requested(), "--template alone should request JSON output");
    }

    #[test]
    fn validate_allows_jq_without_json_fields() {
        // `--jq`/`--template` now imply `--json` (no explicit fields required).
        let f = JsonFlags {
            json: vec![],
            jq: Some(".".to_owned()),
            template: None,
        };
        assert!(f.validate(&["id"]).is_ok());
    }

    #[test]
    fn project_empty_fields_returns_full_value() {
        let arr = json!([{"id": 1, "title": "a"}, {"id": 2, "title": "b"}]);
        assert_eq!(project(arr.clone(), &[]), arr);
        let obj = json!({"id": 1, "title": "a", "extra": true});
        assert_eq!(project(obj.clone(), &[]), obj);
    }

    #[test]
    fn emit_jq_without_json_fields_uses_full_object() {
        let (io, bufs) = IoStreams::test();
        let flags = JsonFlags {
            json: vec![],
            jq: Some(".[].title".to_owned()),
            template: None,
        };
        // No projection -> jq sees the full objects, incl. `title`.
        let v = json!([{"id": 1, "title": "x"}, {"id": 2, "title": "y"}]);
        flags.emit(&io, v).unwrap();
        assert_eq!(bufs.stdout_string(), "\"x\"\n\"y\"\n");
    }

    #[test]
    fn projects_array_to_fields() {
        let v = json!([{"id": 1, "title": "a", "extra": true}, {"id": 2, "title": "b"}]);
        let out = project(v, &["id".to_owned(), "title".to_owned()]);
        assert_eq!(
            out,
            json!([{"id": 1, "title": "a"}, {"id": 2, "title": "b"}])
        );
    }

    #[test]
    fn projects_single_object() {
        let v = json!({"id": 1, "title": "a", "extra": true});
        let out = project(v, &["id".to_owned()]);
        assert_eq!(out, json!({"id": 1}));
    }

    // ---- apply_jq (direct, same-module access to the private fn) ----

    #[test]
    fn jq_array_field_one_value_per_line() {
        let v = json!([{"id": 1}, {"id": 2}]);
        let out = apply_jq(&v, ".[].id").unwrap();
        assert_eq!(out, "1\n2");
    }

    #[test]
    fn jq_identity_on_object_is_compact_json() {
        let v = json!({"id": 1, "title": "a"});
        let out = apply_jq(&v, ".").unwrap();
        // compact, no spaces, one line
        assert_eq!(out, r#"{"id":1,"title":"a"}"#);
    }

    #[test]
    fn jq_pipe_extracts_field_per_element() {
        let v = json!([{"title": "x"}, {"title": "y"}]);
        let out = apply_jq(&v, ".[] | .title").unwrap();
        assert_eq!(out, "\"x\"\n\"y\"");
    }

    #[test]
    fn jq_map_builds_array() {
        let v = json!([{"id": 1}, {"id": 2}]);
        let out = apply_jq(&v, "map(.id)").unwrap();
        assert_eq!(out, "[1,2]");
    }

    #[test]
    fn jq_uses_stdlib_filter() {
        // `keys` comes from jaq-std; proves the std defs/funs are wired.
        let v = json!({"b": 2, "a": 1});
        let out = apply_jq(&v, "keys").unwrap();
        assert_eq!(out, r#"["a","b"]"#);
    }

    #[test]
    fn jq_invalid_expression_errors() {
        let v = json!([{"id": 1}]);
        let err = apply_jq(&v, ".[").unwrap_err();
        assert!(
            err.to_string().contains("invalid jq expression"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn jq_runtime_error_is_surfaced() {
        // Indexing a number with a string key is a runtime type error.
        let v = json!(1);
        let err = apply_jq(&v, ".foo").unwrap_err();
        assert!(
            err.to_string().contains("jq evaluation failed"),
            "unexpected error: {err}"
        );
    }

    // ---- end-to-end through JsonFlags::emit + IoStreams::test() ----

    #[test]
    fn emit_with_jq_writes_filtered_lines_and_trailing_newline() {
        let (io, bufs) = IoStreams::test();
        let flags = JsonFlags {
            json: vec!["id".to_owned()],
            jq: Some(".[].id".to_owned()),
            template: None,
        };
        let v = json!([{"id": 1, "extra": true}, {"id": 2}]);
        flags.emit(&io, v).unwrap();
        // projection keeps only `id`, jq pulls the ids; emit adds a final newline.
        assert_eq!(bufs.stdout_string(), "1\n2\n");
    }

    #[test]
    fn emit_with_jq_invalid_expression_errors() {
        let (io, _bufs) = IoStreams::test();
        let flags = JsonFlags {
            json: vec!["id".to_owned()],
            jq: Some(".[".to_owned()),
            template: None,
        };
        let v = json!([{"id": 1}]);
        let err = flags.emit(&io, v).unwrap_err();
        assert!(err.to_string().contains("invalid jq expression"));
    }

    // ---- render_template (direct, same-module access to the private fn) ----

    #[test]
    fn template_renders_object_fields() {
        let v = json!({"id": 1, "title": "x"});
        let out = render_template(&v, "#{id} {title}").unwrap();
        assert_eq!(out, "#1 x");
    }

    #[test]
    fn template_exposes_array_as_items() {
        let v = json!([{"id": 1}, {"id": 2}]);
        let out = render_template(&v, "{{ for i in items }}{i.id}\n{{ endfor }}").unwrap();
        assert!(out.contains('1'), "expected `1` in {out:?}");
        assert!(out.contains('2'), "expected `2` in {out:?}");
    }

    #[test]
    fn template_does_not_html_escape() {
        // The unescaped formatter must leave quotes/ampersands intact.
        let v = json!({"url": "https://x/?a=1&b=2", "q": "a\"b"});
        let out = render_template(&v, "{url} {q}").unwrap();
        assert_eq!(out, "https://x/?a=1&b=2 a\"b");
    }

    #[test]
    fn template_invalid_errors() {
        let v = json!({"id": 1});
        let err = render_template(&v, "{ unclosed").unwrap_err();
        assert!(
            err.to_string().contains("invalid template"),
            "unexpected error: {err}"
        );
    }

    // ---- end-to-end through JsonFlags::emit + IoStreams::test() ----

    #[test]
    fn emit_with_template_object_adds_trailing_newline() {
        let (io, bufs) = IoStreams::test();
        let flags = JsonFlags {
            json: vec!["id".to_owned(), "title".to_owned()],
            jq: None,
            template: Some("#{id} {title}".to_owned()),
        };
        let v = json!({"id": 1, "title": "x", "extra": true});
        flags.emit(&io, v).unwrap();
        // projection keeps id+title; emit adds a final newline for the terminal.
        assert_eq!(bufs.stdout_string(), "#1 x\n");
    }

    #[test]
    fn emit_with_template_array_exposed_as_items() {
        let (io, bufs) = IoStreams::test();
        let flags = JsonFlags {
            json: vec!["id".to_owned()],
            jq: None,
            template: Some("{{ for i in items }}{i.id}\n{{ endfor }}".to_owned()),
        };
        let v = json!([{"id": 1, "extra": true}, {"id": 2}]);
        flags.emit(&io, v).unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains('1') && out.contains('2'), "got {out:?}");
    }

    #[test]
    fn emit_with_template_invalid_errors() {
        let (io, _bufs) = IoStreams::test();
        let flags = JsonFlags {
            json: vec!["id".to_owned()],
            jq: None,
            template: Some("{ unclosed".to_owned()),
        };
        let v = json!({"id": 1});
        let err = flags.emit(&io, v).unwrap_err();
        assert!(err.to_string().contains("invalid template"));
    }
}
