# CLAUDE.md

## What this repo is

The long-lived Rust demo repo for embedded StackQL MCP: working, repeatable demonstrations of building agentic applications in Rust on the published `stackql-mcp` crate. It began as (and still contains) the demo code and runbook for the talk "Rust-native agentic platform engineering with embedded MCP" (Jeff Aven, Rust User Group Melbourne, August 2026), and additionally hosts the reference apps that shipped alongside the crate before the crate moved into stackql/stackql `packaging/mcpb/cargo`. It is not a library and not a product.

History note: this repository was previously `stackql/stackql-mcp-rs` (the crate's original home; renamed, stars and history preserved). The pre-repurpose crate tree is in git history before commit 464fab9.

The deck is mastered in Google Slides:
https://docs.google.com/presentation/d/15FpxQqUQ6WQdQYkHIL_CgVkKo3DgzZa_75VQYyzbEgo/edit

A PDF export of the deck lives in `ref/` (gitignored) and is the reference for what the demo needs to show. `slides/notes.md` holds the speaker notes, one block per slide; `slides/build_slides.py` regenerates slides 13-15 from a pptx export. When the deck and this file disagree, the deck wins and this file gets updated.

## The talk in one paragraph

StackQL treats cloud and SaaS providers as data sources accessed via SQL. Agents in the platform engineering / SRE / observability space need to query, reason about, and act on actual running state, not on state files. The StackQL MCP server is the agent interface to the StackQL engine, with a small fixed tool surface (16 tools) and safety modes gating writes. The `stackql-mcp` crate embeds that MCP server in a Rust application, either as a sidecar (the release pinned in the crate, downloaded and sha256-verified at first run) or vendored straight into the Rust application (single self-contained binary). The concrete examples are two small agents with the same four parts, model, prompts, context and MCP tools: `sre-agent-sidecar` (Claude, sidecar server) sweeps a service's footprint across AWS and Cloudflare; `finops-agent-vendored` (GPT-5, server vendored into the binary) reports month-to-date spend and waste from Cost Explorer. The audience is AI engineers who program in Rust.

## Demo flow

Three acts, in this order, matching the deck. Each act is runnable on its own from a clean shell in the repo root. Slot is 45 minutes including slides: roughly 40% on the primer side (act 1 + act 2 + the StackQL slides) and 60% on embedded MCP (the MCP and Rust slides + act 3).

Act 1 - StackQL primer (slides "STACKQL >>", "Cloud Providers as Data Sources", "How to use StackQL"), about 7 minutes of demo

`primer/` follows the layout of `clickhouse-stackql-demo/demo/clickhouse-provider`: paste blocks, not batch scripts, one block at a time from the repo root with `.env` exported.

- `primer/shell.iql` - paste into `stackql shell`, three providers and two planes: `REGISTRY PULL aws | cloudflare | github`, `SHOW PROVIDERS`, discovery (`SHOW SERVICES IN aws LIKE 'ec%'`, `SHOW RESOURCES IN cloudflare.dns`, `SHOW EXTENDED METHODS`, `DESCRIBE EXTENDED`), then the key points: `JSON_EXTRACT` (instance state, zone plan), `json_each` (tags as rows, Cost Explorer groups), SQLite built-ins (`strftime`, `julianday`, `GROUP_CONCAT`, `json_array_length`), window functions (`SUM OVER PARTITION BY` on volumes, `ROW_NUMBER` on zones, `RANK` on releases), the cross-plane `UNION ALL`, the cross-provider CTE JOIN, and the mutation verbs (golden-state github values, safe to run). GitHub is limited to single-object calls (the org, one repo's branches and releases): org-wide lists page through the API and take 5 to 50 s.
- `primer/exec.sh` - `stackql exec` over aws and cloudflare, output formats `table | json | jsonl | csv` (those are the ones the engine has), `-f` to a file, `-H`, `-d`, piped into `jq` and `column`, `-i primer/queries/exposure.iql`, jsonnet templating (`finops.iql` + `vars.jsonnet`, `--var month=$(date +%Y-%m)` renders the Cost Explorer window), `--dryrun --output text` to show the rendered SQL, then the cross-provider query.
- `primer/srv.sh` - `stackql srv` (Postgres wire protocol) with `psql` (aws inventory, `-x` on the running instances, `--csv` on the zone's records), then `primer/pgwire-lite-app` (Node, `@stackql/pgwire-lite`, `npm install` first) and pystackql in `server_mode`.
- `primer/pystackql-app/app.py` and `primer/pgwire-lite-app/app.js` - the reference apps stay on public GitHub data so they run with zero credentials (every column arrives as text in pystackql; `pd.to_numeric` before aggregating; `app_root=~/.stackql` shares the provider cache with the CLI).

Timing budget: every query in the flow is about a second of API time (measured 2026-08-26; the Windows CLI adds ~1.5 s of start-up per `exec`, the shell pays it once). `primer/queries/cross-provider.iql` (rendered from `footprint-vars.jsonnet` with `--var zone=$DEMO_DOMAIN --var region=$AWS_REGION`) looks the zone up by name (`cloudflare.zones.zones`), joins its A records, then joins the EC2 instances they point at (needs the act 2/3 credentials) and is the bridge into the next two acts. It returns one row per built stack: `stacks/aws-webserver` (native `aws` provider, `webserver-dev.stackql.xyz`) and `stacks/service-footprint` (`rust-demo.stackql.xyz`); with neither built it returns no rows. With no github credentials in the environment the current build needs no `--auth` flag.

Act 2 - stackql-deploy, Rust native (slide "How to use StackQL", Platform Automation row), about 4 minutes

`stacks/service-footprint/`: one service's footprint across two planes as data: a security group and a tagged `t3.micro` in AWS (`awscc` writes, `aws` reads), an A record `rust-demo.stackql.xyz` in Cloudflare pointing at it, a conformance query. `stackql-deploy build|test|teardown stacks/service-footprint dev --env-file .env`, `--dry-run --show-queries` first. `.env` must be exported as well as passed (the spawned stackql server reads provider creds from the environment). Clean build about 30 s, re-converge about 40 s. `stacks/golden-path/` (GitHub: topics, labels, ruleset) is the zero-credential variant, not in the main flow.

Act 3 - Embedded MCP in a Rust application (slides "STACKQL MCP" through "DEMO"), about 13 minutes

Two agents, modelled on the Python and Node agents in `clickhouse-stackql-demo/demo/agentic-use-cases` (SDK + MCP server + a fixed task prompt), one per way of embedding the server. Each is one `src/main.rs` plus `prompts/system.md` (persona and StackQL context) and `prompts/task.md` (the job), compiled in with `include_str!` and rendered with `{{ NAME }}` from the environment. Both run the server in `Mode::ReadOnly`.

1. `embedded/sre-agent-sidecar` - sidecar (slide "SIDECAR DETAILED"): Claude via rig's anthropic provider (`ANTHROPIC_API_KEY`, `SRE_AGENT_MODEL`, default `claude-opus-5`); providers `aws` + `cloudflare`; the task is the morning assurance sweep over the service footprint (health, exposure, edge, governance), PASS or ATTENTION per check with the fixing SQL shown, not run. `--check` preflights without a model call; a positional argument replaces the task. Show the cache dir before and after the first run, `stacks/service-footprint/drift.sh tag ssh edge dangling`, the sweep again, then `stackql-deploy build` to converge.
2. `embedded/finops-agent-vendored` - vendored (slide "VENDORED DETAILED"): same program with `bundle_bytes(include_bundle!())`, `build.rs` fetches the bundle at build time; GPT-5 via rig's openai provider (`OPENAI_API_KEY`, `FINOPS_AGENT_MODEL`, default `gpt-5`); provider `aws`; the task is the month-to-date FinOps report (Cost Explorer spend by service, compute inventory and tagging gaps, waste: stopped instances, available volumes, unassociated EIPs). Show the binary sizes and a `--check` on a fresh `HOME`.

Safety modes get called out during step 1: `ReadOnly` (default) -> `Safe` -> `DeleteSafe` -> `FullAccess`, escalation is a caller opt-in via `.mode(...)`; `Safe` asks the client for approval before every write (deletes included), `DeleteSafe` only for deletes, over MCP elicitation; a client that cannot answer gets a refusal. Neither agent escalates; the human-in-the-loop write path needs an rmcp `ClientHandler` that answers elicitation, spawned via `Builder::command()`. The companion `embedded/steward` (`fix` subcommand, `src/embed.rs`) is that path, outside the talk flow.

The workspace also builds five companion examples that are not in the talk flow and are documented in `embedded/README.md`: `minimal` and `minimal-vendored` (the smallest embedding, no model, zero credentials), `steward` (policy-driven agent, human-approved writes), `auditron` (YAML control packs in `controls/`, live TUI, evidence zips, no agent loop) and `stackql-agent` (REPL with three personas over public GitHub data). They are reference apps, not demo steps; the runbook and the deck do not mention them.

Crate versions: `stackql-mcp = "0.10"` (pin the minor); `rig-core = "0.40"` with the `rmcp` feature, the last rig release that has it (0.41 dropped it), resolving with `stackql-mcp` on one `rmcp` 1.x. Everything else floats on the latest 1.x/4.x. The companions `steward` and `stackql-agent` pin `rig-core = "0.38"` in their own manifests, the release they were written against; cargo builds both rig versions side by side.

## Repo layout

```
rust-embedded-mcp-with-stackql/
  CLAUDE.md
  README.md                  short, points to the talk and the runbook
  RUNBOOK.md                 exact commands in demo order, copy/paste-able
  .env.example               every variable, all optional
  ref/                       (gitignored) PDF export of the deck
  slides/notes.md            speaker notes per slide; build_slides.py regenerates slides 13-15
  primer/                    act 1, paste blocks: shell.iql, exec.sh, srv.sh, queries/*.iql + *.jsonnet,
                             pystackql-app/app.py, pgwire-lite-app/app.js
  stacks/                    act 2: service-footprint/ (aws + awscc + cloudflare; drift.sh introduces the act 3
                             drift: tag, ssh, edge, dangling), aws-webserver/ (native aws + cloudflare, backs the
                             act 1 cross-provider query), golden-path/ (github), README.md
  embedded/                  act 3
    Cargo.toml               workspace; stackql-mcp = "0.10" from crates.io (version-locked
                             to the stackql release it embeds; a server bump is a normal
                             dependency bump - never reintroduce a git pin)
    sre-agent-sidecar/               sidecar + Claude: src/main.rs, prompts/system.md, prompts/task.md
    finops-agent-vendored/            vendored + GPT-5: src/main.rs, build.rs, prompts/system.md, prompts/task.md
    minimal/, minimal-vendored/      companion examples, not in the talk flow: the smallest embedding, no model,
                             zero credentials (github null_auth), one per way of embedding
    steward/                 companion: policy-driven agent (policies/*.md), the human-in-the-loop write path
                             (Safe / DeleteSafe, elicitation answered at the terminal in src/embed.rs)
    auditron/                companion: YAML control packs (controls/), live TUI, evidence zips, no agent loop
    stackql-agent/           companion: REPL with three personas over public GitHub data, zero credentials
    README.md                the wiring, sidecar vs vendored, safety modes, prompts, companion examples
  controls/github-core.yaml  auditron's built-in control pack, compiled in with include_str!
  .github/workflows/ci.yml   fmt, clippy -D warnings, build, --check smokes, agent-live (one question each)
```

## Conventions

Writing (README, RUNBOOK, slide notes, comments):

- Australian spelling.
- Hyphens only. No em dashes, no `--` used as punctuation.
- `->` for arrows. Nothing that is not on a QWERTY keyboard.
- Matter-of-fact prose. No hyperbole, no marketing adjectives.
- Say "cloud as data sources accessed via SQL", not "cloud as SQL tables". Say "vendored straight into the Rust application", not "into the binary".
- No stacked markdown headings (a heading immediately followed by another heading).

Code:

- Rust 2021 edition, MSRV 1.88 (set by `rmcp` 1.x via `stackql-mcp`).
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` clean before commit (run in `embedded/`).
- Examples are deliberately small. Each agent is one source file and two prompt files; if it grows past that it has stopped being a demo. Sidecar and vendored must stay a diff of a few lines. That rule is for the two demo agents; the companion examples under `embedded/` are reference apps and may be bigger, but they pass the same fmt and clippy gates.
- Every runnable thing has a comment at the top saying which act and slide it belongs to, or that it is a companion example outside the talk flow.
- No credentials in the repo. `.env.example` documents variables; `.env` is gitignored.

Shell scripts (there is one, `stacks/service-footprint/drift.sh`; anything else that can be a runbook line is a runbook line, not a script):

- `set -euo pipefail`, POSIX-ish bash, self-contained (loads `.env` itself), tested on macOS and Linux (Jeff presents from macOS, darwin-universal bundle). Also runs under Git Bash on Windows.
- It prints each statement before running it so the room can read it. Echoes go to stderr so stdout stays pipeable.
- Pass `--approot "$HOME/.stackql"` to `stackql` explicitly in executed scripts (Windows builds default the approot to the cwd). The act 1 paste blocks in `primer/` leave it out (macOS and Linux default to `~/.stackql`) and say so in their header; add it by hand when running them on Windows.

## Demo-day constraints

- Assume conference wifi is bad or absent. The pre-flight block at the top of RUNBOOK.md must leave the machine able to start every act offline: providers pulled, the server bundle cached for sidecar, vendored binaries already built, cargo deps in the local registry cache. Reads against GitHub and calls to Anthropic still need the network; the runbook has fallbacks.
- The main flow needs AWS, Cloudflare, Anthropic and OpenAI credentials in `.env` (exported). The GitHub variant (`stacks/golden-path`, act 1) keeps a zero-credential path for people who clone the repo; the runbook marks what needs what.
- Terminal font size and colour scheme are set for a projector. Output in act 1 fits an 80x24 terminal without wrapping; pick queries and `SELECT` columns accordingly.
- Provider quirks that bite on stage: `ORDER BY` + `LIMIT` in one SELECT applies the limit first (wrap in a subquery); `IN (...)` on a key column fans out to one API call per value; booleans compare as `0`/`1`; a JOIN pushes key params from the other table but filter the fan-out explicitly; a flat three-table JOIN fails to plan (`cannot project response data: missing key`) and a scalar subquery cannot supply a key param, so put the first two-table join in a CTE or derived table and join the third table to that; `json_each` in the same `FROM` as a provider table drops the `WHERE` filters, so filter the provider query in a CTE and `json_each` over the CTE; window functions only work in the innermost query over the provider table (an outer query over a derived table cannot add one, `query rewriting for indirection: cannot find col`); a `CASE ... END = 1` in a provider table's `WHERE` matches nothing (`coalesce()` there is fine; put `CASE` in `SELECT` or an outer query); agents do better on flat one-row-per-item resources (`aws.ec2.security_group_rules`, `aws.ec2.tags`) than on JSON blobs (`security_groups.ip_permissions`, `instances.tags`), so the system prompts say so; `github.repos.repos` by `owner` + `repo` returns nothing on 0.10.601 (use `org` lists or the org/branches/releases resources); GitHub org-wide lists page through the API (20 pages by default) and take 5 to 50 s; `UPDATE ... SET` sends numbers and booleans as strings (Cloudflare `ttl`, classic branch protection), so prefer `awscc` PatchDocument writes and delete+insert where a provider insists on types; a terminated EC2 instance keeps its tags readable for up to an hour.
- The demo mutates a real sandbox: AWS account 824532806693 (ap-southeast-2, default VPC) and the `stackql.xyz` zone. Identity tags (`service`, `stackql:*`) find the resources; governance tags (`owner`, `cost-centre`) are desired state. Never make an `exists` query depend on a tag that is allowed to drift.

## CI

`.github/workflows/ci.yml`, on every push to main and PR. `build-test`: fmt, clippy `-D warnings`, workspace build (finops-agent-vendored's build.rs downloads and pin-verifies the bundle - network needed; the build and clippy steps compile the companion examples too, which have no smoke steps), then zero-credential smokes: `sre-agent-sidecar --check`, `finops-agent-vendored --check`. `agent-live` asks each agent one question that needs no cloud credentials (server mode and installed providers); the `ANTHROPIC_API_KEY` org secret is configured, so pushes make one small Claude call; the finops step skips until an `OPENAI_API_KEY` secret is added.

## Working in this repo with Claude

- Before writing anything for act 3, read the crate docs at docs.rs/stackql-mcp (source: stackql/stackql `packaging/mcpb/cargo`); the published crate API is the source of truth, not memory. The crate version equals the stackql release it embeds; the workspace pins the minor (`stackql-mcp = "0.10"`).
- Models: `sre-agent-sidecar` defaults to `claude-opus-5` (`SRE_AGENT_MODEL`), `finops-agent-vendored` to `gpt-5` (`FINOPS_AGENT_MODEL`); rig reads `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` from the environment. Model names are plain strings, so a new model is an env var, not a rebuild.
- Prompts are `include_str!("../prompts/*.md")`, paths relative to `src/`. `{{ NAME }}` placeholders come from the environment (`{{ TODAY }}` from the clock in finops-agent-vendored); an unset placeholder is an error, not a blank.
- Before writing stack files for act 2, check `stackql-deploy --help` for the current argument order and flags in the Rust build. Env vars are referenced in manifests as `{{ AWS_REGION }}` (no `vars.` prefix). One statement per anchor. Read with `aws`, write with `awscc`; `cloudflare.dns.zones_dns_records` to list/create, `cloudflare.dns.records` to edit/delete.
- When asked for slide content, produce paste-ready text and mermaid source in `slides/notes.md`, not slide files. The deck lives in Google Slides.
- When asked for runbook changes, keep RUNBOOK.md strictly in demo order and copy/paste-able. No prose between commands beyond one line of intent.
- Table format by default when showing StackQL results in discussion.

## References

- stackql-mcp crate: https://crates.io/crates/stackql-mcp (source: https://github.com/stackql/stackql/tree/main/packaging/mcpb/cargo)
- rig: https://docs.rs/rig-core/0.40.0 (the rmcp feature: `AgentBuilder::rmcp_tools`, `StreamingPromptRequest::max_turns`)
- StackQL MCP docs: https://stackql.io/docs/mcp and https://stackql.io/docs/command-line-usage/mcp
- Installing (including the MCP server): https://stackql.io/docs/installing-stackql#installing-the-mcp-server
- stackql-deploy (Rust): https://github.com/stackql/stackql-deploy-rs, installer at get-stackql-deploy.io, docs at https://stackql-deploy.io
- Bundles: built by stackql/stackql packaging/mcpb, published per release on https://github.com/stackql/stackql/releases
- Provider registry: https://registry.stackql.app, github provider docs at https://github-provider.stackql.io
