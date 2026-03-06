# screen-ruler

A smart, edge-detection-based screen ruler for Linux desktops.

Move your mouse cursor over any UI element and instantly read the **width** and **height** of the space between the nearest edges — buttons, panels, windows, icons — with no clicking or dragging required.

---

## How it works

At launch, screen-ruler captures a one-time screenshot and builds a binary edge map using Canny edge detection (with an OpenCV Gaussian pre-blur to suppress font anti-aliasing and wallpaper noise). A transparent, click-through overlay is then shown over all monitors. Each frame, four rays are cast North / South / East / West from the mouse cursor until they hit an edge pixel or the screen boundary. The total East+West distance is reported as **W** and North+South as **H**, live in a small label next to the cursor.

---

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

---

## Usage

```bash
python screen_ruler.py [--threshold-low N] [--threshold-high N]
```

| Option | Default | Description |
|---|---|---|
| `--threshold-low N` | 50 | Lower hysteresis threshold for the Canny edge detector |
| `--threshold-high N` | 150 | Upper hysteresis threshold for the Canny edge detector |

**Controls**

| Key | Action |
|---|---|
| `Escape` or `Q` | Quit |

---

## Build a standalone executable

If you don't want to install a Python environment, you can build a single self-contained binary with [PyInstaller](https://pyinstaller.org):

```bash
pip install -r requirements.txt -r requirements-build.txt
pyinstaller screen_ruler.spec
```

The binary is written to `dist/screen-ruler`. Copy it anywhere and run it directly — no Python or library installation needed on the target machine.

> **Note:** the executable bundles all dependencies and is therefore ~100 MB. This is expected for an application that ships PyQt5, NumPy, and OpenCV.

---

## Contributing

1. Install the dev dependencies (same as the runtime ones, plus `pytest`):
   ```bash
   pip install -r requirements.txt pytest
   ```
2. Run the test suite from the repository root:
   ```bash
   pytest tests/
   ```
3. The pure-logic helpers (`trace_ray`, `compute_edge_map`) are module-level functions specifically so they can be tested without a display. Keep new logic in the same style when possible.
4. The overlay widget (`ScreenRulerOverlay`) and screen-capture code require a running Qt application and are not covered by the unit tests.
