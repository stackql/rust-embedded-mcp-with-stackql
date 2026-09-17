# Golden path for a repository

Every repository we own must satisfy the following. Check each one against
the actual state of the repository, not against any state file or manifest.

1. Topics: the repository carries all of the topics `stackql`, `rust`, `mcp`
   and `golden-path`. Read `topics` from `github.repos.repos` (filter with
   `org = '<org>' AND name = '<repo>'`; `topics` is a JSON array string).
   Remediation: `REPLACE github.repos.topics SET names = '["..."]' WHERE
   owner = '<org>' AND repo = '<repo>'` with the full desired list (existing
   topics plus any missing ones).

2. Labels: the labels `needs-triage` (colour d93f0b), `security` (b60205) and
   `agent-fixed` (0e8a16) exist. Read from `github.issues.labels` with
   `owner = '<org>' AND repo = '<repo>'`.
   Remediation: `INSERT INTO github.issues.labels (owner, repo, name, color,
   description) SELECT '<org>', '<repo>', '<name>', '<hex>', '<desc>'` for a
   missing label; `UPDATE github.issues.labels SET color = '<hex>' WHERE owner
   = '<org>' AND repo = '<repo>' AND name = '<name>'` for a wrong colour.

3. Default branch ruleset: an active ruleset named `golden-path-default-branch`
   with target `branch` exists. Read from `github.repos.rules` with
   `owner = '<org>' AND repo = '<repo>'`.
   Remediation: `INSERT INTO github.repos.rules (owner, repo, name, target,
   enforcement, conditions, rules) SELECT '<org>', '<repo>',
   'golden-path-default-branch', 'branch', 'active',
   '{"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}',
   '[{"type":"deletion"},{"type":"non_fast_forward"}]'`.

4. Hygiene: the repository has a description and a licence, and its default
   branch is `main`. Read from `github.repos.repos`. Report only; these are
   fixed by humans.
