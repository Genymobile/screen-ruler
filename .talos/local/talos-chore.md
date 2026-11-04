Screen Ruler conventions for the chore runner (2026-11-04, Talos setup).

- **This is a Rust and Slint repository**: after a mechanical edit run `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, in the
  existing `target/`, and quote the output.
- **A rename of a key** changes `README.md`, `--help` in `crates/ruler-app/src/cli.rs`,
  `help_lines` in `crates/ruler-app/src/app.rs` and every hint, prompt or tooltip that names it:
  search the repository for the key name.
- **Never run a `git` or `but` write command**, never install anything, never run the overlay.
  Leave the changes in the working tree.
- **Rules to follow**: `.claude/rules/working-rules.md` and
  `.claude/rules/engineering-principles.md`, read from disk.
