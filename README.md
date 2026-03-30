# screen-ruler

A smart, edge-detection-based screen ruler for Linux desktops.

Move your mouse cursor over any UI element and instantly read the **width** and **height** of the space between the nearest edges — buttons, panels, windows, icons — with no clicking or dragging required.

## How it works

At launch, screen-ruler captures a one-time screenshot and builds a binary edge map using Canny edge detection (with an OpenCV Gaussian pre-blur to suppress font anti-aliasing and wallpaper noise). A transparent overlay is then shown over all monitors. Each frame, four rays are cast North / South / East / West from the mouse cursor until they hit an edge pixel or the screen boundary. The total East+West distance is reported as **W** and North+South as **H**, live in a small label next to the cursor.

## Dependencies

| Package | Minimum version |
|---|---|
| [PyQt6](https://pypi.org/project/PyQt6/) | 6.2 |
| [NumPy](https://pypi.org/project/numpy/) | 1.21 |
| [opencv-python-headless](https://pypi.org/project/opencv-python-headless/) | 4.5 |

Install all dependencies at once:

```bash
pip install -r requirements.txt
```

## Usage

```bash
python screen_ruler.py [--threshold-low N] [--threshold-high N] [--debug-edge-overlay]
```

| Option | Default | Description |
|---|---|---|
| `--threshold-low N` | 29 | Lower hysteresis threshold for the Canny edge detector |
| `--threshold-high N` | 101 | Upper hysteresis threshold for the Canny edge detector |
| `--debug-edge-overlay` | off | Keep the captured Canny edge map visible (base opacity) for debugging; slider changes also trigger a transient edge preview |

**Controls**

| Input | Action |
|---|---|
| `Escape` or `Q` | Quit |
| Left click | Copy current measurement as `W × H px` to clipboard, then quit |
| Top `Sensitivity` slider | Recompute edge detection live (debounced) and show the edge-map preview briefly (hold then fade) |

## Build a standalone executable

If you don't want to install a Python environment, you can build a single self-contained binary with [PyInstaller](https://pyinstaller.org):

```bash
pip install -r requirements.txt -r requirements-build.txt
pyinstaller screen_ruler.spec
```

The binary is written to `dist/screen-ruler`. Copy it anywhere and run it directly — no Python or library installation needed on the target machine.

> **Note:** the executable bundles all dependencies and is therefore ~100 MB. This is expected for an application that ships PyQt6, NumPy, and OpenCV.

## Global keyboard shortcut (Linux)

Screen Ruler is designed to be ephemeral — capture, measure, quit. A global keyboard shortcut lets you summon it instantly without a persistent daemon.

After building the binary, run the install script:

```bash
./install-linux.sh            # installs to the current directory
./install-linux.sh -d ~/bin   # or specify a directory
```

The script copies the binary, installs a `.desktop` entry, and automatically registers a **Super+Shift+R** shortcut for the detected desktop environment (GNOME, KDE Plasma, Hyprland, Sway).

If you use a different DE or prefer manual setup, see [docs/manual-shortcut-setup.md](docs/manual-shortcut-setup.md).


## Troubleshooting

### X11 overlay offset under top/side bars

On some X11 window managers, frameless utility windows are constrained to the desktop work area (excluding panels/docks). screen-ruler uses `Qt.X11BypassWindowManagerHint` on X11 so the overlay matches full virtual-desktop coordinates used for capture and measurement.

Because bypass windows may have less predictable focus behavior, the app also installs an application-level `Escape`/`Q` key fallback.

### `qt.qpa.theme.gnome: dbus reply error ... NoReply`

On some GNOME/X11 systems Qt may print this warning during startup while probing desktop theme services over DBus. In most cases it is harmless and does not affect ruler behavior.

If it appears repeatedly, check that your session DBus and portal services are healthy (`dbus-daemon`, `xdg-desktop-portal`).


## Contributing

1. Install the dev dependencies (same as the runtime ones, plus `pytest`):
   ```bash
   pip install -r requirements-dev.txt
   ```
2. Run the test suite from the repository root:
   ```bash
   pytest tests/
   ```
3. The pure-logic helpers (`trace_ray`, `compute_edge_map`) are module-level functions specifically so they can be tested without a display. Keep new logic in the same style when possible.
4. The QML overlay (`screen_ruler.qml`) and screen-capture code require a running Qt application and are not covered by the unit tests.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
