# Manual shortcut setup

If the install scripts do not support your setup, or you prefer to configure things by hand, follow the instructions below.

In all examples, replace `screen-ruler` with the full path to the binary if it is not on your `PATH`.

## Linux

> **Tip:** `install-linux.sh` auto-detects your desktop environment (GNOME,
> KDE Plasma, Hyprland, Sway) and registers **Super+Shift+R** automatically.
> Use these manual steps only if the script doesn't cover your setup.

### GNOME

Open **Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts**, click **+** and fill in:

| Field | Value |
|---|---|
| Name | Screen Ruler |
| Command | `screen-ruler` |
| Shortcut | e.g. `Super+Shift+R` |

Or via the terminal — this safely appends to any existing shortcuts:

```bash
SCHEMA=org.gnome.settings-daemon.plugins.media-keys
SLOT=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/

CURRENT="$(gsettings get "$SCHEMA" custom-keybindings)"
NEW_LIST="$([ "$CURRENT" = "@as []" ] \
  && echo "['$SLOT']" \
  || echo "${CURRENT/%]/, '$SLOT']}")"

gsettings set "$SCHEMA" custom-keybindings "$NEW_LIST"
gsettings set "$SCHEMA.custom-keybinding:$SLOT" name    'Screen Ruler'
gsettings set "$SCHEMA.custom-keybinding:$SLOT" command 'screen-ruler'
gsettings set "$SCHEMA.custom-keybinding:$SLOT" binding '<Super><Shift>r'
```

> If `custom0` is already taken, change the slot to `custom1`, `custom2`, etc.

### KDE Plasma

**System Settings → Shortcuts → Custom Shortcuts → Edit → New → Global Shortcut → Command/URL**, set the command to `screen-ruler`, and press your preferred key combination.

### Hyprland

Add to `~/.config/hypr/hyprland.conf`:

```
bind = $mainMod SHIFT, R, exec, screen-ruler
```

Then reload: `hyprctl reload`

### Sway

Add to `~/.config/sway/config`:

```
bindsym $mod+Shift+r exec screen-ruler
```

Then reload: `swaymsg reload`

### Other desktops

Any launcher or shortcut manager that can run an arbitrary command will work. Point it at the `screen-ruler` binary — the app captures the screen, lets you measure, and exits on its own.

## macOS

### Option 1: Automator Quick Action

1. Open **Automator** and create a new **Quick Action** (Service).
2. Set "Workflow receives" to **no input** in **any application**.
3. Add a **Run Shell Script** action with the command:
   ```
   /path/to/screen-ruler
   ```
4. Save as "Launch Screen Ruler".
5. Open **System Settings → Keyboard → Keyboard Shortcuts → Services** (or on older macOS: **System Preferences → Keyboard → Shortcuts → Services**).
6. Find "Launch Screen Ruler" under **General**, click **Add Shortcut**, and press your preferred keys (e.g. **⌘⇧R**).

### Option 2: Shortcuts app (macOS 12+)

1. Open the **Shortcuts** app and create a new shortcut.
2. Add a **Run Shell Script** action with the command:
   ```
   /path/to/screen-ruler
   ```
3. In the shortcut's details (ⓘ), enable **Use as Quick Action** → **Keyboard Shortcut**.
4. Go to **System Settings → Keyboard → Keyboard Shortcuts → Services** and bind a key.

### Option 3: Third-party tools

Apps like [Raycast](https://www.raycast.com/), [Alfred](https://www.alfredapp.com/), or [Hammerspoon](https://www.hammerspoon.org/) can bind a global hotkey to run an arbitrary command. Point it at the `screen-ruler` binary.

## Windows

### Option 1: Start Menu shortcut hotkey

1. Place `screen-ruler.exe` in a permanent directory.
2. Create a shortcut (`.lnk`) to it in the **Start Menu** folder:
   - Press `Win+R`, type `shell:programs`, and press Enter.
   - Right-click → **New → Shortcut**, point it at `screen-ruler.exe`.
3. Right-click the shortcut → **Properties** → **Shortcut key**, press your preferred combo (e.g. **Ctrl+Shift+R**) → **OK**.

> The hotkey only works while the `.lnk` remains in the Start Menu or Desktop folder.

### Option 2: PowerToys

[PowerToys Keyboard Manager](https://learn.microsoft.com/en-us/windows/powertoys/keyboard-manager) can remap a key combination to launch an application. Add a shortcut that runs `screen-ruler.exe`.

### Option 3: AutoHotkey

Create an `.ahk` script:

```ahk
^+r::Run "C:\path\to\screen-ruler.exe"
```

This binds **Ctrl+Shift+R** to launch screen-ruler.
