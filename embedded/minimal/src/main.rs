// Act 3 companion example, not in the talk flow (the sidecar demo is
// sre-agent-sidecar, slide "SIDECAR DETAILED"): the smallest embedding.
// Start the embedded StackQL MCP server read-only, list its tools, run one
// SELECT over the github provider with no credentials, prove writes are
// refused, shut down. First run downloads and caches the server; after that
// it is offline.

use rmcp::model::CallToolRequestParams;
use stackql_mcp::{Mode, StackqlMcp};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = StackqlMcp::builder()
        .mode(Mode::ReadOnly) // writes refused server-side, whatever the caller asks
        .auth(serde_json::json!({ "github": { "type": "null_auth" } }))
        .start() // acquire -> verify sha256 -> spawn -> MCP handshake
        .await?;

    let tools = server.list_all_tools().await?;
    println!("embedded stackql mcp: {} tools, read_only", tools.len());
    for t in &tools {
        println!("  {}", t.name);
    }

    let sql = "SELECT login, public_repos, followers FROM github.orgs.orgs WHERE org = 'stackql'";
    println!("\nrun_select_query: {sql}");
    let rows = server.call_tool(tool("run_select_query", sql)).await?;
    print_text(&rows);

    let sql = "DELETE FROM github.issues.labels \
               WHERE owner = 'stackql' AND repo = 'stackql' AND name = 'bug'";
    println!("\nrun_mutation_query (should be refused): {sql}");
    let refused = server.call_tool(tool("run_mutation_query", sql)).await?;
    print_text(&refused);

    server.shutdown().await?;
    Ok(())
}

fn tool(name: &'static str, sql: &str) -> CallToolRequestParams {
    let mut p = CallToolRequestParams::new(name);
    p.arguments = serde_json::json!({ "sql": sql }).as_object().cloned();
    p
}

fn print_text(result: &rmcp::model::CallToolResult) {
    for c in &result.content {
        if let Some(t) = c.as_text() {
            println!("{}", t.text);
        }
    }
}
