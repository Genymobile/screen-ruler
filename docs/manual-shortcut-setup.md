# Manual shortcut setup

If the install scripts do not support your setup, or you prefer to configure things by hand, follow the instructions below.

In all examples, replace `screen-ruler` with the full path to the binary if it is not on your `PATH`.

## Linux

### GNOME (Ubuntu)

Open **Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts**, click **+** and fill in:

| Field | Value |
|---|---|
| Name | Screen Ruler |
| Command | `screen-ruler` |
| Shortcut | e.g. `Super+Shift+R` |

Or via the command line:

```bash
# Pick an unused slot (custom0, custom1, …)
SLOT=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/

gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings \
  "['$SLOT']"

gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$SLOT \
  name 'Screen Ruler'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$SLOT \
  command 'screen-ruler'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$SLOT \
  binding '<Super><Shift>r'
```

### KDE Plasma

**System Settings → Shortcuts → Custom Shortcuts → Edit → New → Global Shortcut → Command/URL**, then set the command to `screen-ruler` and choose your key combination.

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
