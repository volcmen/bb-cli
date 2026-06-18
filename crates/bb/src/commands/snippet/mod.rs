//! `bb snippet` — Bitbucket Snippets CRUD (the `gh gist` analog).
//!
//! Snippets are workspace-scoped git repos. They are addressed as `WORKSPACE/ID`
//! (or a bare `ID` plus `--workspace`). Create/edit use `multipart/form-data`
//! where each file's form-field name is its filename, per Bitbucket's API.

use std::path::Path;

use crate::api::client::MultipartPart;
use crate::api::models::Snippet;
use crate::api::BitbucketClient;
use crate::core::{AuthError, CancelError, Context, FlagError, Method};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct SnippetArgs {
    #[command(subcommand)]
    command: SnippetCommands,
}

#[derive(Subcommand, Debug)]
enum SnippetCommands {
    /// Create a snippet from one or more files
    Create(CreateArgs),
    /// List snippets
    List(ListArgs),
    /// View a snippet
    View(ViewArgs),
    /// Edit a snippet (replace files / change the title)
    Edit(EditArgs),
    /// Delete a snippet
    Delete(DeleteArgs),
    /// Clone a snippet's git repository
    Clone(CloneArgs),
}

/// Dispatch `bb snippet <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: SnippetArgs) -> anyhow::Result<()> {
    match args.command {
        SnippetCommands::Create(a) => create(ctx, a),
        SnippetCommands::List(a) => list(ctx, a),
        SnippetCommands::View(a) => view(ctx, a),
        SnippetCommands::Edit(a) => edit(ctx, a),
        SnippetCommands::Delete(a) => delete(ctx, a),
        SnippetCommands::Clone(a) => clone(ctx, a),
    }
}

// ----- shared helpers ----------------------------------------------------

fn client_for(ctx: &Context) -> anyhow::Result<(BitbucketClient, String)> {
    let host = ctx.config.default_host();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    Ok((
        BitbucketClient::new(ctx.transport.clone(), Some(header)),
        host,
    ))
}

/// Parse a snippet reference: `WORKSPACE/ID`, or a bare `ID` with `--workspace`.
fn parse_ref(arg: &str, workspace: Option<&str>) -> anyhow::Result<(String, String)> {
    if let Some((ws, id)) = arg.split_once('/') {
        if !ws.is_empty() && !id.is_empty() && !id.contains('/') {
            return Ok((ws.to_owned(), id.to_owned()));
        }
    }
    if let Some(ws) = workspace {
        return Ok((ws.to_owned(), arg.to_owned()));
    }
    Err(FlagError::new(format!(
        "specify the snippet as WORKSPACE/ID, or pass --workspace (got {arg:?})"
    ))
    .into())
}

/// Read each path into a file [`MultipartPart`] keyed by its base filename.
fn file_parts(files: &[String]) -> anyhow::Result<Vec<MultipartPart>> {
    let mut parts = Vec::with_capacity(files.len());
    for path in files {
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| FlagError::new(format!("invalid file path: {path}")))?
            .to_owned();
        let bytes = std::fs::read(path)?;
        parts.push(MultipartPart::file(name, bytes));
    }
    Ok(parts)
}

// ----- create ------------------------------------------------------------

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Files to include in the snippet
    #[arg(value_name = "FILES", required = true)]
    pub files: Vec<String>,
    /// Snippet title
    #[arg(long, short = 't')]
    pub title: Option<String>,
    /// Make the snippet private
    #[arg(long)]
    pub private: bool,
    /// Create in this workspace (default: your personal snippets)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
}

fn create(ctx: &Context, args: CreateArgs) -> anyhow::Result<()> {
    if args.files.is_empty() {
        return Err(FlagError::new("provide at least one file to create a snippet").into());
    }
    let (client, _host) = client_for(ctx)?;

    let mut parts = vec![MultipartPart::field(
        "is_private",
        if args.private { "true" } else { "false" },
    )];
    if let Some(title) = &args.title {
        parts.push(MultipartPart::field("title", title.clone()));
    }
    parts.extend(file_parts(&args.files)?);

    let path = match &args.workspace {
        Some(ws) => format!("/snippets/{ws}"),
        None => "/snippets".to_owned(),
    };
    let snippet: Snippet = client.send_multipart(Method::Post, &path, &parts)?;

    ctx.io.println(&format!(
        "✓ Created snippet{}",
        snippet
            .id
            .as_deref()
            .map(|id| format!(" {id}"))
            .unwrap_or_default()
    ));
    if let Some(url) = snippet.html_url() {
        ctx.io.println(url);
    }
    Ok(())
}

// ----- list --------------------------------------------------------------

#[derive(Args, Debug)]
pub struct ListArgs {
    /// List a workspace's snippets (default: your own)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
}

fn list(ctx: &Context, args: ListArgs) -> anyhow::Result<()> {
    let (client, _host) = client_for(ctx)?;
    let path = match &args.workspace {
        Some(ws) => format!("/snippets/{ws}"),
        None => "/snippets".to_owned(),
    };
    let snippets: Vec<Snippet> = client.paginate(&path, Some(args.limit))?;

    if snippets.is_empty() {
        ctx.io.println("No snippets found");
        return Ok(());
    }
    for s in &snippets {
        let id = s.id.as_deref().unwrap_or("?");
        let privacy = if s.is_private == Some(true) {
            "private"
        } else {
            "public"
        };
        let title = s.title.as_deref().unwrap_or("");
        ctx.io.println(&format!("{id}\t{privacy}\t{title}"));
    }
    Ok(())
}

// ----- view --------------------------------------------------------------

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// The snippet as WORKSPACE/ID (or ID with --workspace)
    #[arg(value_name = "SNIPPET")]
    pub snippet: String,
    /// The workspace (if SNIPPET is a bare id)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Open the snippet in the browser instead of printing it
    #[arg(long)]
    pub web: bool,
}

fn view(ctx: &Context, args: ViewArgs) -> anyhow::Result<()> {
    let (ws, id) = parse_ref(&args.snippet, args.workspace.as_deref())?;
    let (client, _host) = client_for(ctx)?;
    let snippet: Snippet = client.get(&format!("/snippets/{ws}/{id}"))?;

    if args.web {
        if let Some(url) = snippet.html_url() {
            ctx.io.println(&format!("Opening {url} in your browser."));
            ctx.browser.browse(url)?;
        }
        return Ok(());
    }

    ctx.io.println(&format!(
        "{} ({})",
        snippet.title.as_deref().unwrap_or("(untitled)"),
        if snippet.is_private == Some(true) {
            "private"
        } else {
            "public"
        }
    ));
    for name in snippet.filenames() {
        ctx.io.println(&format!("  {name}"));
    }
    if let Some(url) = snippet.html_url() {
        ctx.io.println(url);
    }
    Ok(())
}

// ----- edit --------------------------------------------------------------

#[derive(Args, Debug)]
pub struct EditArgs {
    /// The snippet as WORKSPACE/ID (or ID with --workspace)
    #[arg(value_name = "SNIPPET")]
    pub snippet: String,
    /// The workspace (if SNIPPET is a bare id)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Files to add or replace
    #[arg(value_name = "FILES")]
    pub files: Vec<String>,
    /// New title
    #[arg(long, short = 't')]
    pub title: Option<String>,
}

fn edit(ctx: &Context, args: EditArgs) -> anyhow::Result<()> {
    if args.files.is_empty() && args.title.is_none() {
        return Err(FlagError::new("nothing to update; pass files and/or --title").into());
    }
    let (ws, id) = parse_ref(&args.snippet, args.workspace.as_deref())?;
    let (client, _host) = client_for(ctx)?;

    let mut parts = Vec::new();
    if let Some(title) = &args.title {
        parts.push(MultipartPart::field("title", title.clone()));
    }
    parts.extend(file_parts(&args.files)?);

    let snippet: Snippet =
        client.send_multipart(Method::Put, &format!("/snippets/{ws}/{id}"), &parts)?;
    ctx.io.println(&format!("✓ Updated snippet {ws}/{id}"));
    if let Some(url) = snippet.html_url() {
        ctx.io.println(url);
    }
    Ok(())
}

// ----- delete ------------------------------------------------------------

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The snippet as WORKSPACE/ID (or ID with --workspace)
    #[arg(value_name = "SNIPPET")]
    pub snippet: String,
    /// The workspace (if SNIPPET is a bare id)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Skip the confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

fn delete(ctx: &Context, args: DeleteArgs) -> anyhow::Result<()> {
    let (ws, id) = parse_ref(&args.snippet, args.workspace.as_deref())?;

    if !args.yes {
        if !ctx.io.can_prompt() {
            return Err(FlagError::new(format!(
                "refusing to delete {ws}/{id} without confirmation; pass --yes"
            ))
            .into());
        }
        let ok = ctx
            .prompter
            .confirm(&format!("Delete snippet {ws}/{id}?"), false)
            .map_err(to_anyhow)?;
        if !ok {
            return Err(CancelError.into());
        }
    }

    let (client, _host) = client_for(ctx)?;
    client.send_empty(Method::Delete, &format!("/snippets/{ws}/{id}"))?;
    ctx.io.println(&format!("✓ Deleted snippet {ws}/{id}"));
    Ok(())
}

// ----- clone -------------------------------------------------------------

#[derive(Args, Debug)]
pub struct CloneArgs {
    /// The snippet as WORKSPACE/ID (or ID with --workspace)
    #[arg(value_name = "SNIPPET")]
    pub snippet: String,
    /// The workspace (if SNIPPET is a bare id)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Directory to clone into
    #[arg(value_name = "DIR")]
    pub dir: Option<String>,
}

fn clone(ctx: &Context, args: CloneArgs) -> anyhow::Result<()> {
    let (ws, id) = parse_ref(&args.snippet, args.workspace.as_deref())?;
    let (client, _host) = client_for(ctx)?;
    let snippet: Snippet = client.get(&format!("/snippets/{ws}/{id}"))?;

    let protocol = ctx
        .config
        .get("", "git_protocol")
        .unwrap_or_else(|| "https".to_owned());
    let fallback = if protocol == "ssh" { "https" } else { "ssh" };
    let url = snippet
        .clone_url(&protocol)
        .or_else(|| snippet.clone_url(fallback))
        .ok_or_else(|| FlagError::new(format!("snippet {ws}/{id} has no clone URL")))?;

    ctx.git.clone_repo(url, args.dir.as_deref())?;
    ctx.io.println(&format!("✓ Cloned snippet {ws}/{id}"));
    Ok(())
}

fn to_anyhow(err: crate::core::PromptError) -> anyhow::Error {
    match err {
        crate::core::PromptError::Cancelled => CancelError.into(),
        other => anyhow::anyhow!(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{Browser, ConfigProvider, Context, GitClient, Method, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, RecordingBrowser, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn authed() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "u").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        test_context(
            transport,
            git(),
            cfg,
            Arc::new(ScriptedPrompter::new()),
            true,
        )
    }

    // ----- parse_ref -----------------------------------------------------

    #[test]
    fn parse_ref_accepts_ws_slash_id_and_flag() {
        assert_eq!(
            parse_ref("acme/abc123", None).unwrap(),
            ("acme".to_owned(), "abc123".to_owned())
        );
        assert_eq!(
            parse_ref("abc123", Some("acme")).unwrap(),
            ("acme".to_owned(), "abc123".to_owned())
        );
    }

    #[test]
    fn parse_ref_bare_id_without_workspace_errors() {
        let err = parse_ref("abc123", None).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    // ----- create --------------------------------------------------------

    #[test]
    fn create_posts_multipart_with_files() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create snippet",
            FakeTransport::rest(Method::Post, "/snippets"),
            FakeTransport::json(
                201,
                r#"{"id":"xyz","links":{"html":{"href":"https://bitbucket.org/snippets/u/xyz"}}}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, "hi there").unwrap();

        create(
            &ctx,
            CreateArgs {
                files: vec![file.to_string_lossy().into_owned()],
                title: Some("My snippet".to_owned()),
                private: true,
                workspace: None,
            },
        )
        .unwrap();

        // Inspect the recorded multipart request.
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let ctype = post.headers.get("Content-Type").unwrap();
        assert!(
            ctype.starts_with("multipart/form-data; boundary="),
            "got {ctype}"
        );
        let body = String::from_utf8_lossy(post.body.as_deref().unwrap());
        assert!(body.contains("name=\"hello.txt\"; filename=\"hello.txt\""));
        assert!(body.contains("hi there"));
        assert!(body.contains("name=\"is_private\""));
        assert!(body.contains("name=\"title\""));

        assert!(bufs.stdout_string().contains("✓ Created snippet xyz"));
    }

    #[test]
    fn create_workspace_targets_workspace_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create in ws",
            FakeTransport::rest(Method::Post, "/snippets/acme"),
            FakeTransport::json(201, r#"{"id":"w1"}"#),
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();

        create(
            &ctx,
            CreateArgs {
                files: vec![file.to_string_lossy().into_owned()],
                title: None,
                private: false,
                workspace: Some("acme".to_owned()),
            },
        )
        .unwrap();

        let reqs = h.requests.lock().unwrap();
        assert!(reqs.iter().any(|r| r.url.contains("/snippets/acme")));
    }

    #[test]
    fn create_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, Arc::new(FileConfig::blank()));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let err = create(
            &ctx,
            CreateArgs {
                files: vec![file.to_string_lossy().into_owned()],
                title: None,
                private: false,
                workspace: None,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }

    // ----- list ----------------------------------------------------------

    #[test]
    fn list_renders_rows() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/snippets"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"id":"a1","title":"First","is_private":true},
                    {"id":"b2","title":"Second","is_private":false}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        list(
            &ctx,
            ListArgs {
                workspace: None,
                limit: 30,
            },
        )
        .unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("a1\tprivate\tFirst"), "got: {out}");
        assert!(out.contains("b2\tpublic\tSecond"), "got: {out}");
    }

    // ----- view ----------------------------------------------------------

    #[test]
    fn view_prints_title_and_files() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get",
            FakeTransport::rest(Method::Get, "/snippets/acme/xyz"),
            FakeTransport::json(
                200,
                r#"{"id":"xyz","title":"Demo","is_private":false,
                    "files":{"a.txt":{},"b.txt":{}},
                    "links":{"html":{"href":"https://bitbucket.org/snippets/acme/xyz"}}}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        view(
            &ctx,
            ViewArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                web: false,
            },
        )
        .unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("Demo (public)"), "got: {out}");
        assert!(out.contains("a.txt") && out.contains("b.txt"));
    }

    #[test]
    fn view_web_opens_browser() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get",
            FakeTransport::rest(Method::Get, "/snippets/acme/xyz"),
            FakeTransport::json(
                200,
                r#"{"id":"xyz","links":{"html":{"href":"https://bitbucket.org/snippets/acme/xyz"}}}"#,
            ),
        );
        let browser = Arc::new(RecordingBrowser::default());
        let (io, _bufs) = crate::core::IoStreams::test();
        let transport: Arc<dyn Transport> = h;
        let browser_dyn: Arc<dyn Browser> = browser.clone();
        let ctx = Context {
            io: Arc::new(io),
            prompter: Arc::new(ScriptedPrompter::new()),
            browser: browser_dyn,
            git: git(),
            config: authed(),
            transport,
            app_version: "test".to_owned(),
            repo_override: None,
        };
        view(
            &ctx,
            ViewArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                web: true,
            },
        )
        .unwrap();
        assert_eq!(
            browser.urls.lock().unwrap().last().unwrap(),
            "https://bitbucket.org/snippets/acme/xyz"
        );
    }

    // ----- edit ----------------------------------------------------------

    #[test]
    fn edit_nothing_to_change_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, authed());
        let err = edit(
            &ctx,
            EditArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                files: Vec::new(),
                title: None,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn edit_title_puts_multipart() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "put",
            FakeTransport::rest(Method::Put, "/snippets/acme/xyz"),
            FakeTransport::json(200, r#"{"id":"xyz"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed());
        edit(
            &ctx,
            EditArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                files: Vec::new(),
                title: Some("Renamed".to_owned()),
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let put = reqs.iter().find(|r| r.method == Method::Put).unwrap();
        let body = String::from_utf8_lossy(put.body.as_deref().unwrap());
        assert!(body.contains("name=\"title\""));
        assert!(body.contains("Renamed"));
        assert!(bufs.stdout_string().contains("✓ Updated snippet acme/xyz"));
    }

    // ----- delete --------------------------------------------------------

    #[test]
    fn delete_yes_skips_prompt() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "delete",
            FakeTransport::rest(Method::Delete, "/snippets/acme/xyz"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        delete(
            &ctx,
            DeleteArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                yes: true,
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted snippet acme/xyz"));
    }

    #[test]
    fn delete_declined_is_cancel() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h;
        // Build an interactive context (can_prompt() == true) so the confirm runs.
        let (mut io, _bufs) = crate::core::IoStreams::test();
        io.set_stdin_tty(true);
        io.set_stdout_tty(true);
        io.set_never_prompt(false);
        let ctx = Context {
            io: Arc::new(io),
            prompter: Arc::new(ScriptedPrompter::new().confirm(false)),
            browser: Arc::new(RecordingBrowser::default()),
            git: git(),
            config: authed(),
            transport,
            app_version: "test".to_owned(),
            repo_override: None,
        };
        let err = delete(
            &ctx,
            DeleteArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                yes: false,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<CancelError>().is_some(), "got: {err}");
    }

    // ----- clone ---------------------------------------------------------

    #[test]
    fn clone_gets_then_git_clones() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get",
            FakeTransport::rest(Method::Get, "/snippets/acme/xyz"),
            FakeTransport::json(
                200,
                r#"{"id":"xyz","links":{"clone":[
                    {"name":"https","href":"https://bitbucket.org/snippets/acme/xyz.git"},
                    {"name":"ssh","href":"git@bitbucket.org:snippets/acme/xyz.git"}
                ]}}"#,
            ),
        );
        let stub = Arc::new(StubRunner::new());
        stub.register(
            r"^git clone -- https://bitbucket\.org/snippets/acme/xyz\.git$",
            0,
            "",
        );
        let transport: Arc<dyn Transport> = h;
        let (ctx, bufs) = test_context(
            transport,
            Arc::new(ShellGit::new(stub)),
            authed(),
            Arc::new(ScriptedPrompter::new()),
            true,
        );
        clone(
            &ctx,
            CloneArgs {
                snippet: "acme/xyz".to_owned(),
                workspace: None,
                dir: None,
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Cloned snippet acme/xyz"));
    }
}
