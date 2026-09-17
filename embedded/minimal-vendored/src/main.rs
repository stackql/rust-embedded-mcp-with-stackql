// Act 3 companion example, not in the talk flow (the vendored demo is
// finops-agent-vendored, slide "VENDORED DETAILED"): identical to minimal,
// except the server bundle is compiled into this binary and extracted on
// first run. No network at run time; one file to ship.

use rmcp::model::CallToolRequestParams;
use stackql_mcp::{Mode, StackqlMcp};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = StackqlMcp::builder()
        .mode(Mode::ReadOnly)
        .auth(serde_json::json!({ "github": { "type": "null_auth" } }))
        .bundle_bytes(stackql_mcp::include_bundle!()) // <- the only change
        .start()
        .await?;

    println!(
        "embedded stackql mcp (vendored): {} tools, server at {}",
        server.list_all_tools().await?.len(),
        server.binary_path().display()
    );

    let sql = "SELECT login, public_repos, followers FROM github.orgs.orgs WHERE org = 'stackql'";
    println!("run_select_query: {sql}");
    let mut p = CallToolRequestParams::new("run_select_query");
    p.arguments = serde_json::json!({ "sql": sql }).as_object().cloned();
    for c in &server.call_tool(p).await?.content {
        if let Some(t) = c.as_text() {
            println!("{}", t.text);
        }
    }

    server.shutdown().await?;
    Ok(())
}
