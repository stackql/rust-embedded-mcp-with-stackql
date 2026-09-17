# Act 3 - embedded MCP in a Rust application

Two agents, one crate. Both embed the StackQL MCP server via [`stackql-mcp`](https://crates.io/crates/stackql-mcp), hand the connected [`rmcp`](https://crates.io/crates/rmcp) client to a [rig](https://docs.rs/rig-core) agent, and run a fixed task written in markdown. They differ in how the server arrives and which model drives them; everything else is the same shape, on purpose. The workspace also builds five companion examples outside the talk flow; see [Companion examples](#companion-examples).

| Program | Server | Model | Task |
|---|---|---|---|
| `sre-agent-sidecar/` | sidecar: downloaded, sha256-verified and cached on first run | Claude (`ANTHROPIC_API_KEY`, `SRE_AGENT_MODEL`, default `claude-opus-5`) | the morning assurance sweep over the service footprint in AWS and Cloudflare: health, exposure, edge, governance |
| `finops-agent-vendored/` | vendored: fetched at build time, compiled into the binary, extracted on first run | GPT-5 (`OPENAI_API_KEY`, `FINOPS_AGENT_MODEL`, default `gpt-5`) | the month-to-date FinOps report: spend by service from Cost Explorer, tagging gaps, waste |

Each program is one `src/main.rs` and two prompt files, `prompts/system.md` (persona and StackQL context) and `prompts/task.md` (the job), compiled in with `include_str!` and rendered with `{{ NAME }}` values from the environment. Both servers run in `Mode::ReadOnly`: the agents investigate and report, they cannot change anything, whatever the prompt.

## Run

```sh
set -a; . ./.env; set +a          # ANTHROPIC_API_KEY, OPENAI_API_KEY, AWS_*, CLOUDFLARE_*, DEMO_*
cd embedded
cargo build --release             # finops-agent-vendored's build.rs fetches the bundle once (network at build time)

./target/release/sre-agent-sidecar --check          # server, providers, tools; no model call
./target/release/sre-agent-sidecar                  # the sweep (prompts/task.md)
./target/release/sre-agent-sidecar "Which security groups in ap-southeast-2 allow port 22 from anywhere?"

# ~/.stackql/mcp-server-bin/0.10.605/

./target/release/finops-agent-vendored --check
./target/release/finops-agent-vendored               # the report
./target/release/finops-agent-vendored "What did EC2 cost us last month, by usage type?"
```

Tool calls are echoed on stderr as `-> tool {args}` while the answer streams to stdout, so the room sees every SQL statement the agent runs. `--max-turns` bounds the tool-call rounds (default 40; rig raises an error at the limit rather than returning a partial answer, so keep it above what the task needs). MSRV 1.88.

## The wiring

The whole integration is five lines. The crate hands back a connected rmcp client; rig's agent builder consumes its tools and peer directly.

```rust
let server = StackqlMcp::builder().mode(Mode::ReadOnly).start().await?;   // sidecar
let tools  = server.list_all_tools().await?;                               // the StackQL MCP tools
let agent  = anthropic::Client::from_env()?
    .agent("claude-opus-5")
    .preamble(include_str!("../prompts/system.md"))
    .rmcp_tools(tools, server.peer().clone())
    .build();
let mut stream = agent.stream_prompt(task).max_turns(40).await;           // the loop is rig's
```

The vendored agent differs in one builder line, `.bundle_bytes(stackql_mcp::include_bundle!())`, one Cargo feature (`stackql-mcp/vendored`) and a `build.rs` that fetches the bundle; and in the provider client, `openai::Client::from_env()?.agent("gpt-5")`.

Versions: `stackql-mcp` is pinned to the minor (`"0.10"`), because the crate version equals the stackql release it embeds. `rig-core` is `0.40`, the last release with the `rmcp` feature (0.41 dropped it); it and `stackql-mcp` resolve on the same `rmcp` 1.x, which is what lets the two plug together without an adapter.

## Sidecar vs vendored

| | Sidecar (default feature) | Vendored (`vendored` feature) |
|---|---|---|
| How the server arrives | the release pinned in the crate, `.mcpb` downloaded and cached on first run | fetched once at build time (`build.rs` calls `stackql_mcp::fetch_bundle()`), embedded with `include_bundle!()` |
| Integrity | sha256 checked against the pins rendered into the crate from the release's `.sha256` assets | the bytes are the artefact (fetched pin-verified at build time) |
| First run | download, verify, extract to `~/.stackql/mcp-server-bin/<version>/<platform>/`, launch | extract to `~/.stackql/mcp-server-bin/vendored/<hash>/`, launch |
| Network at run time | first run of each release only | never |
| Artefact size | small (the app) | app + ~40 MB (linux/windows) or ~90 MB (darwin-universal) |
| Choose when | you want small binaries and easy server updates | air-gapped hosts, single-file distribution, demos on conference wifi |

Overrides that work in both: `STACKQL_MCP_BIN` (run a stackql you already have), `STACKQL_MCP_BUNDLE` (extract a local `.mcpb`), `STACKQL_MCP_BUNDLE_FILE` at build time (embed a bundle you already have), `Builder::approot()`.

## Safety modes

`Mode::ReadOnly` (default) -> `Mode::Safe` -> `Mode::DeleteSafe` -> `Mode::FullAccess`. The mode is enforced inside the server: in `ReadOnly` a `run_mutation_query` is refused whatever the model asks; in `Safe` the server sends an MCP elicitation request before every write (deletes included); in `DeleteSafe` creates and updates run and only deletes ask; a client that cannot answer elicitation gets a refusal. Escalation is a caller opt-in via `.mode(...)`. Both agents here stay in `ReadOnly`; to act with a human in the loop, start the server in `Safe` with an rmcp `ClientHandler` that advertises elicitation and answers it at the terminal (`Builder::command()` gives you the acquired binary and canonical launch arguments to spawn it yourself).

## Prompts

The prompts are the product. `system.md` is the same shape in both agents: who the agent is, that the session is read-only, the MCP tools it has, how to pick the resource before writing SQL (discovery in two calls, flat one-row-per-item resources over JSON blobs, a tool-call budget, change approach rather than retry variants), how to write StackQL that lands on the first try (key columns in WHERE, plain-column filters, booleans as 0/1, JSON columns, the json_each CTE rule, fan-out, ORDER BY with LIMIT, the CTE for a third join), and how to report without narrating the debugging. Measured effect on the free-form question "which security groups allow port 22 from anywhere": 20 tool calls and a caveat section before, 7 tool calls and a clean table after. `task.md` is the job, numbered, naming the resources to read so the agent spends its tool calls on evidence rather than archaeology. Edit either file and rebuild; nothing else changes.

## Companion examples

The workspace also builds five programs that are not in the talk flow. They came out of building this repo and each shows something the two agents do not. Build any of them with `cargo build --release -p <name>` from `embedded/`. `steward` and `stackql-agent` need `ANTHROPIC_API_KEY` for anything that calls the model; the other three need no key.

| Program | Server | What it shows |
|---|---|---|
| `minimal/` | sidecar | The smallest embedding, no model: builder, `Mode::ReadOnly`, github `null_auth`, list the tools, one `run_select_query`, one refused `run_mutation_query`, shutdown. Zero credentials. |
| `minimal-vendored/` | vendored | The same program with the bundle compiled in: `build.rs` fetches it once at build time, `include_bundle!()` embeds it, the crate extracts it on first run. |
| `steward/` | sidecar, or vendored with `--features vendored` | A policy-driven platform-engineering agent (Claude via rig). Reads a service's actual state across AWS and Cloudflare (GitHub as the zero-credential variant), compares it with a markdown policy, reports drift, and in `fix` mode remediates it with every write approved by a human at the terminal. The human-in-the-loop write path the two agents leave out. |
| `auditron/` | sidecar, or vendored with `--features vendored` | A deterministic compliance scanner with no agent loop: runs a YAML control pack (`controls/github-core.yaml`, zero credentials) against live state, renders a live TUI or `--no-tui` line output, and writes an auditor-ready evidence zip. `e` in the TUI asks Claude to explain the selected finding when `ANTHROPIC_API_KEY` is set. |
| `stackql-agent/` | sidecar, or vendored with `--features vendored` | The crate wired into a rig agent as a REPL: one binary, three personas (`--persona platform`, `sre` or `audit`), public GitHub data with zero credentials, `-p` for one-shot, `--check` to preflight without a model call. `sre-agent-sidecar` grew out of it. |

`steward` and `stackql-agent` pin `rig-core = "0.38"` in their own manifests, the release they were written against; the two agents use the workspace `rig-core = "0.40"`, and cargo builds both side by side. `auditron` and `stackql-agent` have no `build.rs`, so their `--features vendored` builds need `STACKQL_MCP_BUNDLE_FILE` pointing at a `.mcpb` you already have (build `minimal-vendored` once; its build script prints the cached bundle path as a cargo warning).

### steward

`check` runs the server read-only. `fix` runs it in `Mode::Safe` (`--mode delete_safe` lets creates and updates through) and serves it with an rmcp `ClientHandler` that advertises elicitation and answers the server's "approve this write?" request at the terminal (`steward/src/embed.rs`). The crate's `start()` uses a client that declines elicitation, so `steward` takes the escape hatch: `Builder::command()` gives the acquired binary and canonical launch arguments, and `steward` spawns and serves it itself.

```sh
set -a; . ./.env; set +a                                 # AWS_*, CLOUDFLARE_*, DEMO_HOST, DEMO_DOMAIN, ANTHROPIC_API_KEY
./target/release/steward preflight                       # server, providers, tools; no model call
./target/release/steward check                           # read-only: one row per control, evidence and the fixing SQL for any DRIFT
./target/release/steward fix                             # remediate; approve each write at the terminal
./target/release/steward fix --mode delete_safe          # creates and updates flow, only deletes ask
./target/release/steward ask "Which instances in ap-southeast-2 have a public IP and no owner tag?"
./target/release/steward sql --write "DELETE ..."        # one write through the approval prompt, no model
./target/release/steward --policy golden-path check      # the GitHub variant; reads need no credentials
```

Policies are markdown with `{{ NAME }}` placeholders filled from the environment. `steward/policies/service-footprint.md` (five controls over the act 2 stack) and `golden-path.md` (GitHub topics, labels, ruleset) are compiled in; `--policy <path>` loads your own. The full option list is in `steward/README.md`.

### auditron

Run it from the repo root so `packs` finds `controls/`.

```sh
./embedded/target/release/auditron packs                                   # the built-in github-core pack plus any controls/*.yaml
./embedded/target/release/auditron scan                                    # live TUI; q quits, e explains the selected finding
./embedded/target/release/auditron scan --no-tui --var org=stackql         # line output for CI and pipes; exit 2 when any control fails
./embedded/target/release/auditron evidence -o out/github-core.zip         # manifest, pack source, per-control SQL and CSV
```

A pack is YAML (`schema: auditron/v1`): provider, auth document, variables, and one SQL statement per control with `pass_when: no_rows` (returned rows are findings) or `pass_when: rows` (returned rows are evidence). The built-in pack is compiled in from `controls/github-core.yaml`, so a vendored binary carries its demo.

### stackql-agent

```sh
./target/release/stackql-agent --check                                     # server, provider, tools; no model call
./target/release/stackql-agent --persona platform                          # REPL
./target/release/stackql-agent --persona sre -p "Show the most recent failed workflow runs for stackql/stackql"
./target/release/stackql-agent --persona audit --provider aws --auth '{"aws":{"type":"aws_signing_v4"}}' \
  -p "Which security groups allow 0.0.0.0/0 on port 22?"
```

The personas are system prompts in `stackql-agent/src/persona.rs`; the backend, the tools and the read-only contract are the same across all three. Details in `stackql-agent/README.md`.
