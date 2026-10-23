# Dotlink

Link an Android phone with a Linux PC over your Wi-Fi. No account, no cloud. Made on Hyprland, works on any desktop.

| Feature | Phone → PC | PC → phone |
|---|---|---|
| Files | Share → Dotlink, or **Send** in the app. Saved to `~/Downloads/Dotlink`, SHA-256 checked | `dotlink send FILE…`. Saved to `Download/Dotlink` |
| Clipboard | When you open the app, Share text → Dotlink, or the notification's *Send clipboard* | Automatic (password-manager entries are skipped) |
| Remote | Touchpad + keyboard (uinput) | |
| Shell | A real shell on the PC (xterm.js). The session survives reconnects | |
| Webcam | Phone camera → **Dotlink Camera** (v4l2loopback) | |
| Microphone | Phone mic → **Dotlink Mic** (PipeWire source) | |

```
daemon/    Rust daemon + CLI (dotlink)
android/   Kotlin + Jetpack Compose app, Dotline UI kit from dotnote
scripts/   install.sh
```

## Works on

| | Hyprland, Sway, niri, river | KDE Plasma | GNOME | X11 (XFCE, Cinnamon, MATE, i3…) |
|---|---|---|---|---|
| Files, shell, notifications | ✅ | ✅ | ✅ | ✅ |
| Remote (uinput) | ✅ | ✅ | ✅ | ✅ |
| Mic (PipeWire) | ✅ | ✅ | ✅ | ✅ |
| Webcam (v4l2loopback) | ✅ | ✅ | ✅ | ✅ |
| Clipboard | ✅ wl-clipboard | ✅ wl-clipboard | ✅ via XWayland | ✅ X11 |

Clipboard picks its backend by itself: `wl-paste --watch` where the compositor has the data-control
protocol, otherwise the X11 clipboard (on GNOME, Mutter keeps XWayland's clipboard in sync with
Wayland's). Force one with `DOTLINK_CLIPBOARD=x11`.

## PC setup

Needs: `ffmpeg`, PipeWire (`pw-cat`), `libnotify`, write access to `/dev/uinput`, `wl-clipboard` on
wlroots/KDE, and `v4l2loopback` for the webcam.

```sh
scripts/install.sh --camera
```

It builds and installs `~/.local/bin/dotlink`, adds an XDG autostart entry (GNOME, KDE, XFCE,
Cinnamon, MATE, LXQt and Budgie start it at login), and checks uinput access, the firewall
(firewalld/ufw) and missing packages for dnf, apt, pacman and zypper. `--camera` creates
"Dotlink Camera" and keeps it across reboots (sudo).

Window managers that ignore XDG autostart need one line:

```
Hyprland   exec-once = dotlink daemon        (Lua config: hl.exec_cmd("dotlink daemon"))
Sway/i3    exec dotlink daemon
niri       spawn-at-startup "dotlink" "daemon"
```

Optional Hyprland keybinds:

```lua
hl.bind("SUPER + ALT + C", hl.dsp.exec_cmd("dotlink clip"))
hl.bind("SUPER + ALT + S", hl.dsp.exec_cmd("zenity --file-selection --multiple --separator='\\n' | xargs -d '\\n' -r dotlink send"))
```

## Pairing

1. Install the app on the phone: `cd android && ./gradlew installDebug`. Or copy `app/build/outputs/apk/debug/app-debug.apk` to the phone.
2. On the PC, run `dotlink pair` and scan the QR code with the app. You can paste the link instead.

The QR code holds the PC's addresses, its certificate fingerprint and a one-time secret (valid for 5 minutes). The phone pins that certificate and gets a token. Later connections use TLS to the pinned certificate, plus the token. If the PC's IP changes, the phone finds it again with mDNS (`_dotlink._tcp`).

## CLI

```
dotlink daemon            run the service
dotlink pair              show a pairing QR code
dotlink send FILE...      send files to the phone
dotlink clip              send the PC clipboard to the phone
dotlink devices           list paired phones
dotlink unpair NAME|ID    forget a phone
```

## Config: `~/.config/dotlink/config.toml`

```toml
port = 47390
download_dir = "~/Downloads/Dotlink"
clipboard_sync = true
terminal = true          # false disables the shell feature
terminal_pin = ""        # if set, the app must send this PIN (Settings → Terminal PIN)
shell = ""               # default: $SHELL
camera_device = ""       # default: the v4l2loopback device named "Dotlink Camera"
camera_size = "1920x1080" # fixed size of Dotlink Camera; phone frames are scaled into it
```

`state.json` (paired phones, token hashes) and `cert.pem`/`key.pem` are kept in the same folder.

## Notes

- **Keyboard layout:** remote typing maps characters with a US layout, so other layouts may type different characters. Special keys and shortcuts work on any layout.
- **Clipboard on Android:** Android 10+ lets an app read the clipboard only while it is in front. That is why phone → PC happens when you open the app or tap the notification button.
- **Webcam:** the phone sends MJPEG, and ffmpeg writes it to v4l2loopback. The device always has a picture ("phone camera off" when idle), so it stays listed in OBS and browsers. Its size never changes while apps read it.
- **Security:** anyone with a paired phone gets a shell on your PC. Set `terminal = false` or a `terminal_pin` if that's a concern. `dotlink unpair` revokes a phone right away.
