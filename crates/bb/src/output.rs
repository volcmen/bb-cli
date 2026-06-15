//! Machine-readable output: `--json <fields>`, `--jq <expr>`, `--template`.
//!
//! Commands flatten [`JsonFlags`] into their args. When `--json` is requested
//! they build a `serde_json::Value` of the full result and call
//! [`JsonFlags::emit`], which validates the requested fields against the
//! command's allowlist, projects to those fields, and then applies `--jq` (the
//! analog of `gh`'s `--jq`) or pretty-prints.

use bb_core::{FlagError, IoStreams};
use serde_json::{Map, Value};

/// Shared output flags, flattened into list/view commands.
#[derive(clap::Args, Debug, Default)]
pub struct JsonFlags {
    /// Output JSON with the given comma-separated `fields`
    #[arg(long, value_name = "FIELDS", value_delimiter = ',')]
    pub json: Vec<String>,
    /// Filter JSON output with a jq `expression` (implies `--json`)
    #[arg(long, short = 'q', value_name = "EXPRESSION")]
    pub jq: Option<String>,
    /// Format JSON output with a `template` (implies `--json`)
    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,
}

impl JsonFlags {
    /// Whether JSON output was requested.
    #[must_use]
    pub fn requested(&self) -> bool {
        !self.json.is_empty()
    }

    /// Validate the requested fields against `allowed`, and that `--jq` /
    /// `--template` are only used with `--json`.
    ///
    /// # Errors
    /// Returns [`FlagError`] for an unknown field or a misused flag.
    pub fn validate(&self, allowed: &[&str]) -> Result<(), FlagError> {
        if !self.requested() && (self.jq.is_some() || self.template.is_some()) {
            return Err(FlagError::new(
                "`--jq` and `--template` require `--json <fields>`",
            ));
        }
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

        if self.template.is_some() {
            return Err(FlagError::new("--template is not supported yet (#33)").into());
        }

        io.println(&serde_json::to_string_pretty(&projected)?);
        Ok(())
    }
}

/// Project a JSON value (array of objects, or a single object) down to `fields`.
fn project(value: Value, fields: &[String]) -> Value {
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

/// Apply a jq `expression` to `value`, returning the rendered output.
///
/// TODO(#32): implemented by the jaq integration. Kept as a stable seam so
/// commands can wire `--jq` now.
fn apply_jq(_value: &Value, _expr: &str) -> anyhow::Result<String> {
    anyhow::bail!("--jq is not wired yet (#32)")
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
    fn validate_jq_requires_json() {
        let f = JsonFlags {
            json: vec![],
            jq: Some(".".to_owned()),
            template: None,
        };
        assert!(f.validate(&["id"]).is_err());
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
}
