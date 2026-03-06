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
    # Bundle the QML file so it is available inside the one-file binary.
    datas=[('screen_ruler.qml', '.')],
    # cv2 is imported inside a try/except; declare it explicitly so
    # PyInstaller's static analyser does not miss it.
    # QtQml and QtQuick are used at runtime by QQmlApplicationEngine but
    # are not imported at the top level of screen_ruler.py.
    hiddenimports=['cv2', 'PyQt6.QtQml', 'PyQt6.QtQuick'],
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
