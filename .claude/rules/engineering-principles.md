# Engineering principles

What good code and documentation look like here. These apply to every role and every change.

- Readability and maintainability first. Prefer the design a maintainer understands fastest:
  few abstractions, no speculative generality, no clever tricks. Stability comes before elegance
  and speed of delivery.
- Modern idioms are welcome, not avoided: stable Rust, edition 2021. Use iterators, `?`, pattern
  matching, `Option` and `Result` rather than sentinel values, enums rather than boolean flags.
  Simple means readable, not primitive.
- Theme: in Slint, shared colours and sizes come from `crates/ruler-app/ui/theme.slint`. Do not
  hardcode a value the theme already has.
- Small diffs. One topic per change. A change that grows a second topic is split.
- Existing style over personal style. Follow `rustfmt`, `CLAUDE.md` and the surrounding code.
- Behaviour-preserving unless the task says otherwise. A refactor changes no observable behaviour.
- Everything in the repo is assumed in use. Never remove something because no reference was found.
- Tests: tests cover what changed, when the change is testable. No coverage number is enforced. Unit
  tests sit in a `#[cfg(test)] mod tests` at the bottom of the file they test. A test-only helper
  may sit next to the code it supports, behind `#[cfg(test)]`. Each test is named as a sentence that
  states the behaviour, and tests one behaviour.
- Doc comments: they explain why the code exists and what a caller must know; they do not restate
  the signature. Every new type and non-trivial function has one. A doc comment is updated when the
  signature or meaning of its item changes.
- Documentation follows the change: `README.md` and `docs/` for users. No "(moved to...)" style
  change notes.
- Keys: the keys are listed in three places that must agree: the README, `--help` in
  `crates/ruler-app/src/cli.rs` and the in-app shortcut help (`help_lines` in
  `crates/ruler-app/src/app.rs`). Key names also appear in hints, confirm prompts and tooltips, in
  Rust and in Slint. A key change searches the repository for the key name.
- Platforms: Linux (Wayland on GNOME, KDE and wlroots; X11), Windows and macOS all build in CI and
  are meant to work. The README says what has been tested by hand. Platform code stays in its own
  backend (`crates/ruler-core/src/capture/`, `clipboard.rs`) behind `cfg`, never mixed into shared
  code. The one exception is the app entry point, `crates/ruler-app/src/main.rs`, which sets the
  Windows subsystem and attaches the console.
- Settle design questions on what the user of the app experiences: no hang, no panic, no silently
  wrong measurement, results always on screen. The Python app and earlier ports are evidence of
  behaviour, never the reason for a choice.
- The Python app (`ruler/`, `qml/`, `tests/`, `screen_ruler.py`, `screen_ruler.spec`,
  `conftest.py`, `requirements*.txt`) is legacy and is retired in v0.2. Change it only when the
  task says so.
