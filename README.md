# screen-ruler

A smart, edge-detection-based screen ruler for Linux, Windows and macOS.

Move your mouse cursor over any UI element and instantly read the **width** and **height** of the space between the nearest edges — buttons, panels, windows, icons — with no clicking or dragging required.

> **The Rust rewrite is in preview.** v0.1 has every measurement tool in quick mode: measure, copy, quit. Session mode (persistent annotations, undo/redo, Markdown export) is ported but not released yet: it ships in v0.2, or [build from source](#build-from-source) to use it now. Composite image export is not ported yet. Until both ship, the [Python version](#python-version) remains the full-featured option.

## How it works

At launch, screen-ruler takes a one-time screenshot of every monitor and detects edges in it with a Canny edge detector that works on colour, so a step between two colours of equal brightness still counts. It then covers each monitor with a fullscreen overlay showing that screenshot. As the mouse moves, rays are cast North / South / East / West from the cursor until they hit an edge or the screen boundary, and the distances are shown next to the cursor as **W × H**.

Measurements are in logical pixels (the units UI toolkits and CSS use) and land exactly on edge boundaries, including on mixed and fractional display scales.

## Install

Download the archive for your platform from the [releases page](https://github.com/Genymobile/screen-ruler/releases):

| Platform | Archive |
|---|---|
| Linux x86_64 (glibc 2.35+, e.g. Ubuntu 22.04 or later) | `screen-ruler-<version>-linux-x86_64.tar.gz` |
| Windows x86_64 | `screen-ruler-<version>-windows-x86_64.zip` |
| macOS | no binary yet: [build from source](#build-from-source) |

Each archive contains the `screen-ruler` binary and an install script that copies it to a directory of your choice and sets up a [global keyboard shortcut](#global-keyboard-shortcut). The binary also runs as is, without installing.

On Linux the only runtime dependency is fontconfig, which every desktop distribution ships.

## Build from source

Install [Rust](https://rustup.rs) (stable), then:

```bash
cargo build --release     # binary at target/release/screen-ruler
cargo run --release       # or build and run in one step
```

On Linux, building also needs the fontconfig headers (`libfontconfig1-dev` on Debian/Ubuntu, `fontconfig-devel` on Fedora).

**macOS:** this is the only way to get screen-ruler on macOS for now. It builds, but hasn't been tested there yet; you will likely need to grant the Screen Recording permission (System Settings → Privacy & Security). Reports are welcome.

## Usage

```bash
screen-ruler [--sensitivity N] [--threshold-low N --threshold-high N] [--debug-edge-overlay]
```

| Option | Default | Description |
|---|---|---|
| `--sensitivity N` | 85 | Edge-detection sensitivity, 0–100. Higher finds more edges |
| `--threshold-low N` | | Lower Canny threshold, 0–255. Overrides `--sensitivity` |
| `--threshold-high N` | | Upper Canny threshold, 0–255. Overrides `--sensitivity` |
| `--debug-edge-overlay` | off | Keep the detected edge map visible |
| `-h`, `--help` / `-V`, `--version` | | Show the help / the version |

**Modes** (press `1`–`6`, or use the controls panel)

| Key | Mode | |
|---|---|---|
| `1` | Crosshair | Measure to the nearest edges around the cursor |
| `2` | Drag rectangle | Drag a rectangle, snapping to edges |
| `3` | Container detection | Detect the enclosing UI container |
| `4` | Shrink-to-fit | Drag, then tighten onto the content inside |
| `5` | Color picker | Sample a colour, optionally averaged |
| `6` | Point distance | Measure between two points |

**Controls**

| Input | Action |
|---|---|
| Left click | Copy the measurement to the clipboard and quit (point distance: place A, then B) |
| Drag | Select a rectangle (modes 2 and 4), then click or press `Enter` |
| `Enter` | Copy the current selection and quit |
| `Ctrl+C` | Copy the measurement and quit |
| Mouse wheel | Adjust the active mode's control (sensitivity, snap distance, colour averaging) |
| `Tab` | Toggle session mode (see below) |
| `?` or `H` | Toggle the shortcut help |
| `Esc` or `Q` | Quit |

**Session mode** *(`Tab`, or the `SESSION` button)*

Measurements stay on screen as annotations instead of being copied one at a time, on every monitor. A frame around the screen shows the session is on.

| Input | Action |
|---|---|
| Left click | Place the current measurement (point distance: place A, then B) |
| Drag | Place a rectangle (modes 2 and 4) |
| `Ctrl+C`, or the `MD` button | Copy all annotations as a Markdown list |
| `Ctrl+Z` or `Z` | Undo the last annotation |
| `Ctrl+Shift+Z` | Redo |
| `Tab`, `Esc` or `SESSION` | Leave session mode; with annotations placed, press again to confirm discarding them |
| `Q` | Quit; with annotations placed, press again to confirm |

## Global keyboard shortcut

Screen Ruler is designed to be ephemeral — capture, measure, quit. A global keyboard shortcut lets you summon it instantly without a persistent daemon. The install scripts take the binary next to them (as in a release archive), else `target/release/` (after `cargo build --release`), or the one given with `-b` / `-Binary`.

### Linux

```bash
./install-linux.sh            # installs to the current directory
./install-linux.sh -d ~/bin   # or specify a directory
```

The script copies the binary, installs a `.desktop` entry, and automatically registers a **Super+Shift+R** shortcut for the detected desktop environment (GNOME, KDE Plasma, Hyprland, Sway).

### macOS

After [building from source](#build-from-source):

```bash
./install-macos.sh            # installs to the current directory
./install-macos.sh -d ~/bin   # or specify a directory
```

The script copies the binary and creates an Automator Quick Action ("Launch Screen Ruler"). Then bind a shortcut (e.g. **⌘⇧R**) in **System Settings → Keyboard → Keyboard Shortcuts → Services**.

### Windows

In PowerShell:

```powershell
.\install-windows.ps1                                               # installs to the current directory
.\install-windows.ps1 -InstallDir "$env:LOCALAPPDATA\screen-ruler"  # or specify a directory
```

The script copies the binary, adds it to the user `PATH`, and creates a Start Menu shortcut with a **Ctrl+Shift+R** hotkey.

---

For manual setup on any platform, see [docs/manual-shortcut-setup.md](docs/manual-shortcut-setup.md).

## Known limitations

- No composite image export yet (planned for v0.2). Session mode is not in a release yet: [build from source](#build-from-source) to use it.
- On wlroots compositors (Sway, Hyprland), rotated or flipped monitors are captured unrotated. GNOME and Windows handle rotation.
- Tested live on GNOME Wayland (mixed scales, several monitors) and Windows 11 (several monitors, one rotated). KDE, X11 and macOS haven't been tested yet.

## Python version

The original implementation (`screen_ruler.py`, `ruler/`, `qml/`) uses PyQt6 and OpenCV. It is the full-featured option until v0.2, and will be retired once the Rust version has session mode and image export.

| Package | Minimum version |
|---|---|
| [PyQt6](https://pypi.org/project/PyQt6/) | 6.2 |
| [NumPy](https://pypi.org/project/numpy/) | 1.21 |
| [opencv-python-headless](https://pypi.org/project/opencv-python-headless/) | 4.5 |

```bash
pip install -r requirements.txt
python screen_ruler.py [--threshold-low N] [--threshold-high N] [--debug-edge-overlay]
```

It has the same modes and controls, including session mode, plus composite image export: in session mode, `Ctrl+Shift+C` or the `IMG` button, then drag a region to copy a screenshot of it with its annotations.

To build a standalone ~100 MB executable with [PyInstaller](https://pyinstaller.org) and install it with a keyboard shortcut:

```bash
pip install -r requirements.txt -r requirements-build.txt
pyinstaller screen_ruler.spec            # writes dist/screen-ruler
./install-linux.sh -b dist/screen-ruler  # or install-macos.sh; on Windows: -Binary dist\screen-ruler.exe
```

**Troubleshooting**

- *X11 overlay offset under top/side bars:* on some X11 window managers, frameless utility windows are constrained to the work area. The Python version uses `Qt.X11BypassWindowManagerHint` so the overlay matches the full virtual desktop, plus an application-level `Escape`/`Q` fallback, since bypass windows may have less predictable focus behaviour.
- *`qt.qpa.theme.gnome: dbus reply error ... NoReply`:* Qt may print this at startup on some GNOME/X11 systems while probing theme services over DBus. It is usually harmless; if it repeats, check that `dbus-daemon` and `xdg-desktop-portal` are healthy.

## Contributing

The Rust workspace is in `crates/`: `ruler-core` holds everything that doesn't depend on the UI toolkit (capture, edge detection, measurement, colour, clipboard, interaction state), and `ruler-app` is the Slint UI.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

CI runs the same checks on Linux, Windows and macOS for every pull request. To run them locally before each push, enable the pre-push hook, as described in [.githooks/pre-push](.githooks/pre-push).

For the Python version: `pip install -r requirements-dev.txt`, then `pytest tests/`.

## License

screen-ruler is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

The UI is built with [Slint](https://slint.dev), used under the [Slint Royalty-free Desktop, Mobile and Web Applications License](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md).

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)
