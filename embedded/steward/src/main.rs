//! steward - a platform-engineering agent that keeps a service's estate on
//! its golden path, across providers. Act 3 companion example, not in the
//! talk flow: the human-in-the-loop write path (safe mode, MCP elicitation
//! answered at the terminal) that sre-agent-sidecar and finops-agent-vendored
//! leave out.
//!
//! An embedded StackQL MCP server (sidecar by default, vendored straight into
//! this binary with `--features vendored`) wired into a rig agent. The
//! agent's tools are the StackQL MCP tools; the policy is a markdown file.
//!
//! - `steward check`  reads actual state, reports drift, proposes SQL fixes
//!   (server in read_only mode: writes are refused whatever the model does)
//! - `steward fix`    same, then remediates; the server runs in safe mode and
//!   asks for approval before every write, which this binary puts to the
//!   human at the terminal (--mode delete_safe: only deletes ask)
//! - `steward ask`    a free-form read-only question
//! - `steward sql`    run one statement through the embedded server directly
//!   (no model): the deterministic fallback, and the quickest way to watch
//!   the approval prompt fire (`--write`)
//! - `steward preflight`  start the server, list tools, no model calls
//!
//! Needs ANTHROPIC_API_KEY for anything that calls the model. Tool-call
//! traces and diagnostics go to stderr; the answer goes to stdout.

mod embed;
mod prompt;

use std::collections::BTreeMap;
use std::io::Write as _;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::{CompletionClient, ProviderClient};
use rig::providers::anthropic;
use rig::providers::anthropic::completion::CompletionModel;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use rmcp::model::CallToolRequestParams;
use stackql_mcp::{Mode, StackqlMcp};
use tokio::io::AsyncBufReadExt as _;

const DEFAULT_MODEL: &str = "claude-opus-5";

/// Policies compiled into the binary, so the vendored build is self-contained.
/// `--policy <name>` picks one; `--policy <path>` loads your own.
struct BuiltinPolicy {
    name: &'static str,
    text: &'static str,
    /// Providers the policy reads and writes; pulled at start-up.
    providers: &'static [&'static str],
    /// Env vars the policy's placeholders need (for the error message).
    needs: &'static [&'static str],
}

const POLICIES: &[BuiltinPolicy] = &[
    BuiltinPolicy {
        name: "service-footprint",
        text: include_str!("../policies/service-footprint.md"),
        providers: &["aws", "awscc", "cloudflare"],
        needs: &[
            "AWS_REGION",
            "CLOUDFLARE_ZONE_ID",
            "DEMO_HOST",
            "DEMO_DOMAIN",
        ],
    },
    BuiltinPolicy {
        name: "golden-path",
        text: include_str!("../policies/golden-path.md"),
        providers: &["github"],
        needs: &["GITHUB_ORG", "GITHUB_REPO"],
    },
];

#[derive(Parser)]
#[command(
    name = "steward",
    version,
    about = "Keeps a service's estate on its golden path: an agent over actual state, on an embedded StackQL MCP server"
)]
struct Cli {
    /// Built-in policy name (service-footprint, golden-path) or a path to a
    /// markdown policy of your own. Placeholders like {{ AWS_REGION }} are
    /// filled from the environment and --set.
    #[arg(long, global = true, default_value = "service-footprint")]
    policy: String,
    /// Policy placeholder override, NAME=value (repeatable).
    #[arg(long = "set", global = true, value_name = "NAME=value")]
    sets: Vec<String>,
    /// Provider to pull and query (repeatable). Default: the policy's providers.
    #[arg(long = "provider", global = true)]
    providers: Vec<String>,
    /// Provider auth document as JSON, merged over the defaults. Default:
    /// each provider's own env-var auth (AWS_*, CLOUDFLARE_API_TOKEN,
    /// STACKQL_GITHUB_*), and github null_auth when no github creds are set.
    #[arg(long, global = true)]
    auth: Option<String>,
    /// Server mode for writes (fix, sql --write): safe asks approval for
    /// every write; delete_safe lets creates and updates through and asks
    /// only for deletes. Reads always run read_only.
    #[arg(long, global = true, default_value = "safe", value_parser = ["safe", "delete_safe"])]
    mode: String,
    /// Model id (or set STEWARD_MODEL).
    #[arg(long, global = true, env = "STEWARD_MODEL", default_value = DEFAULT_MODEL)]
    model: String,
    /// Max agent tool-call rounds per task.
    #[arg(long, global = true, default_value_t = 24)]
    max_turns: usize,
    /// Approve every write without asking (scripted demos, CI). Decisions are still logged.
    #[arg(long, global = true)]
    yes: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Read actual state, report drift against the policy, propose fixes (read-only)
    Check,
    /// Report drift and remediate it; every write is approved at the terminal
    Fix,
    /// Ask a free-form question about the estate (read-only)
    Ask { prompt: String },
    /// Interactive read-only session
    Chat,
    /// Run one SQL statement through the embedded server directly, no model.
    /// SELECT by default; --write sends a mutation (safe mode, approval prompt).
    Sql {
        sql: String,
        #[arg(long)]
        write: bool,
    },
    /// Start the embedded server and list its tools; no model calls
    Preflight,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Check);
    let can_write = matches!(command, Command::Fix | Command::Sql { write: true, .. });
    let mode = match (can_write, cli.mode.as_str()) {
        (false, _) => Mode::ReadOnly,
        (true, "delete_safe") => Mode::DeleteSafe,
        (true, _) => Mode::Safe,
    };

    // The policy: built-in by name or a file, placeholders from env + --set.
    let builtin = POLICIES.iter().find(|p| p.name == cli.policy);
    let (policy_name, policy_text) = match builtin {
        Some(p) => (p.name.to_string(), p.text.to_string()),
        None => (
            cli.policy.clone(),
            std::fs::read_to_string(&cli.policy)
                .with_context(|| format!("reading policy {} (not a built-in name)", cli.policy))?,
        ),
    };
    let mut vars: BTreeMap<String, String> = std::env::vars().collect();
    for kv in &cli.sets {
        let (k, v) = kv
            .split_once('=')
            .with_context(|| format!("--set expects NAME=value, got {kv:?}"))?;
        vars.insert(k.trim().to_string(), v.to_string());
    }
    let policy = prompt::render_policy(&policy_text, &vars).with_context(|| match builtin {
        Some(p) => format!("policy {} needs {}", p.name, p.needs.join(", ")),
        None => format!("policy {}", cli.policy),
    })?;

    let providers: Vec<String> = if cli.providers.is_empty() {
        match builtin {
            Some(p) => p.providers.iter().map(|s| s.to_string()).collect(),
            None => bail!("pass --provider at least once with a custom policy"),
        }
    } else {
        cli.providers.clone()
    };

    // Auth: providers read their own env vars server-side; github falls back
    // to null_auth (public data) when no credentials are set; --auth merges
    // over the top.
    let mut auth = serde_json::Map::new();
    if providers.iter().any(|p| p == "github") && !github_creds_present() {
        auth.insert("github".into(), serde_json::json!({ "type": "null_auth" }));
    }
    if let Some(raw) = &cli.auth {
        let extra: serde_json::Value =
            serde_json::from_str(raw).context("parsing --auth as JSON")?;
        for (k, v) in extra.as_object().context("--auth must be a JSON object")? {
            auth.insert(k.clone(), v.clone());
        }
    }
    let auth_label: Vec<String> = providers
        .iter()
        .map(|p| match auth.get(p).and_then(|a| a["type"].as_str()) {
            Some(t) => format!("{p}:{t}"),
            None => format!("{p}:env"),
        })
        .collect();

    // 1. The embedded StackQL MCP server. Vendored when built with the
    //    feature, otherwise downloaded and sha256-verified on first run.
    let mut builder = StackqlMcp::builder().mode(mode);
    if !auth.is_empty() {
        builder = builder.auth(serde_json::Value::Object(auth));
    }
    #[cfg(feature = "vendored")]
    let builder = builder.bundle_bytes(stackql_mcp::include_bundle!());
    eprintln!(
        "steward: starting embedded StackQL MCP server ({})...",
        mode.as_str()
    );
    let server = embed::start(builder, embed::ApprovingClient::new(cli.yes)).await?;

    for p in &providers {
        if let Err(e) = pull_provider(&server, p).await {
            eprintln!(
                "steward: could not pull provider {p} ({e:#}); continuing with whatever is cached"
            );
        }
    }
    let tools = server.list_all_tools().await.context("listing MCP tools")?;
    eprintln!(
        "steward: policy {} | providers {} | {} tools | mode {} | model {}",
        policy_name,
        auth_label.join(", "),
        tools.len(),
        mode.as_str(),
        cli.model
    );

    if let Command::Sql { sql, write } = command {
        let tool = if write {
            "run_mutation_query"
        } else {
            "run_select_query"
        };
        eprintln!("  -> {tool}({sql})");
        let mut params = CallToolRequestParams::new(tool);
        params.arguments = serde_json::json!({ "sql": sql }).as_object().cloned();
        let result = server.call_tool(params).await?;
        for c in &result.content {
            if let Some(t) = c.as_text() {
                println!("{}", t.text);
            }
        }
        server.cancel().await.ok();
        return Ok(());
    }

    if let Command::Preflight = command {
        println!("MCP tools available to the agent:");
        for t in &tools {
            println!("  - {}", t.name);
        }
        let key = std::env::var("ANTHROPIC_API_KEY").is_ok();
        println!(
            "ANTHROPIC_API_KEY: {}",
            if key {
                "set"
            } else {
                "not set (needed for check/fix/ask/chat)"
            }
        );
        server.cancel().await.ok();
        return Ok(());
    }

    // 2. The agent. list_all_tools() + peer() off the embedded server go
    //    straight into rig's rmcp_tools(); that is the integration.
    let agent = anthropic::Client::from_env()
        .context("ANTHROPIC_API_KEY not set")?
        .agent(&cli.model)
        .preamble(&prompt::system_prompt(&policy, &policy_name, mode))
        .rmcp_tools(tools, server.peer().clone())
        .max_tokens(32_000) // rig requires it for Anthropic; streaming keeps it safe
        .default_max_turns(cli.max_turns)
        .build();

    // 3. Run the task.
    let result = match command {
        Command::Check => {
            answer(
                &agent,
                "Check the estate against the policy and report drift.",
                cli.max_turns,
            )
            .await
        }
        Command::Fix => {
            answer(
                &agent,
                "Check the estate against the policy, then remediate every DRIFT finding the policy and the session mode allow you to fix.",
                cli.max_turns,
            )
            .await
        }
        Command::Ask { prompt } => answer(&agent, &prompt, cli.max_turns).await,
        Command::Chat => repl(&agent, cli.max_turns).await,
        Command::Sql { .. } | Command::Preflight => unreachable!(),
    };

    server.cancel().await.ok();
    result
}

fn github_creds_present() -> bool {
    std::env::var("STACKQL_GITHUB_USERNAME").is_ok_and(|v| !v.is_empty())
        && std::env::var("STACKQL_GITHUB_PASSWORD").is_ok_and(|v| !v.is_empty())
}

/// Pull the provider into the server's local cache so the first query is
/// instant. Read-only safe and idempotent (offline if already cached).
async fn pull_provider(server: &embed::Server, provider: &str) -> Result<()> {
    let mut params = CallToolRequestParams::new("pull_provider");
    params.arguments = serde_json::json!({ "provider": provider })
        .as_object()
        .cloned();
    server
        .call_tool(params)
        .await
        .with_context(|| format!("pulling provider {provider}"))?;
    Ok(())
}

async fn repl(agent: &Agent<CompletionModel>, max_turns: usize) -> Result<()> {
    println!("steward chat. Ask about the estate; 'quit' or Ctrl-D to exit.\n");
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        print!("you> ");
        std::io::stdout().flush().ok();
        let Some(line) = lines.next_line().await? else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "quit" | "exit" | ":q") {
            break;
        }
        if let Err(e) = answer(agent, line, max_turns).await {
            eprintln!("\nsteward: {e:#}");
        }
    }
    Ok(())
}

/// Stream one answer. Text goes to stdout as it arrives; every tool call the
/// agent makes is announced on stderr, so the room watches it query (and,
/// in fix mode, change) the live estate.
async fn answer(agent: &Agent<CompletionModel>, prompt: &str, max_turns: usize) -> Result<()> {
    let mut stream = agent.stream_prompt(prompt).multi_turn(max_turns).await;
    while let Some(item) = stream.next().await {
        match item? {
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)) => {
                print!("{}", text.text);
                std::io::stdout().flush().ok();
            }
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                ..
            }) => {
                let args = tool_call.function.arguments.to_string();
                eprintln!(
                    "\n  -> {}({})",
                    tool_call.function.name,
                    truncate(&args, 240)
                );
            }
            _ => {}
        }
    }
    println!();
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str(" ...");
    out
}
