# DESKTOP Domain Rules — screen-ruler

## Verification Commands

### C++
```bash
# Build
cmake -B build -S . && cmake --build build
# Test
python3 jenkins/build run_tests
# Lint
jenkins/sanitize --cpp --cpplint --py --ts --check-only
```
- **Headless (xvfb-run):** `python3 jenkins/build run_tests`
- **GUI Tests:** `jenkins/run_squish`

### Python
```bash
# Test
QT_QPA_PLATFORM=offscreen pytest -v tests/
```

Run the commands of every technology present in this repository; a technology absent from the tree is skipped.

## Architectural Directives
- Build System: CMake 3.19+ out-of-tree builds strictly required; in-source builds are forbidden.
- Qt Architecture: Enforce Qt signal/slot thread affinity and QObject lifetime safety.
- Class Fields: Prefix instance members with `m`, static members with `s`.
- Static Analysis: `cppcheck` 1.90 as pinned by `genymotion-softs`; `clang-format` 18.1.8 via `jenkins/sanitize`.
- Submodule Containment: Respect boundaries of internal submodules (`libs/common`, `libs/libcom`, `libs/librendering`, `libs/prebuilt`, `misc/microtar`). `libs/libminicrypt` and `shared/libauth` are in-tree libraries, not submodules: do not look for a submodule there.
- Headless Qt Tests: Run display-dependent Qt tests headless with the mechanism the repository's own suite uses: `QT_QPA_PLATFORM=offscreen` (as `screen-ruler` does) or `xvfb-run` (as `genymotion-softs` does, applied by `python3 jenkins/build run_tests` on Linux only when `DISPLAY` is unset). `genymotion-softs` has no root `tests/`: its pytest suites under `functional-tests/tests/` are functional tests outside the `genymotion-softs-PRB` gate, and they need a configured `conftest.py`, so never report them as a pre-merge check.
- Multi-OS Compatibility: Code must compile and operate cleanly on Linux, macOS, and Windows.
