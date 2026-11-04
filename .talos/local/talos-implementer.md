Screen Ruler conventions for the implementer (2026-11-04, Talos setup).

- **This is a Rust and Slint repository.** The Desktop C++/Qt and Python commands above do not
  apply: there is no CMake, Qt or `jenkins/` here. The verification commands are those of
  `.claude/rules/working-rules.md`: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. The pytest
  suite under `tests/` covers the legacy Python app only; run it only when the diff touches it.
- **Read first**, from disk, `.claude/rules/working-rules.md` and
  `.claude/rules/engineering-principles.md`. They are the definition of done of every work package:
  readability first, small diffs, tests in a `#[cfg(test)] mod tests` at the bottom of the file
  they test, doc comments that say why, platform code behind `cfg` in its own backend, Slint
  colours and sizes from `crates/ruler-app/ui/theme.slint`.
- **Documentation is part of the package**: `README.md`, `docs/`, `--help` in
  `crates/ruler-app/src/cli.rs` and `help_lines` in `crates/ruler-app/src/app.rs` are updated in
  the same change, and a key change searches the repository for the key name.
- **Never run a `git` or `but` write command**: no commit, stage, stash, reset, branch, push or
  history edit. Leave the changes in the working tree; the orchestrator commits.
- **Never install anything or run an install script**, and never run the overlay with synthetic
  mouse or keyboard input: it covers the user's screens and moves their real cursor. List what
  needs a real screen as a hand test in the report.
- **Build in the existing `target/`** and never touch a repository file to force a rebuild.
- **The "red once" proof** quotes the failing output of the new test run against the broken or
  reverted implementation; the final evidence is the output of the three commands above.
