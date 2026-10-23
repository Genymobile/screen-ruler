# Rust + Slint rewrite plan

Tracks the rewrite of screen-ruler from the Python/PyQt6/QML implementation
(`ruler/`, `qml/`) to a Rust workspace (`crates/`) with a Slint UI. This
replaces an earlier, uncommitted `plan.md` session artifact that was lost
between sessions — this file is checked in so it isn't lost again.

## Where things stand

- `crates/ruler-core` and `crates/ruler-app` (workspace defined in the root
  `Cargo.toml`) were scaffolded as placeholders in `chore(rust): scaffold
  Cargo workspace for Rust+Slint rewrite`.
- No Slint code existed anywhere in the repo's history until this round of
  work — `ruler-app` had no `slint` dependency yet.
- A **complete, working, tested reference rewrite** already exists as the
  `rust-rewrite` branch (identical to `remotes/edznux/rust-rewrite`, 6 commits
  on top of `main`). It has first-class multi-monitor support and hand-rolled
  Canny edge detection (no OpenCV, so it stays a single self-contained
  binary), but its UI shell is built on **winit + glutin + egui**, not Slint.
  It is the porting source for `ruler-core`, not something we apply wholesale.

### What's portable as-is vs. what needs a fresh Slint design

Checked every module in the reference branch for UI-toolkit imports
(`egui::`, `glutin::`, `winit::`, `glow::`, `raw_window_handle::`):

| Module (reference branch `src/`) | Toolkit-coupled? | Tests | Plan |
|---|---|---|---|
| `geometry.rs` | no | 9 | **Ported** → `ruler-core::geometry` |
| `image.rs` | no | 6 | **Ported** → `ruler-core::image` |
| `color.rs` | no | 9 | **Ported** → `ruler-core::color` |
| `edges.rs` | no | 10 | port as-is |
| `regions.rs` | no | 6 | port as-is |
| `measure.rs` | no | 10 | port as-is |
| `capture/mod.rs`, `capture/wlr.rs` | no | 13 | port as-is (brings in `xcap`, `wayland-client`, `wayland-protocols-wlr` deps) |
| `png.rs` | no | 4 | port as-is |
| `export.rs` | no | 6 | port as-is |
| `clipboard.rs` | no | 0 | port as-is (brings in `arboard`) |
| `state.rs` (1395 lines — the mode/annotation state machine) | no | 30 | port as-is; this is the crate's centerpiece |
| `cli.rs` | no | 10 | port as-is |
| `window.rs` (597 lines — winit/glutin event loop, one GL context per monitor) | **yes** | — | **rewrite** for Slint's windowing model |
| `ui/mod.rs`, `ui/layout.rs`, `ui/panel.rs`, `ui/overlay.rs`, `ui/help.rs`, `ui/theme.rs` (~2,600 lines — egui widgets) | **yes** | — | **rewrite** in `.slint` files; mine these for layout/behavior decisions only, not code |

`state.rs`'s module doc is explicit about why it ported cleanly: "Deliberately
free of any windowing or drawing types: the UI layer reads this and renders
it... That keeps the behaviour that is easy to get wrong — mode switching,
undo/redo, the confirm-before-discard flow — testable without a display."

### Open design question for Phase 2

The reference branch's multi-monitor support hinges on winit's
`Fullscreen::Borderless(Some(monitor))` — one borderless fullscreen window
pinned to a specific output, which is the only placement primitive Wayland
gives a client. Slint's windowing API needs to be checked for an equivalent
before the UI design work starts; if it can't express "fullscreen on *this*
output" directly, we may need Slint's winit backend or a different placement
strategy.

### Slint spike (`dev/slint-hello-window`) — validated

Built and run successfully (screenshot confirmed by the user): a minimal
inline-markup window renders correctly on this machine's Wayland session
with Slint 1.18's default (winit + femtovg) backend. Needed
`libfontconfig1-dev` installed as a build dependency (font discovery).

Watch item for the real multi-window UI: closing the spike window logs
`Slint winit backend: request to hide window failed because references to
the window still exist` from `i-slint-backend-winit`'s `suspend()` path —
it can't reclaim the winit `Window`'s `Arc` because our own `main()` keeps
a strong reference alive across `.run()`. Harmless for a single window (the
process still exits cleanly), but worth re-checking once we're creating and
tearing down one window per monitor, so a held reference doesn't leak a
window's resources across repeated show/hide cycles.

## Phases

**Phase 1 — port the toolkit-agnostic core into `ruler-core`.**
Pull each module marked "port as-is" above from `remotes/edznux/rust-rewrite`
module-by-module, one commit per module, tests included. Order (leaf
dependencies first): `geometry` → `color` → `edges`/`image` → `regions` →
`measure` → `capture` → `png`/`export`/`clipboard` → `state` → `cli`.

**Phase 2 — design the Slint UI in `ruler-app`.**
New work, not a port. Define `.slint` files for the overlay chrome (controls
panel, mode selector, crosshair/annotation rendering, shortcut help),
resolve the per-monitor window placement question above, then wire it to
`RulerState`/commands the way `qml/screen_ruler.qml`'s `ruler` context
property does today.

**Phase 3 — wire and validate.**
Reach feature parity with the Python app (and the reference branch's
multi-monitor improvements), update install scripts/CI, then retire the
Python implementation.

## Module porting checklist (Phase 1)

- [x] `geometry.rs`
- [x] `image.rs`
- [x] `color.rs`
- [ ] `edges.rs`
- [ ] `regions.rs`
- [ ] `measure.rs`
- [ ] `capture/mod.rs` / `capture/wlr.rs`
- [ ] `png.rs`
- [ ] `export.rs`
- [ ] `clipboard.rs`
- [ ] `state.rs`
- [ ] `cli.rs`
