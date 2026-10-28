# Platform features

What the desktop app does on each OS, how, and how far it has been tested. Linux is the
reference implementation; [PORTING.md](PORTING.md) is the plan for the rest. Everything
OS-specific lives in `desktop/core/src/platform/{linux,windows,macos}/` (plus the tray in
`desktop/app/src/gui/tray.rs` and the camera DLL in `desktop/camera/`).

Status as of 2026-10-28. Tested with an iQOO I2218 and a realme RMX5085 (both Android 16)
against Fedora 44 (GNOME, KDE Plasma, Hyprland), Windows 11 Pro 26H2 (build 26300) and, with
the iQOO, macOS 15.8 (MacBook Air M1). Windows means Windows 11 only: Windows 10
isn't supported. macOS: 13 Ventura or newer; [MACOS_PLAN.md](MACOS_PLAN.md) has its plan and
what's left.

| Mark | Meaning |
|---|---|
| ✅ | Works, tested with a real phone (or, where noted, end to end on the PC) |
| 🟡 | Built and running, not fully tested yet (see the note) |
| 🔧 | Written, never compiled on that OS |
| ❌ | Not implemented |

## Overview

| Feature | Linux | Windows | macOS |
|---|---|---|---|
| Builds and runs | ✅ | ✅ | ✅ |
| Pairing (QR / link) | ✅ | ✅ | ✅ |
| Files, both ways | ✅ | ✅ | ✅ |
| Clipboard, both ways | ✅ | ✅ | ✅ |
| Shell | ✅ | ✅ | ✅ |
| Remote (touchpad + keyboard) | ✅ | ✅ | ✅ |
| Cast (scrcpy) | ✅ | ✅ | ✅ |
| Webcam (phone camera) | ✅ | ✅ | ✅ (through OBS) |
| Microphone (phone mic) | ✅ | ✅ (VB-CABLE) | ✅ (BlackHole) |
| Notifications | ✅ | ✅ | ✅ (from the .app) |
| Tray / menu bar | ✅ | ✅ | ✅ |
| Start at login | ✅ | ✅ | 🟡 |
| Setup checks and fixes | ✅ | ✅ | ✅ |
| Control socket (CLI ↔ app) | ✅ | ✅ | ✅ |
| Link over a USB cable (`adb reverse`) | ✅ | ✅ | ✅ |
| Installer | ✅ .deb, .rpm, AppImage | ✅ setup .exe (unsigned) | 🟡 .app built and run; the .dmg job in CI not run yet |

## Wi-Fi or USB

Everything goes over the phone's Dotlink connection to the PC: HTTPS and WebSockets on port
47390. That connection runs over either:

- **Wi-Fi**: phone and PC on the same network; the PC is found again over mDNS when its
  address changes, and its firewall has to let the phone in (Setup checks it). A PC on the
  phone's own hotspot is out of mDNS's reach (Android browses only the Wi-Fi it's on): the
  phone then tries the PC's port on its own networks (`Discovery.scan`) and keeps the one
  that answers with the pinned certificate.
- **USB cable**: with USB debugging allowed on the phone, the PC runs
  `adb reverse tcp:47390 tcp:47390` (`core/src/usb.rs`, every 3 s for each phone plugged in),
  and the phone connects to `127.0.0.1:47390`, which adb carries through the cable. The
  certificate is pinned, so it is still the same, verified PC. No Wi-Fi or firewall rule is
  needed, and it's faster and steadier (50 MB in ~2 s in a test).

The phone tries the cable first. On Wi-Fi it checks for the cable every 5 s and moves over
(the link drops for a few tens of milliseconds); plugging a cable in also retries at once
when the phone was offline. Unplugged, it reconnects over Wi-Fi within seconds. The PC's
Phones page and the phone show which one is in use ("Online · USB" / "USB cable"). The PC
setting `usb_link = false` turns the cable link off.

USB needs, once: Developer options and USB debugging on, and "Allow USB debugging" accepted
for this PC. On Windows some phones also need their maker's USB driver for adb to see them.

| Feature | Wi-Fi (same network) | USB cable | Notes |
|---|---|---|---|
| Pairing | ✅ | ✅ | Over the cable the phone tries `127.0.0.1` first: under a second, against ~20 s over Wi-Fi with virtual adapters. **Pair over USB** (`dotlink pair --usb`) opens the link on the phone through adb, so nothing is scanned (tested on Linux and Windows, two phones at once) |
| Files, both ways | ✅ | ✅ | |
| Clipboard, both ways | ✅ | ✅ | |
| Shell | ✅ | ✅ | |
| Remote | ✅ | ✅ | |
| Webcam | ✅ | ✅ | MJPEG over a WebSocket |
| Microphone | ✅ | ✅ | PCM over a WebSocket |
| Cast | ✅ Wireless debugging (Android 11+, paired once with a 6-digit code) | ✅ USB debugging | USB is preferred when both are there; it also keeps the phone awake (`--stay-awake`). Cast needs the phone's Dotlink connection (either kind) to know which phone is which |

✅ above are from Windows and Linux (Wi-Fi turned off on the phone for the USB tests). The
code is the same on macOS; it only needs adb, which Dotlink already finds or downloads for
Cast.

## Per feature

### Core: pairing, files, mDNS, TLS

Portable code (`core/src/server.rs`, `service.rs`, `tls.rs`): HTTPS + WebSocket on port
47390 with a pinned self-signed certificate, `_dotlink._tcp` over mDNS (`mdns-sd`).

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | portable code | same | same |
| Config dir | `~/.config/dotlink` | `%APPDATA%\dotlink` | `~/Library/Application Support/dotlink` |
| Received files | `~/Downloads/Dotlink` | same | same |
| Status | ✅ | ✅ pair, files both ways, re-pair after reinstall | ✅ pair (QR, and over USB), files both ways with Accept / Decline |

Windows note: the phone tries every PC address in order, so virtual adapters (VMware,
Hyper-V) make pairing slower (~20 s seen); it still connects.

### Control socket

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | Unix socket in `$XDG_RUNTIME_DIR` (user-only) | named pipe `\\.\pipe\dotlink-<user SID>`, DACL with only that SID | Unix socket in `$TMPDIR` (user-only) |
| Status | ✅ | ✅ ACL checked with `Get-Acl`; the CLI prints in a terminal (`AttachConsole`) | ✅ the CLI (`dotlink devices`, `quit`) reaches the app; the socket is in the per-user temp folder from `confstr`, also under launchd |

### Clipboard

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `wl-paste --watch` / `wl-copy` on Wayland (event driven); X11 through `arboard` + XFixes events | `arboard`, polled every 500 ms | `arboard`, polled every 500 ms |
| Status | ✅ | ✅ both ways | ✅ both ways |

### Shell

`core/src/term.rs` (`portable-pty`), xterm.js on the phone. Sessions survive the phone
disconnecting; scrollback is replayed.

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | login shell (`$SHELL -l`) in a PTY | ConPTY; `pwsh.exe` when on `PATH`, else `powershell.exe`; starts in the home folder | login shell in a PTY |
| Tab titles | foreground process group → `/proc/<pid>/comm` | the shell's exe name (`QueryFullProcessImageNameW`) | foreground process group → `proc_name`, or `ps` for root's (`top`, `sudo`) |
| Status | ✅ | ✅ commands, colours, resize, tab title `PWSH` | ✅ two tabs, `top` full screen, resize with the keyboard, tab titles (`ZSH`, `TOP`) |

On the phone, xterm.js (WebGL renderer) draws the text in Geist Mono, then two fallback fonts:
"Dotlink Symbols Mono" (a renamed JuliaMono subset: ⏺ ⎿ ⏵ and other technical symbols,
arrows, dingbats, braille, legacy computing, all one cell wide) and Symbols Nerd Font Mono for
icons, scaled to one cell. Powerline separators and box lines are drawn by xterm itself at the
cell's size.

Known issue (all OSes, phone side): text injected very fast (adb, ~20 ms per character) can
come out doubled in xterm.js. Normal typing is fine.

### Remote (touchpad + keyboard)

Messages in `core/src/input.rs`; key names are the same everywhere (`ENTER`, `F5`, `CTRL`,
`SUPER`…).

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `/dev/uinput` virtual mouse + keyboard (`evdev`) | `SendInput`: relative mouse moves, `WHEEL`/`HWHEEL` in 120ths, virtual-key codes with scan codes and the extended flag, `KEYEVENTF_UNICODE` for text | CGEvent (CoreGraphics), posted at the HID level; media keys as system-defined NSEvents |
| Text | US layout mapping only | any character, any layout | — |
| Permission | udev rule (Setup) | none; can't reach elevated windows (UAC, Task Manager) | — |
| Status | ✅ | ✅ move, click L/R/M, combos, typing, Backspace from the phone; scroll and Unicode (`é ß 😀 日本`) tested on the PC directly | ✅ move, click, drag, scroll, keys, typing from the phone |

### Cast (phone screen on the PC)

`core/src/cast.rs`: scrcpy over adb, USB first, otherwise Wireless debugging found over mDNS.
One cast per phone, several at once; Stop also cancels while connecting.

| | Linux | Windows | macOS |
|---|---|---|---|
| scrcpy / adb | PATH, Android SDK, or the downloaded release (`.tar.gz`) | PATH, `%LOCALAPPDATA%\Android\Sdk`, or the downloaded release (`.zip`, win64 / winarm64) | PATH, `~/Library/Android/sdk`, or the downloaded release (`.tar.gz`) |
| Process cleanup | kill on stop | job object (`KILL_ON_JOB_CLOSE`): ends package-manager shims and their child | kill on stop |
| USB detection | `usb:` in `adb devices -l` | serial isn't `ip:port` or an mDNS name (Windows' adb prints no `usb:`) | `usb:` field |
| Status | ✅ USB and Wi-Fi, stop, cancel, two phones at once | ✅ USB and Wi-Fi, download, stop, cancel, several phones (tested with one) | ✅ with Homebrew's scrcpy and adb |

### Webcam (phone camera)

The phone sends MJPEG frames; only the newest frame matters, the output size is fixed while
apps read, and a placeholder shows while the phone camera is off.

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `v4l2loopback` device "Dotlink Camera", fed by `ffmpeg` | Windows 11 virtual camera (`MFCreateVirtualCamera`, per user, while Dotlink runs). Its media source is `dotlink_camera.dll` (`desktop/camera`), loaded by the Camera Frame Server; Dotlink decodes the JPEG (`zune-jpeg`), fits it into NV12 and shares it through `%ProgramData%\Dotlink\camera.bin` | an MJPEG stream on `127.0.0.1:<port + 1000>` at a secret path (`camera-token` in the config folder), shown by an OBS Browser source (`<token>.html`; a Media Source reading the `.mjpg` works too, with more delay) and passed on by OBS Virtual Camera. A camera of its own (a CoreMediaIO Camera Extension) needs a Developer ID and an Apple entitlement |
| Setup | kernel module + device (Setup, polkit) | Setup → Fix (UAC once): DLL into `%ProgramFiles%\Dotlink`, class registered under HKLM, then the Camera Frame Server stopped so it loads the new DLL (an app using the camera loses it for a moment) | install OBS (Setup → Fix opens its page), add the copied link as a Browser source, Start Virtual Camera; README has the steps |
| Requirements | v4l2loopback, ffmpeg | Windows 11 (build 22000+) | OBS Studio, running |
| Status | ✅ | ✅ the phone's picture in OBS (and any Media Foundation app), 1920×1080 NV12 at 30 or 60 fps, over Wi-Fi and USB | ✅ the phone's camera in OBS (Browser source, less delay than a Media Source) and through OBS Virtual Camera |

Webcam frame rate: 30 or 60 fps, and the phone only offers what the lens in use can do (front
and back are read separately, from the camera's auto-exposure ranges). Both test phones (iQOO
I2218, realme RMX5085) top out at 30 on both lenses, so they show only 30. Their back cameras do
have 60-120 fps high-speed modes, but Android only gives those to preview and video-encoder
outputs, not to the frames the app reads and sends as JPEG; using them would mean sending H.264
instead. The PC takes 60: Linux passes every frame through, and the Windows virtual camera
offers 60 and 30 fps (`RATES` in `desktop/camera/src/source.rs`; OBS lists and plays both).

### Microphone (phone mic)

The phone sends 16-bit PCM in 10 ms chunks; a short backlog is kept and older audio dropped,
so the mic catches up after a Wi-Fi stall.

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `pw-cat` as a PipeWire source "Dotlink Mic" | plays into VB-CABLE's "CABLE Input" through WASAPI (`cpal`), converting rate and channels; apps pick "CABLE Output" | plays into BlackHole 2ch through CoreAudio (`cpal`, shared with Windows in `platform/cpal_sink.rs`); apps pick "BlackHole 2ch" |
| Setup | none (PipeWire) | install VB-CABLE (Setup → Fix opens its download page). Its installer makes "CABLE Input" the default output: set your speakers back as default in Windows' sound settings | Setup → Fix downloads BlackHole 2ch (checksum as in Homebrew's cask) and installs it behind the administrator prompt |
| Status | ✅ | ✅ the phone's audio comes out of "CABLE Output" (recorded and measured), over Wi-Fi and USB | ✅ the phone's voice in QuickTime through "BlackHole 2ch" |

Phone side: the app records with voice processing (`VOICE_COMMUNICATION`). Some phones (vivo /
iQOO) return only zeros from that source outside a call; after 1.5 s of exact silence the app
switches to the plain microphone.

### Notifications

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `notify-send` with Open / Show in folder actions | toasts under the AppUserModelID `dev.dotlink.Dotlink`, registered in HKCU with name and icon; Open / Show in folder buttons are `dotlink:` links (`dotlink url`), so they work from the notification center too | Notification Center through GPUI (UNUserNotificationCenter), posted by the app: Accept / Decline, Open / Show in Finder. Only from the .app, and not while it runs from `/tmp` |
| Status | ✅ | ✅ toasts show as "Dotlink" with its icon; received files carry Open / Show in folder, also clicked from the notification center | ✅ from the .app: Accept / Decline on incoming files answered from the notification, Open / Show in Finder on received ones |

### Tray / menu bar

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | StatusNotifierItem over D-Bus (`ksni`); a symbolic icon the panel recolours (white pixmap as fallback), text-only menu | notification area (`tray-icon`); left click opens the window, menu on right click; the icon white or black with the taskbar's light/dark mode | menu bar item through `tray-icon`, the dots as a template image (drawn in the menu bar's colour); same menu as Windows. The Dock icon shows only while the window is open (`gui/mac.rs`) |
| Status | ✅ GNOME (AppIndicator) and Hyprland (quickshell's tray); started before the bar at login, it registers once the tray appears | ✅ Open, Send clipboard, Cast screen and Quit clicked through; closing the window says once that Dotlink keeps running | ✅ Open, Send files…, Send clipboard, Cast screen, Pair… and Quit clicked through |

### Start at login

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | XDG autostart entry; on Hyprland also a config include | `HKCU\…\CurrentVersion\Run` value `Dotlink` = `"<exe>" --background`; off in Task Manager counts as off | LaunchAgent `~/Library/LaunchAgents/dev.dotlink.Dotlink.plist` running `<exe> --background`; off under Login Items → Allow in the Background counts as off |
| Status | ✅ | ✅ the setting adds and removes the entry; its command starts Dotlink in the tray, without the window | 🟡 the agent file is written and removed; not yet tested across a logout |

### Setup checks

| | Linux | Windows | macOS |
|---|---|---|---|
| Checks | uinput, ffmpeg, v4l2loopback, camera device, `pw-cat`, adb, scrcpy, wl-clipboard, `notify-send`, firewalld / ufw port | firewall rule for the port, Public network (passes when Windows already allows Dotlink there), Dotlink Camera, VB-CABLE, adb, scrcpy | Accessibility (Remote), OBS (Webcam), BlackHole (Microphone), adb, scrcpy |
| Fixes | one `pkexec` run (packages, udev rule, camera service, firewall port) | firewall rule and camera install through one UAC prompt each; adb/scrcpy download without admin; VB-CABLE opens its download page | Accessibility: macOS's prompt, then System Settings; BlackHole: downloaded (checksum as in Homebrew's cask) and installed behind the administrator prompt; OBS: opens its download page; adb/scrcpy download |
| Status | ✅ | ✅ firewall and camera fixes run through UAC | ✅ Accessibility and BlackHole fixed from Setup |

### Window and app integration

| | Linux | Windows | macOS |
|---|---|---|---|
| Window | GPUI; where the desktop doesn't decorate (GNOME) its own title bar in libadwaita's look, with the buttons GNOME's `button-layout` asks for; Hyprland tiles it with its own border. Dark mode followed from the portal, also when started at login | GPUI with Windows decorations; app icon embedded (`app/build.rs`, `winresource`) | GPUI with macOS decorations; an app menu (About, Settings ⌘,, Hide ⌘H, Quit ⌘Q, Edit, Window ⌘W) |
| Console | — | GUI subsystem: no console window; CLI commands print in the terminal they came from | — |
| Closing the window | keeps running in the tray | same, with a one-time notification saying so | keeps running in the menu bar; the Dock icon goes |

### Packaging

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `cargo packager` (.deb, AppImage, pacman), `cargo generate-rpm` | `cargo packager -f nsis`: a per-user setup .exe with `dotlink_camera.dll` next to `dotlink.exe`; install and uninstall tested (settings and pairing are kept). Not Authenticode-signed yet, so SmartScreen and the firewall show "Unknown publisher" | universal .app (lipo) in a .dmg, signed ad hoc: no Developer ID, so Gatekeeper asks once (Open Anyway) and Accessibility is asked again after an update. Updates: the .dmg is mounted and its app swapped in (`update.rs`) when the app's folder is writable |

## Still to do

**Windows**
- Code signing (Authenticode) for the installer and the exe; CI.
- Offer more formats than NV12 if a camera app refuses it.

**Windows, retested after the Linux pass** (2026-10-24, with the iQOO I2218 and the realme
RMX5085). These changed in shared or Windows code and had only been built or type-checked on
Linux:
- [x] Build on Windows: `cargo build`, `cargo test`, `cargo clippy --all-targets` (no
  warnings). The GNOME title bar used gpui's Linux-only `WindowButtonLayout::linux_default`,
  which broke the Windows build; it now spells the fallback out.
- [x] Setup: the camera shows as outdated (the DLL changed); Fix reinstalls it, with OBS
  using the camera. Two fixes on the way: Fix said "administrator rights were refused" after
  a successful install (removing the old DLL failed silently and left PowerShell's exit code
  at 1; the script now ends with `exit 0`), and the frame server kept the old DLL and its
  formats until restarted (Fix now stops it after registering the new DLL).
- [x] Dotlink Camera offers 60 and 30 fps: OBS lists both (and the rates between), both play;
  a 30 fps phone at 60 just repeats frames.
- [x] The tray icon: monochrome dots, white on a dark taskbar, black on a light one, switching
  live when the Windows mode changes (within ~3 s).
- [x] The tray icon is the same size as the icons next to it.
- [x] Pairing over USB: `dotlink pair --usb` opens the pairing screen on the phone; the phone's
  own Pair with <PC> over USB pairs without a tap.
- [x] Pairing over USB from the window (Pair → Pair over USB); two phones at once.
- [x] Unpairing a connected phone (`dotlink unpair`) cuts it off within a second, and the phone
  forgets the PC ("X-PC forgot this phone").
- [x] Unpairing from the window (Phones → Forget).
- [x] Devices: each connected phone's battery and charging state.
- [x] Setup's texts and order (the firewall first, the camera's "pick Dotlink Camera" line).
- [x] Shell: the phone's terminal PIN is per PC (Settings → Paired PC).

**Linux** (Fedora 44, GNOME and Hyprland; no KDE machine at hand). Rechecked 2026-10-24 with
an iQOO I2218 and a realme RMX5085 after the Windows work changed shared code:
- [x] Build: `cargo build -p dotlink`, `--no-default-features`, `cargo test`, `cargo clippy`
  (no warnings).
- [x] Pair (over the cable), files both ways (SHA-256 checked), clipboard both ways, Shell
  (⏺ ⎿ ⏵, Nerd icons, powerline separators, box lines, colours, resize), Remote.
- [x] USB link: "USB cable" on the phone; unplugged, Wi-Fi within a second (files, clipboard,
  mic, Cast); replugged, back to the cable; with the phone's Wi-Fi off, still over the cable.
- [x] Pairing over USB from the PC and from the phone, two phones at once; unpairing cuts a
  connected phone off within a second.
- [x] Cast over USB and Wi-Fi, Stop, Stop while connecting, two phones at once.
- [x] Webcam and mic; the phone's tab rail in landscape; battery on the Phones page.
- [x] Tray (monochrome icon, text menu), start at login, `dotlink quit`, Setup page.
- [x] Hyprland: tiled window without our title bar, `wl-paste --watch`, started at login by the
  `hyprland.lua` hook, tray in quickshell.
- [ ] `scripts/install.sh` on a clean user.

**macOS** (details in [MACOS_PLAN.md](MACOS_PLAN.md))
- Run the CI workflows (desktop build on every OS, the release's .dmg job).
- Test: start at login across a logout.
- Developer ID signing and notarization, then a Camera Extension of our own instead of OBS.

**Phone app**
- Hide the Webcam and Mic switches when the PC reports it can't do them.
- 60 fps webcam on phones whose cameras only reach 60 in high-speed mode: needs H.264 instead
  of JPEG frames.
