# Manual shortcut setup

If `install-linux.sh` does not support your desktop environment, or you prefer to configure the shortcut by hand, follow the instructions below.

In all examples, replace `screen-ruler` with the full path to the binary if it is not on your `PATH`.

## GNOME (Ubuntu)

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

## KDE Plasma

**System Settings → Shortcuts → Custom Shortcuts → Edit → New → Global Shortcut → Command/URL**, then set the command to `screen-ruler` and choose your key combination.

## Hyprland

Add to `~/.config/hypr/hyprland.conf`:

```
bind = $mainMod SHIFT, R, exec, screen-ruler
```

Then reload: `hyprctl reload`

## Sway

Add to `~/.config/sway/config`:

```
bindsym $mod+Shift+r exec screen-ruler
```

Then reload: `swaymsg reload`

## Other desktops

Any launcher or shortcut manager that can run an arbitrary command will work. Point it at the `screen-ruler` binary — the app captures the screen, lets you measure, and exits on its own.
