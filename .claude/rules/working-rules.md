# Working in this repository

How to work and report here. These rules apply to the main session and to every dispatched agent.

## Dispatched agents

- A dispatched agent does not dispatch other agents, ask the user or commit. Open questions go in
  its report.
- A read-only role uses Bash only to read, build and run the checks.
- Read `CLAUDE.md` and `.claude/rules/*.md` from disk when they matter: a copy loaded earlier in
  the session may be stale.

## Checks

The same checks run in CI (`.github/workflows/ci.yml`) and in `.githooks/pre-push`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

There is no Markdown checker: read each changed link and anchor to check that it resolves.

## Building

- Build with `cargo` in the existing `target/` directory.
- To build another revision, export it to the scratchpad and build it there with its own
  `CARGO_TARGET_DIR`.
- Never touch a repository file to force a rebuild, not even its timestamp.

## The user's machine

- Never run an install script and never install anything.
- Never run the overlay with synthetic mouse or keyboard input: it covers the user's screens and
  moves their real cursor. List what needs a real screen as a hand test for the user instead.

## Version control

- A dispatched agent runs no `git` or `but` write command of any kind: no commit, stage, stash,
  reset, branch switch, push or history edit. It leaves its changes in the working tree.
- The version-control section of `CLAUDE.md` is for the main session, which alone commits.
- Inspect version-control changes only with the diff command the brief names. In a GitButler
  workspace, a whole-workspace `but status` or `but diff` also shows other agents' changes.

## Reports

Report faithfully. A failing check, a skipped step or an unverified claim is said plainly. No
claim of success without the command output that supports it.

Every role uses one severity scale, sorted by impact:

- **blocker**: a check fails, or a crash, hang, data loss or wrong measurement in normal use.
- **major**: wrong behaviour users will meet, or a written rule broken so that behaviour or
  maintainability suffers.
- **minor**: a real problem with a rare or small effect, which can wait.
- **nit**: small wording, naming or layout. Small naming or prose breaches of a rule are nits.

Blockers and majors are proposed for fix before commit; the user approves each fix.

Review findings go in one table, ordered by severity:

| Sev | path:line | Finding | Evidence | Fix |
| --- | --- | --- | --- | --- |

- Finding and Fix are one sentence each.
- Each agent file says what goes in Evidence. For an extra review angle, its brief says.
- A finding not verified against the code is marked "plausible".
- An empty table is a valid result. Say so plainly.
- When the orchestrator merges reports, it adds a `By` column that names the agent behind each
  finding.
