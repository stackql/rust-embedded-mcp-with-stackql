# steward

A platform-engineering agent that keeps a service's estate on its golden path. It reads actual state across providers through an embedded StackQL MCP server, compares it with a markdown policy, reports drift, and repairs it with a human approving each write at the terminal. Built on [`stackql-mcp`](https://crates.io/crates/stackql-mcp), [rig](https://docs.rig.rs) and Claude.

The same source builds two ways. Sidecar acquires the StackQL server at first run; vendored compiles it in.

## Build

Sidecar (default):

```sh
cd embedded
cargo build --release -p steward
# -> target/release/steward
```

First run downloads the `.mcpb` bundle for the stackql release pinned in the crate from `releases.stackql.io`, verifies its sha256 against the pins in the crate, and caches it under `~/.stackql/mcp-server-bin/<version>/<platform>/`. Every later run is offline for the server. Override with `STACKQL_MCP_BIN` (run a stackql you already have) or `STACKQL_MCP_BUNDLE` (extract a local `.mcpb`).

Vendored:

```sh
cd embedded
cargo build --release -p steward --features vendored --target-dir target/vendored
# -> target/vendored/release/steward
```

`build.rs` fetches the same pinned bundle once at build time (network needed then, or set `STACKQL_MCP_BUNDLE_FILE=/path/to.mcpb` to embed one you already have) and `include_bundle!()` compiles it in. At run time the server is extracted from the binary into `~/.stackql/mcp-server-bin/vendored/<hash>/` and started; no download, one file to ship. Expect about 40 MB extra on linux and windows, about 90 MB on macOS (universal bundle).

`--target-dir` keeps the two builds from overwriting each other; drop it if you only want one.

## Run

Credentials come from the environment. Copy `.env.example` to `.env` at the repo root, fill it in, and export it:

```sh
set -a; . ./.env; set +a
```

The default policy (`service-footprint`) needs `AWS_*`, `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ZONE_ID`, `DEMO_HOST`, `DEMO_DOMAIN`; the model needs `ANTHROPIC_API_KEY`. The estate it checks is created by `stackql-deploy build stacks/service-footprint dev --env-file .env`.

```sh
steward preflight            # start the server, pull providers, list the 16 tools; no model call
steward check                # read-only: one row per control, evidence for any DRIFT, the SQL that would fix it
steward fix                  # safe mode: the server asks before every write, you answer y/n
steward fix --mode delete_safe   # creates and updates flow, only deletes ask
steward fix --yes            # auto-approve (scripted runs); every decision still logged
steward ask "..."            # free-form, read-only
steward sql "SELECT ..."     # one statement through the embedded server, no model
steward sql --write "..."    # one write through the approval prompt, no model
```

Swap the policy or the providers without rebuilding:

```sh
steward --policy golden-path check                          # GitHub variant; reads need no credentials
steward --policy ./my-estate.md --provider aws --provider okta check
steward --set DEMO_HOST=api check                           # override a policy placeholder
```

Policies are markdown with `{{ NAME }}` placeholders filled from the environment. `policies/service-footprint.md` and `policies/golden-path.md` are compiled in, so the vendored binary carries them; `--policy <path>` loads your own.

Other flags: `--auth '<json>'` merged over each provider's env-var auth, `--model` (default `claude-opus-5`, or `STEWARD_MODEL`), `--max-turns` (default 24).

## Deploy

The vendored binary is the deployable: copy `target/vendored/release/steward` to the host, provide the credentials in its environment, run it. Nothing to install, nothing downloaded for the server. Provider definitions are pulled from the StackQL registry on first use (`pull_provider`, a few seconds, needs the network) into `~/.stackql`; bake that directory into the image or run `steward preflight` once for an air-gapped host.

The sidecar binary deploys the same way but expects to reach `releases.stackql.io` on its first run on each host, or a pre-populated `~/.stackql/mcp-server-bin/`. The cache is shared with the StackQL npm and PyPI wrappers, so a host that already runs one of those has the server already.

Run it read-only on a schedule (`steward check`) and keep `fix` for a terminal with a human in front of it; the server mode is set in code per subcommand, so a scheduled `check` cannot write whatever the model decides.

## How it is wired

`embed.rs` spawns the server from `Builder::command()` and serves it with an rmcp `ClientHandler` that advertises elicitation and answers the server's "approve this write?" request at the terminal. `prompt.rs` renders the policy and the StackQL guidance into the system prompt. `main.rs` hands `list_all_tools()` and `peer()` to rig's `rmcp_tools()`; the agent's tools are the StackQL MCP tools, so every action it takes is a SQL statement you can read on stderr.
