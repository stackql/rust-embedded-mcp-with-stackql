# Rust-native agentic platform engineering with embedded MCP

Demo code for a demonstration of Rust-native agentic platform engineering with embedded MCP.

[StackQL](https://github.com/stackql/stackql) treats cloud and SaaS providers as data sources accessed via SQL. Agents doing platform engineering, SRE and audit work need to query, reason about and act on actual running state, not on state files. The StackQL MCP server is the agent interface to that engine, with a small fixed tool surface and safety modes gating writes.  

The [`stackql-mcp`](https://crates.io/crates/stackql-mcp) crate embeds that server in a Rust application, either as a sidecar (downloaded and verified at first run) or vendored straight into the Rust application (one self-contained binary). The worked examples are two small agents with the same four parts, model, prompts, context and MCP tools: an SRE assurance sweep over a service's footprint in AWS and Cloudflare (Claude, server as a sidecar) and a FinOps report from Cost Explorer (GPT-5, server vendored into the binary). The `embedded/` workspace also carries five companion examples outside the talk flow: the smallest embedding with no model (sidecar and vendored), a policy-driven agent with human-approved writes, a deterministic compliance scanner with evidence packs, and a persona REPL over public GitHub data; see [embedded/README.md](embedded/README.md#companion-examples).

## Quick start

```sh
git clone https://github.com/stackql/rust-embedded-mcp-with-stackql
cd rust-embedded-mcp-with-stackql
cp .env.example .env            # AWS, Cloudflare, Anthropic and OpenAI keys for the main flow
set -a; . ./.env; set +a
for p in aws awscc cloudflare github; do stackql exec "REGISTRY PULL $p"; done
(cd embedded && cargo build --release)    # both agents; the vendored one fetches the server bundle at build time
```

Needs `stackql`, `stackql-deploy`, `cargo` 1.88+ on PATH; `psql`, `jq`, `node` and `python` for the act 1 extras.

Then, from the repo root:

```sh
set -a; . ./.env; set +a
stackql exec -i primer/queries/exposure.iql                        # act 1 (paste blocks in primer/shell.iql, exec.sh, srv.sh)
stackql-deploy build stacks/service-footprint dev --env-file .env  # act 2: SG + instance + DNS record, about 30 s
./embedded/target/release/sre-agent-sidecar --check                # act 3: embedded server, providers, tools, no model call
./embedded/target/release/sre-agent-sidecar                        # act 3: the SRE sweep (Claude, sidecar server)
./stacks/service-footprint/drift.sh && ./embedded/target/release/sre-agent-sidecar   # act 3: drift on both planes, found and explained
./embedded/target/release/finops-agent-vendored                    # act 3: the FinOps report (GPT-5, vendored server)
```

## Why this shape

- One engine, several consumption patterns: shell, exec, Postgres wire protocol, declarative deploy, MCP. Act 1 and act 2 show the first four; act 3 is the fifth.
- The agent's tools are the StackQL MCP tools, so what the agent decides to do is always a readable SQL statement. The tool-call trace is the audit trail.
- Safety is a server-side contract. `Mode::ReadOnly` refuses writes however the model is prompted; both agents run in it. `Mode::Safe` asks a human before each write over MCP elicitation (`Mode::DeleteSafe` asks only for deletes).
- Agency is a property of the process, not the prompt: the mode is set in code, the prompt names what to read, the audit log records every tool call.
- Sidecar and vendored are the same builder with one line of difference. `finops-agent-vendored` is a single file with the StackQL engine inside that starts with no network.
- The prompts are markdown compiled into the binary (`include_str!`), so the persona, the StackQL context and the task are data you can read and edit without touching the Rust.

## References

- Crate and docs: [`stackql-mcp` on crates.io](https://crates.io/crates/stackql-mcp), [docs.rs/stackql-mcp](https://docs.rs/stackql-mcp); source lives in [stackql/stackql `packaging/mcpb/cargo`](https://github.com/stackql/stackql/tree/main/packaging/mcpb/cargo). The crate version equals the stackql release it embeds - pin the minor (`stackql-mcp = "0.10"`).
- StackQL MCP server: [stackql.io/docs/mcp](https://stackql.io/docs/mcp)
- stackql-deploy: [stackql-deploy.io](https://stackql-deploy.io), [stackql/stackql-deploy-rs](https://github.com/stackql/stackql-deploy-rs)
- Server bundles: built by [stackql/stackql `packaging/mcpb`](https://github.com/stackql/stackql/tree/main/packaging/mcpb), published on [stackql/stackql releases](https://github.com/stackql/stackql/releases)
- Provider registry: [registry.stackql.app](https://registry.stackql.app)

MIT licensed. Stars, issues and pull requests here and on [stackql/stackql](https://github.com/stackql/stackql) are the ask at the end of the talk.
