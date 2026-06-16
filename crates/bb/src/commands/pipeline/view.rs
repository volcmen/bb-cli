//! `bb pipeline view` — show a pipeline's state, steps, and (optionally) logs.

use crate::api::models::{Pipeline, PipelineStep};
use crate::api::BitbucketClient;
use crate::core::{AuthError, ColorScheme, Context, FlagError};
use clap::Args;

use crate::auth;
use crate::render::sanitize;

/// JSON fields a pipeline can be projected to with `--json`.
const FIELDS: &[&str] = &["uuid", "build_number", "state", "target", "created_on"];

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Pipeline build number (or UUID)
    #[arg(value_name = "BUILD")]
    pub id: String,
    /// Also print each step's log
    #[arg(long)]
    pub log: bool,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb pipeline view`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// [`FlagError`] (exit 1) when the pipeline is not found, and propagates
/// [`ApiError`](crate::core::ApiError) from the lookups.
pub fn run(ctx: &Context, args: ViewArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // The Bitbucket pipelines endpoint canonically selects by UUID, but accepts
    // a build number at the same path; we pass the positional `BUILD` through
    // verbatim and map a 404 to a usage error pointing back at `list`.
    let base = format!(
        "/repositories/{}/{}/pipelines/{}",
        repo.workspace(),
        repo.slug(),
        args.id,
    );

    let pipeline: Pipeline = match client.get(&base) {
        Ok(pipeline) => pipeline,
        Err(e) if e.is_not_found() => {
            return Err(FlagError::new(format!(
                "pipeline {} not found (try `bb pipeline list`)",
                args.id
            ))
            .into());
        }
        Err(e) => return Err(e.into()),
    };

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&pipeline)?)?;
        return Ok(());
    }

    let color = ctx.io.is_stdout_tty();
    ctx.io
        .print(&render_pipeline(&pipeline, ctx.io.color_scheme(), color));

    // Steps live at `.../steps/` (trailing slash). Fetch all of them.
    let steps_path = format!("{base}/steps/");
    let steps: Vec<PipelineStep> = client.paginate(&steps_path, None)?;

    if steps.is_empty() {
        ctx.io.println("\nNo steps.");
    } else {
        ctx.io.println("\nSteps:");
        for step in &steps {
            ctx.io
                .println(&render_step(step, ctx.io.color_scheme(), color));
            if args.log {
                // Best-effort: a step without a uuid can't have its log fetched.
                if let Some(uuid) = step.uuid.as_deref() {
                    let log_path = format!("{base}/steps/{uuid}/log");
                    if let Ok(log) = client.get_raw(&log_path) {
                        ctx.io.print(&log);
                        if !log.ends_with('\n') {
                            ctx.io.println("");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Render a pipeline's header: build line plus a state · result line.
fn render_pipeline(p: &Pipeline, cs: ColorScheme, color: bool) -> String {
    let build = match p.build_number {
        Some(n) => format!("#{n}"),
        None => p.uuid.clone().unwrap_or_else(|| "?".to_owned()),
    };
    let mut out = format!("Pipeline {build}\n");

    let mut parts: Vec<String> = Vec::new();
    let state = p.state_name();
    if !state.is_empty() {
        parts.push(state.to_owned());
    }
    let result = p.result_name();
    if !result.is_empty() {
        parts.push(if color {
            color_result(cs, result)
        } else {
            result.to_owned()
        });
    }
    if !parts.is_empty() {
        out.push_str(&parts.join(" · "));
        out.push('\n');
    }

    if let Some(ref_name) = p.target.as_ref().and_then(|t| t.ref_name.as_deref()) {
        out.push_str(&format!("Ref: {}\n", sanitize(ref_name)));
    }
    out
}

/// Render one step: `- <name>  <state> · <result>`.
fn render_step(step: &PipelineStep, cs: ColorScheme, color: bool) -> String {
    let name = sanitize(step.name.as_deref().unwrap_or("(unnamed)"));
    let state = step
        .state
        .as_ref()
        .and_then(|s| s.name.as_deref())
        .unwrap_or("");
    let result = step
        .state
        .as_ref()
        .and_then(|s| s.result.as_ref())
        .and_then(|r| r.name.as_deref())
        .unwrap_or("");

    let mut status: Vec<String> = Vec::new();
    if !state.is_empty() {
        status.push(state.to_owned());
    }
    if !result.is_empty() {
        status.push(if color {
            color_result(cs, result)
        } else {
            result.to_owned()
        });
    }

    if status.is_empty() {
        format!("- {name}")
    } else {
        format!("- {name}  {}", status.join(" · "))
    }
}

/// Color a pipeline/step result: SUCCESSFUL green, FAILED/ERROR red, else gray.
fn color_result(cs: ColorScheme, result: &str) -> String {
    match result {
        "SUCCESSFUL" => cs.green(result),
        "FAILED" | "ERROR" => cs.red(result),
        other => cs.gray(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Method, RepoId, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    /// A git client that errors on every call — `repo_override` makes
    /// `base_repo()` skip git, so the tests never actually shell out.
    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "u").unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn args(id: &str, log: bool) -> ViewArgs {
        ViewArgs {
            id: id.to_owned(),
            log,
            json: crate::output::JsonFlags::default(),
        }
    }

    const PIPELINE_12: &str = r#"{
        "uuid": "{aaa}",
        "build_number": 12,
        "state": {"name": "COMPLETED", "result": {"name": "SUCCESSFUL"}},
        "target": {"ref_name": "main"},
        "created_on": "2026-06-15T10:00:00Z"
    }"#;

    const TWO_STEPS: &str = r#"{
        "values": [
            {"uuid": "{s1}", "name": "Build", "state": {"name": "COMPLETED", "result": {"name": "SUCCESSFUL"}}},
            {"uuid": "{s2}", "name": "Test", "state": {"name": "COMPLETED", "result": {"name": "FAILED"}}}
        ]
    }"#;

    #[test]
    fn view_renders_state_and_steps() {
        let h = Arc::new(FakeTransport::new());
        // Order matters: the bare pipeline GET is registered before the steps GET
        // so the steps URL (which also contains `/pipelines/12`) doesn't shadow it.
        h.stub(
            "get steps",
            FakeTransport::rest(Method::Get, "/pipelines/12/steps/"),
            FakeTransport::json(200, TWO_STEPS),
        );
        h.stub(
            "get pipeline 12",
            FakeTransport::rest(Method::Get, "/pipelines/12"),
            FakeTransport::json(200, PIPELINE_12),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("12", false)).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("Pipeline #12"), "out: {out}");
        assert!(out.contains("COMPLETED"), "out: {out}");
        assert!(out.contains("SUCCESSFUL"), "out: {out}");
        assert!(out.contains("Ref: main"), "out: {out}");
        assert!(out.contains("Steps:"), "out: {out}");
        assert!(out.contains("- Build"), "out: {out}");
        assert!(out.contains("- Test"), "out: {out}");
        assert!(out.contains("FAILED"), "out: {out}");
    }

    #[test]
    fn view_no_steps_prints_message() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get steps empty",
            FakeTransport::rest(Method::Get, "/pipelines/12/steps/"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        h.stub(
            "get pipeline 12",
            FakeTransport::rest(Method::Get, "/pipelines/12"),
            FakeTransport::json(200, PIPELINE_12),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("12", false)).unwrap();
        assert!(bufs.stdout_string().contains("No steps."));
    }

    #[test]
    fn view_log_prints_each_step_log() {
        let h = Arc::new(FakeTransport::new());
        // Logs first so their URL doesn't get captured by the steps/pipeline stubs.
        h.stub(
            "log s1",
            FakeTransport::rest(Method::Get, "/steps/{s1}/log"),
            FakeTransport::text(200, "build log line\n"),
        );
        h.stub(
            "log s2",
            FakeTransport::rest(Method::Get, "/steps/{s2}/log"),
            FakeTransport::text(200, "test log line\n"),
        );
        h.stub(
            "get steps",
            FakeTransport::rest(Method::Get, "/pipelines/12/steps/"),
            FakeTransport::json(200, TWO_STEPS),
        );
        h.stub(
            "get pipeline 12",
            FakeTransport::rest(Method::Get, "/pipelines/12"),
            FakeTransport::json(200, PIPELINE_12),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("12", true)).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("build log line"), "out: {out}");
        assert!(out.contains("test log line"), "out: {out}");
    }

    #[test]
    fn view_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        // --json short-circuits before steps are fetched, so only the GET runs.
        h.stub(
            "get pipeline 12 json",
            FakeTransport::rest(Method::Get, "/pipelines/12"),
            FakeTransport::json(200, PIPELINE_12),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ViewArgs {
            id: "12".to_owned(),
            log: false,
            json: crate::output::JsonFlags {
                json: vec!["uuid".into(), "build_number".into(), "target".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["uuid"], "{aaa}");
        assert_eq!(v["build_number"], 12);
        assert_eq!(v["target"]["ref_name"], "main");
        // Unrequested fields are projected away.
        assert!(v.get("state").is_none(), "out: {out}");
        // No steps were fetched (only one request).
        assert_eq!(h.request_count(), 1);
    }

    #[test]
    fn view_json_unknown_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pipeline 12 json bogus",
            FakeTransport::rest(Method::Get, "/pipelines/12"),
            FakeTransport::json(200, PIPELINE_12),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ViewArgs {
            id: "12".to_owned(),
            log: false,
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn view_not_found_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pipeline 404",
            FakeTransport::rest(Method::Get, "/pipelines/99"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"No such pipeline."}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, args("99", false)).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(
            err.to_string()
                .contains("pipeline 99 not found (try `bb pipeline list`)"),
            "msg: {err}"
        );
    }

    #[test]
    fn view_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (mut ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, args("12", false)).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn render_pipeline_colors_result_when_enabled() {
        let (mut io, _) = crate::core::IoStreams::test();
        io.set_stdout_tty(true);
        let cs = io.color_scheme();
        let p: Pipeline = serde_json::from_str(PIPELINE_12).unwrap();
        let out = render_pipeline(&p, cs, true);
        assert!(out.contains("\x1b[32mSUCCESSFUL"), "out: {out}");
    }
}
