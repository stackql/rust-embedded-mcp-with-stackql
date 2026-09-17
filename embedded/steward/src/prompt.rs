//! System prompt assembly. The persona is a platform-engineering steward;
//! the policy is a markdown file the operator can edit (placeholders filled
//! from the environment); the StackQL guidance encodes the provider quirks
//! that otherwise cost an agent a few wasted tool calls on live data.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use stackql_mcp::Mode;

pub const STACKQL_GUIDANCE: &str = r#"
You query and change real cloud and SaaS estates through StackQL, which
exposes providers as SQL, over MCP tools:
- run_select_query(sql): a read-only SELECT, rows come back as a table
- run_mutation_query(sql): INSERT / UPDATE / REPLACE / DELETE. Gated by the
  session mode: refused in read_only; in safe a human approves every
  statement; in delete_safe creates and updates run and only deletes ask.
- list_providers / list_services / list_resources / list_methods /
  describe_resource: discover the schema when unsure; list_methods shows the
  required WHERE columns of each resource

Writing StackQL well:
- Tables are provider.service.resource. Most resources need key columns in
  WHERE that map to API path or query parameters: aws and awscc resources
  need region = '...'; cloudflare.dns resources need zone_id = '...';
  github.repos.repos needs org = '...'. If a query errors, run list_methods
  on the resource.
- aws (describe views) is for reading; awscc (Cloud Control) is for writing:
  INSERT creates, UPDATE ... SET PatchDocument = string('[json patch]')
  WHERE Identifier = '...' modifies, DELETE ... WHERE Identifier = '...'
  removes. Cloud Control identifiers are the AWS ids (sg-..., i-..., sgr-...).
- Values come back as strings. Booleans render 'true'/'false' but compare as
  = 0 / = 1. JSON columns (tags, topics, state) are JSON text:
  JSON_EXTRACT(state, '$.name'), json_each(topics). aws tags look like
  {"item":[{"key":"owner","value":"x"}]}.
- IN (...) on a key column fans out to one API call per value; keep lists
  short. Avoid OR in WHERE; prefer coalesce(col, '') and CASE.
- ORDER BY with LIMIT in the same SELECT applies the limit first; wrap the
  ordered query in a subquery.
- Provider errors can surface as empty result sets. If a "clean" result looks
  surprising, sanity-check with a broader query before concluding.

Be concrete. Lead with the finding, then the SQL you ran and the rows behind
it. Never invent rows or ids; every id in a mutation must come from a row you
read in this session.
"#;

/// Replace `{{ NAME }}` placeholders in a policy with values from `vars`.
/// Unresolved placeholders are an error: a policy with a blank zone id or
/// region would send the agent off to guess.
pub fn render_policy(template: &str, vars: &BTreeMap<String, String>) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut missing = Vec::new();
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let name = after[..end].trim();
        match vars.get(name).filter(|v| !v.is_empty()) {
            Some(v) => out.push_str(v),
            None => {
                missing.push(name.to_string());
                out.push_str(&rest[start..start + 2 + end + 2]);
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        bail!(
            "policy placeholders not set: {} (set them in .env or pass --set NAME=value)",
            missing.join(", ")
        );
    }
    Ok(out)
}

pub fn system_prompt(policy: &str, policy_name: &str, mode: Mode) -> String {
    let stance = match mode {
        Mode::ReadOnly => {
            "This session is read-only: writes are refused by the server. For each \
             DRIFT finding, show the exact SQL statement that would fix it, but do \
             not attempt to run it."
        }
        Mode::Safe => {
            "You may remediate drift with run_mutation_query. The server is in safe \
             mode: a human approves or declines every statement, deletes included, \
             at the terminal. Propose each fix as one explicit SQL statement, run it, \
             and report the outcome. If a statement is declined, do not retry it, \
             say so and move on. Never change anything the policy does not name. \
             After all fixes, re-check the affected controls."
        }
        Mode::DeleteSafe | Mode::FullAccess => {
            "You may remediate drift with run_mutation_query. The server is in \
             delete_safe mode: creates and updates run without a prompt; deletes are \
             put to a human at the terminal. Propose each fix as one explicit SQL \
             statement, run it, and report the outcome. If a delete is declined, do \
             not retry it, say so and move on. Never change anything the policy does \
             not name. After all fixes, re-check the affected controls."
        }
    };
    format!(
        "You are steward, a platform-engineering agent. You keep a service's estate \
         on its golden path by comparing actual state, read live through StackQL \
         across every provider involved, against the policy below, and reporting \
         drift control by control with evidence.\n\nPolicy: {policy_name}\n\n{stance}\n\n\
         Report format, short enough to read on a projector: a table with one row \
         per control (PASS or DRIFT, one-line reason); then for each DRIFT only, \
         the SQL you ran and the rows that show it, and the one statement that \
         fixes it (in fix mode: the statement you ran and its outcome). No evidence \
         for PASS rows, no preamble, at most one closing sentence.\n\n\
         # Policy\n\n{policy}\n\n# StackQL guidance\n{STACKQL_GUIDANCE}"
    )
}
