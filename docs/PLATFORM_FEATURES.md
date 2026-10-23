# Platform features

What the desktop app does on each OS, how, and how far it has been tested. Linux is the
reference implementation; [PORTING.md](PORTING.md) is the plan for the rest. Everything
OS-specific lives in `desktop/core/src/platform/{linux,windows,macos}/` (plus the tray in
`desktop/app/src/gui/tray.rs` and the camera DLL in `desktop/camera/`).

Status as of 2026-10-23.

| Mark | Meaning |
|---|---|
| ✅ | Works, tested with a real phone (or, where noted, end to end on the PC) |
| 🟡 | Built and running, not fully tested yet (see the note) |
| 🔧 | Written, never compiled on that OS |
| ❌ | Not implemented |

## Overview

| Feature | Linux | Windows | macOS |
|---|---|---|---|
| Builds and runs | ✅ | ✅ | 🔧 |
| Pairing (QR / link) | ✅ | ✅ | 🔧 |
| Files, both ways | ✅ | ✅ | 🔧 |
| Clipboard, both ways | ✅ | ✅ | 🔧 |
| Shell | ✅ | ✅ | 🔧 |
| Remote (touchpad + keyboard) | ✅ | ✅ | ❌ |
| Cast (scrcpy) | ✅ | ✅ USB, 🟡 Wi-Fi | 🔧 |
| Webcam (phone camera) | ✅ | 🟡 | ❌ |
| Microphone (phone mic) | ✅ | 🟡 | ❌ |
| Notifications | ✅ | 🟡 | 🔧 text only |
| Tray / menu bar | ✅ | 🟡 | ❌ |
| Start at login | ✅ | 🟡 | ❌ |
| Setup checks and fixes | ✅ | ✅ | ❌ |
| Control socket (CLI ↔ app) | ✅ | ✅ | 🔧 |
| Link over a USB cable (`adb reverse`) | 🟡 | ✅ | 🔧 |
| Installer | ✅ .deb, .rpm, AppImage | ❌ | ❌ |

## Wi-Fi or USB

Everything goes over the phone's Dotlink connection to the PC: HTTPS and WebSockets on port
47390. That connection runs over either:

- **Wi-Fi**: phone and PC on the same network; the PC is found again over mDNS when its
  address changes, and its firewall has to let the phone in (Setup checks it).
- **USB cable**: with USB debugging allowed on the phone, the PC runs
  `adb reverse tcp:47390 tcp:47390` (`core/src/usb.rs`, every 3 s for each phone plugged in),
  and the phone connects to `127.0.0.1:47390`, which adb carries through the cable. The
  certificate is pinned, so it is still the same, verified PC. No Wi-Fi or firewall rule is
  needed, and it's faster and steadier (50 MB in ~2 s in a test).

The phone tries the cable first. On Wi-Fi it checks for the cable every 5 s and moves over
(the link drops for a few tens of milliseconds); plugging a cable in also retries at once
when the phone was offline. Unplugged, it reconnects over Wi-Fi within seconds. The PC's
Devices page and the phone show which one is in use ("Online · USB" / "USB cable"). The PC
setting `usb_link = false` turns the cable link off.

USB needs, once: Developer options and USB debugging on, and "Allow USB debugging" accepted
for this PC. On Windows some phones also need their maker's USB driver for adb to see them.

| Feature | Wi-Fi (same network) | USB cable | Notes |
|---|---|---|---|
| Pairing | ✅ | 🟡 | Over the cable the phone tries `127.0.0.1` before the addresses in the QR code |
| Files, both ways | ✅ | ✅ PC → phone, 🟡 phone → PC | |
| Clipboard, both ways | ✅ | ✅ PC → phone, 🟡 phone → PC | |
| Shell | ✅ | 🟡 | Same connection, not tried over USB yet |
| Remote | ✅ | 🟡 | Same connection, not tried over USB yet |
| Webcam | ✅ | 🟡 | MJPEG over a WebSocket |
| Microphone | ✅ | 🟡 | PCM over a WebSocket |
| Cast | ✅ Wireless debugging (Android 11+, paired once with a 6-digit code) | ✅ USB debugging | USB is preferred when both are there; it also keeps the phone awake (`--stay-awake`). Cast needs the phone's Dotlink connection (either kind) to know which phone is which |

✅ / 🟡 above are from Windows (Wi-Fi turned off on the phone for the USB tests). The code is
the same on Linux and macOS; it only needs adb, which Dotlink already finds or downloads for
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
| Status | ✅ | ✅ pair, files both ways, re-pair after reinstall | 🔧 |

Windows note: the phone tries every PC address in order, so virtual adapters (VMware,
Hyper-V) make pairing slower (~20 s seen); it still connects.

### Control socket

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | Unix socket in `$XDG_RUNTIME_DIR` (user-only) | named pipe `\\.\pipe\dotlink-<user SID>`, DACL with only that SID | Unix socket in `$TMPDIR` (user-only) |
| Status | ✅ | ✅ ACL checked with `Get-Acl`; the CLI prints in a terminal (`AttachConsole`) | 🔧 |

### Clipboard

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `wl-paste --watch` / `wl-copy` on Wayland (event driven); X11 through `arboard` + XFixes events | `arboard`, polled every 500 ms | `arboard`, polled every 500 ms |
| Status | ✅ | ✅ both ways | 🔧 |

### Shell

`core/src/term.rs` (`portable-pty`), xterm.js on the phone. Sessions survive the phone
disconnecting; scrollback is replayed.

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | login shell (`$SHELL -l`) in a PTY | ConPTY; `pwsh.exe` when on `PATH`, else `powershell.exe`; starts in the home folder | login shell in a PTY |
| Tab titles | foreground process group → `/proc/<pid>/comm` | the shell's exe name (`QueryFullProcessImageNameW`) | ❌ (`None`) |
| Status | ✅ | ✅ commands, colours, resize, tab title `PWSH` | 🔧 |

Known issue (all OSes, phone side): text injected very fast (adb, ~20 ms per character) can
come out doubled in xterm.js. Normal typing is fine.

### Remote (touchpad + keyboard)

Messages in `core/src/input.rs`; key names are the same everywhere (`ENTER`, `F5`, `CTRL`,
`SUPER`…).

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `/dev/uinput` virtual mouse + keyboard (`evdev`) | `SendInput`: relative mouse moves, `WHEEL`/`HWHEEL` in 120ths, virtual-key codes with scan codes and the extended flag, `KEYEVENTF_UNICODE` for text | ❌ (plan: CGEvent + Accessibility) |
| Text | US layout mapping only | any character, any layout | — |
| Permission | udev rule (Setup) | none; can't reach elevated windows (UAC, Task Manager) | — |
| Status | ✅ | ✅ move, click L/R/M, combos, typing, Backspace from the phone; scroll and Unicode (`é ß 😀 日本`) tested on the PC directly | ❌ |

### Cast (phone screen on the PC)

`core/src/cast.rs`: scrcpy over adb, USB first, otherwise Wireless debugging found over mDNS.
One cast per phone, several at once; Stop also cancels while connecting.

| | Linux | Windows | macOS |
|---|---|---|---|
| scrcpy / adb | PATH, Android SDK, or the downloaded release (`.tar.gz`) | PATH, `%LOCALAPPDATA%\Android\Sdk`, or the downloaded release (`.zip`, win64 / winarm64) | PATH, `~/Library/Android/sdk`, or the downloaded release (`.tar.gz`) |
| Process cleanup | kill on stop | job object (`KILL_ON_JOB_CLOSE`): ends package-manager shims and their child | kill on stop |
| USB detection | `usb:` in `adb devices -l` | serial isn't `ip:port` or an mDNS name (Windows' adb prints no `usb:`) | `usb:` field |
| Status | ✅ | ✅ USB, download, stop, cancel, several phones (tested with one); 🟡 Wi-Fi | 🔧 |

### Webcam (phone camera)

The phone sends MJPEG frames; only the newest frame matters, the output size is fixed while
apps read, and a placeholder shows while the phone camera is off.

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `v4l2loopback` device "Dotlink Camera", fed by `ffmpeg` | Windows 11 virtual camera (`MFCreateVirtualCamera`, per user, while Dotlink runs). Its media source is `dotlink_camera.dll` (`desktop/camera`), loaded by the Camera Frame Server; Dotlink decodes the JPEG (`zune-jpeg`), fits it into NV12 and shares it through `%ProgramData%\Dotlink\camera.bin` | ❌ (plan: CoreMediaIO Camera Extension) |
| Setup | kernel module + device (Setup, polkit) | Setup → Fix (UAC once): DLL into `%ProgramFiles%\Dotlink`, class registered under HKLM | — |
| Requirements | v4l2loopback, ffmpeg | Windows 11 (build 22000+); Windows 10 isn't supported | — |
| Status | ✅ | 🟡 the camera is listed and delivers 1920×1080 NV12 at 30 fps to Media Foundation apps; phone frames arrive in it. Still to check: a real picture in the Camera app / a call | ❌ |

### Microphone (phone mic)

The phone sends 16-bit PCM in 10 ms chunks; a short backlog is kept and older audio dropped,
so the mic catches up after a Wi-Fi stall.

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `pw-cat` as a PipeWire source "Dotlink Mic" | plays into VB-CABLE's "CABLE Input" through WASAPI (`cpal`), converting rate and channels; apps pick "CABLE Output" | ❌ (plan: BlackHole, later an AudioServerPlugIn) |
| Setup | none (PipeWire) | install VB-CABLE (Setup → Fix opens its download page) | — |
| Status | ✅ | 🟡 built, converter unit-tested; waits for VB-CABLE to be installed | ❌ |

### Notifications

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `notify-send` with Open / Show in folder actions | toasts (`notify-rust`) under the AppUserModelID `dev.dotlink.Dotlink`, registered in HKCU with name and icon; Open / Show in folder buttons | `notify-rust`, text only |
| Status | ✅ | 🟡 to check: toast with buttons on a received file | 🔧 |

### Tray / menu bar

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | StatusNotifierItem over D-Bus (`ksni`) | notification area (`tray-icon`); left click opens the window, menu on right click | ❌ |
| Status | ✅ | 🟡 runs; menu actions not all clicked through yet | ❌ |

### Start at login

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | XDG autostart entry; on Hyprland also a config include | `HKCU\…\CurrentVersion\Run` value `Dotlink` = `"<exe>" --background`; off in Task Manager counts as off | ❌ (plan: SMAppService / LaunchAgent) |
| Status | ✅ | 🟡 not tested yet | ❌ |

### Setup checks

| | Linux | Windows | macOS |
|---|---|---|---|
| Checks | uinput, ffmpeg, v4l2loopback, camera device, `pw-cat`, adb, scrcpy, wl-clipboard, `notify-send`, firewalld / ufw port | firewall rule for the port, Public network (passes when Windows already allows Dotlink there), Dotlink Camera, VB-CABLE, adb, scrcpy | ❌ |
| Fixes | one `pkexec` run (packages, udev rule, camera service, firewall port) | firewall rule and camera install through one UAC prompt each; adb/scrcpy download without admin; VB-CABLE opens its download page | ❌ |
| Status | ✅ | ✅ firewall and camera fixes run through UAC | ❌ |

### Window and app integration

| | Linux | Windows | macOS |
|---|---|---|---|
| Window | GPUI; own title bar where the desktop doesn't decorate (GNOME) | GPUI with Windows decorations; app icon embedded (`app/build.rs`, `winresource`) | 🔧 GPUI |
| Console | — | GUI subsystem: no console window; CLI commands print in the terminal they came from | — |
| Closing the window | keeps running in the tray | same, with a one-time notification saying so | — |

### Packaging

| | Linux | Windows | macOS |
|---|---|---|---|
| Method | `cargo packager` (.deb, AppImage, pacman), `cargo generate-rpm` | ❌ (plan: NSIS per-user install, shipping `dotlink_camera.dll` next to `dotlink.exe`, Authenticode signing) | ❌ (plan: signed, notarized .app in a .dmg) |

## Still to do

**Windows**
- Webcam: confirm a real picture in the Camera app and a call; offer more formats than NV12
  if an app refuses it.
- Microphone: test with VB-CABLE installed.
- Check toasts, the tray menu, start at login, and Cast over Wi-Fi.
- Installer, CI.
- Windows 10 webcam (a DirectShow source filter) if it's wanted.

**macOS**
- Build it first (Step 1 in [PORTING.md](PORTING.md)), then everything marked ❌ above.

**Phone app**
- Hide the Webcam and Mic switches when the PC reports it can't do them.
