# Dotlink

Link an Android phone with a Linux PC over your Wi-Fi. No account, no cloud. Made on Hyprland, works on any desktop.
On the PC it's one app: the service, a window and a tray icon. Windows and macOS are next (see [Platforms](#platforms)).

| Feature | Phone → PC | PC → phone |
|---|---|---|
| Files | Share → Dotlink, or **Send** in the app. Saved to `~/Downloads/Dotlink`, SHA-256 checked | Drop files on the window, the tray's *Send files…*, or `dotlink send FILE…`. Saved to `Download/Dotlink` |
| Clipboard | When you open the app, Share text → Dotlink, or the notification's *Send clipboard* | Automatic (password-manager entries are skipped) |
| Remote | Touchpad + keyboard (uinput) | |
| Shell | A real shell on the PC (xterm.js). The session survives reconnects | |
| Webcam | Phone camera → **Dotlink Camera** (v4l2loopback) | |
| Microphone | Phone mic → **Dotlink Mic** (PipeWire source) | |
| Cast | | Phone screen in a window with mouse and keyboard ([scrcpy](https://github.com/Genymobile/scrcpy), USB or Wi-Fi) |

```
desktop/         Rust workspace for the PC app
  core/          the service: pairing, HTTPS/WebSocket API, control socket (dotlink-core)
    src/platform/  per-OS code: linux/ (complete), windows/ and macos/ (stubs + TODO)
  app/           the `dotlink` binary: window (GPUI) + tray + CLI
android/         Kotlin + Jetpack Compose app, Dotline UI kit from dotnote
scripts/         install.sh
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
wlroots/KDE, and `v4l2loopback` for the webcam. Building the window needs the usual GPUI libraries
(`libxkbcommon(-x11)`, `wayland`, `libxcb`, `fontconfig`, `freetype` development packages);
install.sh lists the exact packages for your distro.

```sh
scripts/install.sh --camera
```

It builds and installs `~/.local/bin/dotlink`, adds Dotlink to the app menu, adds an XDG autostart
entry (GNOME, KDE, XFCE, Cinnamon, MATE, LXQt and Budgie start it at login, in the tray), and checks
uinput access, the firewall (firewalld/ufw) and missing packages for dnf, apt, pacman and zypper.
`--camera` creates "Dotlink Camera" and keeps it across reboots (sudo). `--headless` builds without
the window and tray (service + CLI only).

Then open **Dotlink**. Its **Setup** page shows what each feature still needs and fixes uinput access,
the webcam device and the firewalld port with one click (it asks for your password through polkit;
on Hyprland that needs a polkit agent such as `hyprpolkitagent`).

Window managers that ignore XDG autostart need one line:

```
Hyprland   exec-once = dotlink --background        (Lua config: hl.exec_cmd("dotlink --background"))
Sway/i3    exec dotlink --background
niri       spawn-at-startup "dotlink" "--background"
```

### The app

- Closing the window keeps Dotlink running in the tray; the phone stays connected. Quit from the
  tray or Settings. Opening Dotlink again brings the window back.
- The tray works on KDE, Waybar (Hyprland, Sway) and GNOME with the AppIndicator extension. Without
  a tray, Dotlink still runs in the background after you close the window.
- `dotlink daemon` runs the same service without a window, for setups without a tray or a desktop.
  Opening Dotlink while it runs gives you a window that controls it.

Optional Hyprland keybinds:

```lua
hl.bind("SUPER + ALT + C", hl.dsp.exec_cmd("dotlink clip"))
hl.bind("SUPER + ALT + S", hl.dsp.exec_cmd("zenity --file-selection --multiple --separator='\\n' | xargs -d '\\n' -r dotlink send"))
```

## Pairing

1. Install the app on the phone: `cd android && ./gradlew installDebug`. Or copy `app/build/outputs/apk/debug/app-debug.apk` to the phone.
2. On the PC, open **Pair a phone** in Dotlink (or run `dotlink pair`) and scan the QR code with the app. You can paste the link instead.

The QR code holds the PC's addresses, its certificate fingerprint and a one-time secret (valid for 5 minutes). The phone pins that certificate and gets a token. Later connections use TLS to the pinned certificate, plus the token. If the PC's IP changes, the phone finds it again with mDNS (`_dotlink._tcp`).

## CLI

```
dotlink                   open the app (or bring its window up)
dotlink --background      start in the tray, without the window (used at login)
dotlink daemon            run the service without a window
dotlink pair              show a pairing QR code
dotlink send FILE...      send files to the phone
dotlink clip              send the PC clipboard to the phone
dotlink devices           list paired phones
dotlink unpair NAME|ID    forget a phone
dotlink camera on|off     start/stop the phone camera as Dotlink Camera
dotlink mic on|off        start/stop the phone mic as Dotlink Mic
dotlink cast [stop]       show the phone screen on the PC (scrcpy)
dotlink cast pair CODE    pair with the phone's Wireless debugging once
dotlink quit              stop the running app or daemon
```

The commands talk to whichever is running (app or daemon) over a socket in `$XDG_RUNTIME_DIR`.

## Config: `~/.config/dotlink/config.toml`

The Settings page edits the same file.

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
- **Cast:** uses the PC's `scrcpy` and `adb` when they're installed. Otherwise the first Cast (or Fix in Setup) downloads the official scrcpy release, which includes adb, into `~/.local/share/dotlink/scrcpy/` (18 MB, SHA-256 checked, no root). The phone needs Developer options. Over USB, allow USB debugging once and press Cast. Over Wi-Fi (Android 11+), turn on Wireless debugging, then pair once: on the phone choose *Pair device with pairing code* and type the code in Dotlink. After that Cast finds the phone by itself (mDNS); if Wireless debugging is off, the phone gets a notification that opens it.
- **Webcam and mic from the PC:** Start in Devices (or `dotlink camera on`) starts them right away while the Dotlink app is open on the phone. Otherwise Android doesn't let a background app turn on the camera or mic, so the phone shows a notification and one tap on **Start** does it.
- **Webcam:** the phone sends MJPEG, and ffmpeg writes it to v4l2loopback. The device always has a picture ("phone camera off" when idle), so it stays listed in OBS and browsers. Its size never changes while apps read it.
- **Security:** anyone with a paired phone gets a shell on your PC. Turn the Shell off or set a PIN in Settings (`terminal = false` / `terminal_pin`) if that's a concern. Forget a phone in Devices (or `dotlink unpair`) to revoke it right away.

## Platforms

| | Linux | Windows | macOS |
|---|---|---|---|
| App (window), pairing, files, shell | ✅ | written, not built yet | written, not built yet |
| Clipboard, notifications | ✅ | written (arboard, notify-rust) | written (arboard, notify-rust) |
| Tray | ✅ | TODO | TODO |
| Remote, start at login, Setup checks | ✅ | TODO | TODO |
| Webcam, microphone | ✅ | TODO (needs a driver) | TODO (needs a system extension) |

Everything OS-specific lives in `desktop/core/src/platform/`. `linux/` is the reference;
`windows/mod.rs` and `macos/mod.rs` are stubs (not compiled yet: build them on those systems first) that list what to build in their TODO comments
(SendInput / CGEvent for the remote, login items, virtual camera and mic). The tray on those two needs
the `tray-icon` crate (`desktop/app/src/gui/tray.rs`), and the named pipe on Windows needs a
user-only security descriptor (`desktop/core/src/ipc.rs`). Search for `TODO(windows)` and `TODO(macos)`.

## Development

```sh
cd desktop
cargo run -p dotlink                          # the app
cargo run -p dotlink -- daemon                # headless
cargo build -p dotlink --no-default-features  # no GPUI at all
```

Packages (into `desktop/target/`):

```sh
cargo install cargo-packager cargo-generate-rpm
NO_STRIP=1 cargo packager --release -f deb,appimage   # packages/  (.deb, .AppImage)
cargo generate-rpm -p app                            # generate-rpm/  (.rpm, after a release build)
```

They need the glibc of the system they were built on or newer, so build releases on the oldest
distro you want to support (e.g. in a Debian stable or Ubuntu LTS container).
