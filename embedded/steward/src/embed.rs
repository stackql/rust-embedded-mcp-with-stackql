//! Start the embedded StackQL MCP server with a client that can answer
//! elicitation requests.
//!
//! `StackqlMcp::builder().start()` hands back a connected rmcp client, but
//! that client declines every elicitation. In `Mode::Safe` the server asks
//! for approval before each mutation, so a client that cannot answer means
//! every write is refused. `Builder::command()` is the escape hatch: it
//! resolves and acquires the binary exactly as `start()` would and returns a
//! `std::process::Command` with the canonical launch arguments. We spawn it
//! ourselves and serve it with a handler whose `create_elicitation` puts the
//! question to the human at the terminal.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::model::{
    ClientCapabilities, ClientInfo, CreateElicitationRequestParams, CreateElicitationResult,
    ElicitationAction, ElicitationCapability, ErrorData, FormElicitationCapability, Implementation,
};
use rmcp::service::{RequestContext, RoleClient, RunningService};
use rmcp::{ClientHandler, ServiceExt};
use stackql_mcp::Builder;
use tokio::io::AsyncBufReadExt as _;

pub type Server = RunningService<RoleClient, ApprovingClient>;

/// An rmcp client that advertises form elicitation and answers it by asking
/// the operator on the terminal. `auto_approve` skips the question (for
/// scripted runs) but still logs every decision.
#[derive(Clone)]
pub struct ApprovingClient {
    auto_approve: Arc<AtomicBool>,
}

impl ApprovingClient {
    pub fn new(auto_approve: bool) -> Self {
        Self {
            auto_approve: Arc::new(AtomicBool::new(auto_approve)),
        }
    }
}

impl ClientHandler for ApprovingClient {
    #[allow(clippy::field_reassign_with_default)]
    fn get_info(&self) -> ClientInfo {
        // These structs are non_exhaustive, so build them by field assignment.
        let mut elicitation = ElicitationCapability::default();
        elicitation.form = Some(FormElicitationCapability::default());
        let mut capabilities = ClientCapabilities::default();
        capabilities.elicitation = Some(elicitation);
        let mut implementation = Implementation::from_build_env();
        implementation.name = "steward".into();
        implementation.version = env!("CARGO_PKG_VERSION").into();
        ClientInfo::new(capabilities, implementation)
    }

    async fn create_elicitation(
        &self,
        request: CreateElicitationRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<CreateElicitationResult, ErrorData> {
        let message = match &request {
            CreateElicitationRequestParams::FormElicitationParams { message, .. } => message,
            CreateElicitationRequestParams::UrlElicitationParams { message, .. } => message,
        };
        eprintln!("\n[approval] server asks:\n{}", indent(message));
        let approved = if self.auto_approve.load(Ordering::Relaxed) {
            eprintln!("[approval] auto-approved (--yes)");
            true
        } else {
            ask_yes_no("[approval] allow this write? [y/N] ").await
        };
        let action = if approved {
            eprintln!("[approval] approved");
            ElicitationAction::Accept
        } else {
            eprintln!("[approval] declined");
            ElicitationAction::Decline
        };
        Ok(CreateElicitationResult {
            action,
            content: approved.then(|| serde_json::json!({})),
            meta: None,
        })
    }
}

/// Acquire (if needed), spawn, and handshake, using the crate's canonical
/// launch arguments but our own client handler.
pub async fn start(builder: Builder, client: ApprovingClient) -> Result<Server> {
    let cmd = tokio::task::spawn_blocking(move || builder.command())
        .await
        .context("acquisition task")??;
    let mut child = tokio::process::Command::from(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // server diagnostics stay visible
        .kill_on_drop(true)
        .spawn()
        .context("spawning the stackql mcp server")?;
    let stdout = child.stdout.take().context("child stdout")?;
    let stdin = child.stdin.take().context("child stdin")?;
    let service = client
        .serve((stdout, stdin))
        .await
        .context("MCP initialize with the embedded server")?;
    // When `service` is cancelled its transport drops, the server sees EOF on
    // stdin and exits; this task reaps it.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(service)
}

async fn ask_yes_no(prompt: &str) -> bool {
    eprint!("{prompt}");
    let mut line = String::new();
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    match stdin.read_line(&mut line).await {
        Ok(_) => matches!(line.trim(), "y" | "Y" | "yes" | "YES"),
        Err(_) => false,
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
