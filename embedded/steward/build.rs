// With --features vendored, fetch the pinned .mcpb at build time so
// include_bundle!() has something to embed (override: STACKQL_MCP_BUNDLE_FILE).
fn main() {
    println!("cargo:rerun-if-env-changed=STACKQL_MCP_BUNDLE_FILE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_VENDORED");
    if std::env::var_os("CARGO_FEATURE_VENDORED").is_none()
        || std::env::var_os("STACKQL_MCP_BUNDLE_FILE").is_some()
    {
        return;
    }
    let path = stackql_mcp::fetch_bundle().expect("fetch the platform stackql-mcp bundle");
    println!("cargo:rustc-env=STACKQL_MCP_BUNDLE_FILE={}", path.display());
    println!("cargo:warning=vendoring {}", path.display());
}
