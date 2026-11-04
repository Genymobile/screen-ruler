Screen Ruler conventions for the reviewer (2026-11-04, Talos setup).

- **This is a Rust and Slint repository**: the verification commands are `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, run in the
  existing `target/`. Reproduce a claim by running them and report what was measured.
- **The written rules are** `CLAUDE.md`, `.claude/rules/working-rules.md` and
  `.claude/rules/engineering-principles.md`, read from disk. A breach is a finding; a preference
  of yours is not. Where they say nothing, the surrounding code is the rule.
- **Phase 0 shapes for this domain**: empty selection, zero-size rectangle, pointer on another
  monitor or off every monitor, repeated or held keys, a drag cancelled or finished after a mode
  switch, the first frame before analysis is ready; logical versus physical pixels, mixed and
  fractional monitor scales, rotated monitors, a value from one monitor used on another,
  off-by-one at edges, float rounding that changes a displayed measurement; a `RefCell` borrowed
  twice through a callback, a Slint model left out of sync, a timer that never stops; `unwrap`,
  indexing and arithmetic that can fail on real input; code behind `cfg` that only compiles on one
  platform (Wayland on GNOME, KDE and wlroots; X11; Windows; macOS).
- **When the diff touches capture, the clipboard, child processes, D-Bus, `unsafe`, platform
  APIs, install scripts or command-line input**, also derive: a helper or portal call that can
  block forever, a missing timeout, a zombie process, an unquoted path, a buffer size trusted from
  the compositor, an error that leaves the overlay covering the screens or quits silently.
- **Complexity is a finding**: an abstraction the change does not need, duplication of an
  existing helper, a diff wider than its topic, a dated construct where a modern idiom reads
  better.
- **The three key lists agree**: `README.md`, `--help` in `crates/ruler-app/src/cli.rs` and
  `help_lines` in `crates/ruler-app/src/app.rs`; so do the hints, prompts and tooltips that name
  a key.
- **Report** each finding with the severity scale of `.claude/rules/working-rules.md`, its
  `path:line` and a concrete failure scenario (the input or state, then the wrong outcome). Never
  run the overlay with synthetic input: what needs a real screen is listed as a hand test.
