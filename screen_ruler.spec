# -*- mode: python ; coding: utf-8 -*-
#
# PyInstaller spec file for screen-ruler.
#
# Produces a single self-contained executable:
#   dist/screen-ruler
#
# Build with:
#   pyinstaller screen_ruler.spec

a = Analysis(
    ['screen_ruler.py'],
    pathex=[],
    binaries=[],
    datas=[],
    # cv2 is imported inside a try/except; declare it explicitly so
    # PyInstaller's static analyser does not miss it.
    hiddenimports=['cv2'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    # Trim heavy stdlib/third-party modules that are not used.
    excludes=['tkinter', 'matplotlib', 'scipy', 'PIL'],
    noarchive=False,
    optimize=0,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='screen-ruler',
    debug=False,
    bootloader_ignore_signals=False,
    # strip debug symbols to reduce file size
    strip=True,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    # console=True keeps startup messages ("Capturing screen…") visible
    # on Linux; it does not open a separate terminal window.
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
