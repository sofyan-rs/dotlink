# Porting the desktop app to Windows and macOS

Linux is complete and is the reference. **Windows 11 is done**: every step below, tested with
a phone; what's left there is code signing and running CI. The macOS module was written on
Linux and **has never been compiled there**. This page is the plan:
what already works in principle, what each OS still needs, in which order, and how to test
it. [ARCHITECTURE.md](ARCHITECTURE.md) explains the pieces; this page assumes it.

The Android app needs no changes: it already reads the PC's OS from `hello` (`os`:
`"linux"`, `"windows"`, `"macos"`) and labels the Remote's modifier keys by it
(`RemoteScreen.kt`). On macOS the ⌘ key is sent as `SUPER`, ⌥ as `ALT`.

## Where things stand

| Piece | Where | Windows | macOS |
|---|---|---|---|
| Service, pairing, files, mDNS, TLS | `core/src/server.rs`, `service.rs` | done, tested with a phone | portable, untested |
| Control socket (CLI ↔ service ↔ window) | `core/src/ipc.rs` | named pipe `dotlink-<SID>`, user-only ACL | Unix socket in `$TMPDIR` |
| Shell | `core/src/term.rs` (portable-pty) | ConPTY + `pwsh.exe` or `powershell.exe` | login shell |
| Clipboard | `platform/polling_clipboard.rs` (arboard, 500 ms poll) | works | works |
| Notifications | `notify-rust` | toasts under `dev.dotlink.Dotlink` (registered in HKCU), Open / Show in folder | text only, no buttons |
| Open file / Show in folder | `platform/{windows,macos}/mod.rs` | `explorer` | `open`, `open -R` |
| Window | `app/src/gui/` (GPUI) | done (icon embedded by `app/build.rs`) | Cargo features gated, untested |
| Tray | `app/src/gui/tray.rs` | done (`tray-icon`) | **TODO** (menu bar) |
| Remote (mouse + keyboard) | `Input` | done (SendInput) | **TODO** (CGEvent + Accessibility) |
| Start at login | `autostart` | done (Run key, honours Task Manager) | **TODO** (SMAppService / LaunchAgent) |
| Setup checks | `setup` | done (firewall rule, Public network, input, camera, VB-CABLE, adb/scrcpy, notifications) | **TODO** (Accessibility, Local Network) |
| Cast (scrcpy) | `core/src/cast.rs` | done (.zip download, SDK adb) | download works (tar.gz) |
| Webcam | `CameraHub` | done (Windows 11 virtual camera, `desktop/camera` DLL) | **TODO**: Camera Extension |
| Microphone | `MicSink` | done (plays into VB-CABLE through WASAPI) | **TODO**: virtual audio device |
| Installer | `app/Cargo.toml` packager section | done (NSIS setup .exe, per user; not signed yet) | **TODO**: signed .app in .dmg |

`platform::features()` tells the window what to show. Each OS starts with `remote`,
`camera` and `mic` set to `false`; turn a flag on in the same commit that makes the feature
work.

## Order of work

Each step leaves a usable app, so it can ship after any of them.

1. **It builds.** CI for both OSes (below), then `cargo build -p dotlink` and `cargo test`
   until they're clean. Expect small fixes: the stubs were never type-checked there.
2. **Core works.** Pair a phone, send files both ways, clipboard both ways, Shell. Fix the
   named-pipe ACL (Windows) before anything leaves your machine.
3. **Feels native.** Tray / menu bar, start at login, notifications under the app's name,
   the app menu and Dock behaviour on macOS, the window icon on Windows.
4. **Remote.** SendInput / CGEvent, plus the Accessibility check on macOS.
5. **Cast.** Windows scrcpy download; on both, finding `adb`.
6. **Installers.** Signed and installable: NSIS or MSI, a notarized .dmg.
7. **Webcam and mic.** The largest pieces: both need a driver or system extension.

## Step 1: building and CI

There is no CI yet. Add `.github/workflows/desktop.yml` with a matrix of `ubuntu-latest`,
`windows-latest` and `macos-latest` (Apple Silicon) running, in `desktop/`:

```sh
cargo build -p dotlink
cargo build -p dotlink --no-default-features   # service + CLI only, no GPUI
cargo test
cargo clippy -p dotlink -- -D warnings   # clean on Linux
```

Ubuntu needs the GPUI libraries listed in [CONTRIBUTION.md](CONTRIBUTION.md#desktop-rust).
CI can't test the hardware parts, but it keeps the stubs compiling while Linux moves on.

### Fixed on the first Windows build

- **GPUI features** (`app/Cargo.toml`): `x11` and `wayland` are only enabled on Linux;
  `font-kit` / `runtime_shaders` for all. `runtime_shaders` compiles the Metal shaders at run
  time, so macOS builds don't need full Xcode, only the Command Line Tools.
- **Windows console.** `app/src/main.rs` is a `windows_subsystem = "windows"` program (no
  black console window) and calls `AttachConsole(ATTACH_PARENT_PROCESS)` for CLI commands, so
  `dotlink devices` in a terminal still prints.
- **Shutdown.** `service.rs::terminate()` stops on SIGTERM (Linux, macOS) and on
  `ctrl_close` / `ctrl_logoff` / `ctrl_shutdown` (Windows).

## Step 2: core

### Control socket (Windows, before shipping)

`core/src/ipc.rs` uses a named pipe `dotlink-<USERNAME>`. By default other users on the
machine can open it, and through it pair a phone or read the clipboard. Create the listener
with a security descriptor that grants only the current user's SID: interprocess's
`ListenerOptionsExt::security_descriptor`, built from an SDDL string like
`D:P(A;;GA;;;<user SID>)`. Also check that a second user's Dotlink gets its own pipe and
doesn't see "already running".

macOS: `runtime_dir()` falls back to `std::env::temp_dir()`, which is `$TMPDIR`
(`/var/folders/…/T/`): per user and mode 0700, so the socket is already private. Keep it.

### Files on disk

`dirs` picks the OS's own places; the ARCHITECTURE table only lists Linux paths.

| | Windows | macOS |
|---|---|---|
| config, state, certificate | `%APPDATA%\dotlink\` | `~/Library/Application Support/dotlink/` |
| scrcpy download | `%APPDATA%\dotlink\scrcpy\` | `~/Library/Application Support/dotlink/scrcpy/` |
| received files | `~/Downloads/Dotlink` (same default) | same |

`config::set_private` does nothing off Unix. On Windows that's fine while `%APPDATA%` is
used (its ACL is already user-only); revisit it only if the config dir becomes configurable.

### Shell

- Windows: `default_shell()` returns `powershell.exe -NoLogo`. Prefer `pwsh.exe` when it is
  on `PATH` (PowerShell 7), then `powershell.exe`. Make sure the PTY's working directory is
  the user's home, not `C:\Windows\System32` (where services start).
- `process_name(pid)` (tab titles): ConPTY has no foreground process group, so `term.rs`
  passes the child's pid; the shell's exe name is enough. Windows:
  `QueryFullProcessImageNameW` and keep the file stem. macOS: `proc_name` from libproc
  (the `libproc` crate) on the foreground process group from `term.rs`.
- Both use xterm.js on the phone, and ConPTY speaks VT sequences, so colours and full-screen
  programs should work. Test `vim`/`nvim` and a resize.

### Clipboard

`polling_clipboard.rs` works on both as it is. Optional improvements, after everything else:

- Windows: `AddClipboardFormatListener` on a message-only window gives change events instead
  of a 500 ms poll (same role as XFixes on Linux; send `Cmd::Changed`).
- macOS: there are no events. Compare `NSPasteboard.changeCount` instead of reading the text
  every time; it's cheaper.
- Password managers: skip entries marked secret. Windows: the
  `ExcludeClipboardContentFromMonitorProcessing` and `CanIncludeInClipboardHistory` (= 0)
  formats. macOS: the `org.nspasteboard.ConcealedType` / `TransientType` types.

### Network

- **Windows Firewall** asks "Allow access?" the first time the service listens on 47390,
  and blocks it on Public networks. The installer should add the rule (Step 6) and Setup
  should check it (below).
- **macOS** asks to allow incoming connections for unsigned apps on every rebuild; a signed
  app is asked once. On macOS 15+ the **Local Network** permission gates mDNS and LAN
  connections: `Info.plist` must have `NSLocalNetworkUsageDescription` and
  `NSBonjourServices` = `_dotlink._tcp`, `_adb-tls-connect._tcp`, `_adb-tls-pairing._tcp`,
  or browsing fails silently (Cast won't find the phone). This only applies to a bundled
  `.app`; `cargo run` from Terminal inherits Terminal's permission.
- `mdns-sd` binds UDP 5353 next to the OS's own responder (Windows 11 and mDNSResponder on
  macOS). Check that the phone still finds the PC after its IP changes.

## Step 3: feels native

### Tray / menu bar

`app/src/gui/tray.rs` returns `false` off Linux. Use the `tray-icon` crate (with `muda` for
the menu) and send the same `TrayAction`s:

- Create it on the main thread after GPUI has started (inside `Application::run`). GPUI owns
  the run loop on both OSes; `tray-icon` only needs the main thread to pump messages, which
  GPUI does.
- Menu events arrive on `MenuEvent::receiver()` / `TrayIconEvent::receiver()`; forward them
  into the existing `UnboundedSender<TrayAction>`. Left click on Windows opens the window.
- Icons: Windows uses `assets/icons/32x32.png` (64 on high DPI). The macOS menu bar wants a
  black-only **template** image (`with_icon_as_template(true)`) so it follows light/dark:
  make `assets/tray-template.png` (22 pt, @2x at 44 px) from the dot glyph without the black
  tile, like `android/.../drawable/ic_notification.xml`.
- Once a tray exists, `has_tray` in `gui/mod.rs` is true and the "still running" notice goes
  away by itself.

### macOS app behaviour (`app/src/gui/mod.rs`)

- App menu with About Dotlink, Settings… (⌘,), Hide (⌘H), Quit Dotlink (⌘Q) via
  `cx.set_menus`. Quit stops the service, like Quit in the tray.
- `cx.on_reopen`: clicking the Dock icon with no window open shows it again.
- Dock icon only while a window is open: `NSApplication.setActivationPolicy` →
  `.regular` when the window opens, `.accessory` when it closes (the menu bar item stays).
  With `--background` start as `.accessory`.
- Title bar: GPUI on macOS can use a transparent title bar with the traffic lights over the
  sidebar. The GNOME `TitleBar` path in `window.rs` (client-side decorations) should not be
  used there; check which branch macOS takes.

### Windows app behaviour

- Window and taskbar icon: embed `assets/dotlink.ico` as a resource (`winresource` crate in
  `app/build.rs`), so the .exe, the taskbar and Alt-Tab all show it. Make the .ico from
  `assets/icons/*.png` (16, 24, 32, 48, 64, 256).
- Closing the window keeps running in the tray, as on Linux. The first time, say so in a
  notification (Windows users expect close = quit).

### Notifications

`notify-rust` works on both, but:

- Windows shows toasts under the app's **AppUserModelID**. An unregistered exe shows up as
  "PowerShell" or not at all. The installer's Start-menu shortcut should carry
  `System.AppUserModel.ID = dev.dotlink.Dotlink`; call `.app_id("dev.dotlink.Dotlink")`.
- macOS shows notifications only from a bundled, signed app with a bundle id; from
  `cargo run` they appear as Terminal/Script Editor. Call
  `notify_rust::set_application("dev.dotlink.Dotlink")` once at startup.
- `notify_file` should get **Open** / **Show in folder** buttons like Linux: on Windows a
  toast with actions through the `windows` crate (`Windows.UI.Notifications`), on macOS
  `UNUserNotificationCenter` with a category and two actions (needs the bundle).

### Start at login (`autostart`)

The command line is always `<exe> --background` (service + tray, no window).

- Windows: value `Dotlink` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  (`winreg` crate, or `auto-launch`). `enabled()` reads it back and compares the path.
- macOS 13+: `SMAppService.mainApp.register()` (needs the .app bundle; it appears in System
  Settings → General → Login Items). For older macOS, or `cargo run`, a LaunchAgent
  `~/Library/LaunchAgents/dev.dotlink.Dotlink.plist` with `ProgramArguments` and
  `RunAtLoad`.
- `wm_hint()` stays `None` (it's for Hyprland).

## Step 4: Remote (`Input`)

Messages are in `core/src/input.rs`; `platform/linux/input.rs` is the reference for
names. Keep the same key names (`key_by_name`): `ENTER`, `F5`, `CTRL`, `ALT`, `SHIFT`,
`SUPER`, arrows, letters…

| Message | Windows (`SendInput`, `windows` crate) | macOS (CGEvent, `core-graphics` / `objc2-core-graphics`) |
|---|---|---|
| `Move {x, y}` | `MOUSEEVENTF_MOVE` with relative dx/dy | read the cursor position, add, clamp to the displays, `kCGEventMouseMoved` (or `…Dragged` while a button is down) |
| `Scroll {x, y}` | `MOUSEEVENTF_WHEEL` / `HWHEEL`; `NOTCH` = 120 = `WHEEL_DELTA`, so pass it as is (negate y: positive is down here, up there) | `CGEventCreateScrollWheelEvent2`, pixel units for smooth scrolling |
| `Btn {b, s}` | `LEFTDOWN`/`UP`, `RIGHT…`, `MIDDLE…`; `click` = down + up | `kCGEventLeftMouseDown`/`Up` etc. at the current position; set the click count for double clicks |
| `Key {k}`, `Combo {k}` | virtual-key codes (`VK_RETURN`, `VK_LWIN` for `SUPER`); arrows and Insert/Delete/Home/End need `KEYEVENTF_EXTENDEDKEY` | `kVK_*` codes, `SUPER` = ⌘ (`kVK_Command`); set modifier flags on each event of a combo |
| `Text {s}` | `KEYEVENTF_UNICODE` per UTF-16 unit: any character, any layout | `CGEventKeyboardSetUnicodeString`: same |

Both can type any character in `Text`, unlike Linux (US layout only). `enigo` covers both if
writing it by hand isn't worth it, but check its relative-move and smooth-scroll support.

Permissions:

- Windows: `SendInput` can't reach elevated windows (UAC prompts, Task Manager) from a
  normal process. Accept it; don't run Dotlink elevated.
- macOS: needs **Accessibility**. `Input::new()` should fail with a clear message when
  `AXIsProcessTrusted()` is false, and Setup shows it (below). TCC remembers the permission
  per code signature: an unsigned build loses it on every rebuild. While developing, sign with
  a stable identity (`codesign -s "Apple Development: …" target/debug/dotlink`).

## Setup checks (`setup::checks` / `fix`)

Same `Check` struct as Linux; fixable ones get a Fix button. The page shows a section per
feature in the order the checks come, so every OS lists them as Network, Remote, Webcam,
Microphone, Cast, Clipboard, Notifications (skipping what it has nothing for).

**Windows**

| id | Feature | Check | Fix |
|---|---|---|---|
| `firewall` | Network | an inbound TCP rule for the port exists (`netsh advfirewall firewall show rule name=Dotlink`) | `netsh advfirewall firewall add rule name=Dotlink dir=in action=allow protocol=TCP localport=<port> profile=private,domain` through an elevated `ShellExecuteW(…, "runas", …)` (one UAC prompt) |
| `network_profile` | Network | the active Wi-Fi is Private (Public blocks the rule above) | not fixable: explain where to switch it |
| `input` | Remote | always passes (SendInput); notes that elevated windows are out of reach | — |
| `camera` | Webcam | the camera DLL is installed in Program Files and registered | installs it (one UAC prompt) |
| `vb_cable` | Microphone | VB-CABLE's "CABLE Input" is present (see Step 7) | opens its download page |
| `adb`, `scrcpy` | Cast | found on PATH or downloaded | downloads scrcpy with adb |
| `notifications` | Notifications | toasts aren't switched off, globally (`PushNotifications\ToastEnabled`) or for Dotlink (`Notifications\Settings\<APP_ID>\Enabled`) | not fixable: explain where to turn them on |

**macOS**

| id | Feature | Check | Fix |
|---|---|---|---|
| `accessibility` | Remote | `AXIsProcessTrusted()` | `AXIsProcessTrustedWithOptions` with the prompt, then open `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility` |
| `local_network` | Network | can't be queried; show it as information on macOS 15+ | open `…?Privacy_LocalNetwork` |
| `mic_driver` | Microphone | the virtual audio device is present | link / installer |
| `camera_extension` | Webcam | the Camera Extension is activated (`OSSystemExtensionRequest` state) | activate it; the user approves in System Settings |

## Step 5: Cast

- **Windows download**: `cast.rs::scrcpy_asset()` already has the win64 SHA-256 in a
  comment. The release is `scrcpy-win64-<version>.zip` (with `adb.exe` and its DLLs): add
  the `zip` crate, unpack into `managed_dir()`, and make `install()` choose zip or tar.gz by
  the asset name. `find_tool` already adds `.exe`.
- **Windows adb from the SDK**: `%LOCALAPPDATA%\Android\Sdk\platform-tools` (the code
  currently looks in `~/Android/Sdk`). macOS: `~/Library/Android/sdk` is already handled;
  also look in Homebrew's `/opt/homebrew/bin` and `/usr/local/bin`, which a GUI app's `PATH`
  doesn't include.
- scrcpy starts its own window; on Windows spawn it with `CREATE_NO_WINDOW` so no console
  flashes up.

## Step 6: installers and signing

`cargo packager` (already configured for Linux) also builds `nsis`, `wix` (.msi), `app`
and `dmg`. Fill in the TODOs in `app/Cargo.toml`:

**Windows**

- NSIS per-user install (no admin): `%LOCALAPPDATA%\Programs\Dotlink`, Start-menu shortcut
  with the AppUserModelID above.
- Firewall rule: per-user installs can't add one. Either leave it to Setup's Fix (one UAC
  prompt) or offer a per-machine install that adds it. Remove it on uninstall.
- Sign the .exe and the installer (Authenticode). Unsigned builds get SmartScreen's "Windows
  protected your PC".
- Icon: `assets/dotlink.ico`.

**macOS**

- `.app` with `Info.plist`: `CFBundleIdentifier` = `dev.dotlink.Dotlink`,
  `LSMinimumSystemVersion` (13.0 if SMAppService is used),
  `NSLocalNetworkUsageDescription`, `NSBonjourServices` (see Network), and
  `LSUIElement` = false (the Dock policy is switched at run time instead).
- Icon: `assets/dotlink.icns` from the PNGs (`iconutil` needs 16–1024 px @1x/@2x). The
  desktop PNGs already use a 416/512 tile with a margin, close to Apple's grid, so they can be
  used as they are.
- Universal binary (`aarch64` + `x86_64`, `lipo`), or two downloads.
- Developer ID signing with the hardened runtime, then `notarytool` and staple. Required for
  Gatekeeper, and for the Camera Extension and SMAppService.

## Step 7: webcam and microphone

The service side doesn't change: `CameraHub::send` receives MJPEG frames and `MicSink::write`
16-bit PCM chunks, at whatever rate the phone sends. Keep the Linux rules on both:
**only the newest frame matters** (a `watch` channel, never a queue), a few mic chunks of
backlog at most, a placeholder picture while the phone camera is off, and a fixed output size
while apps are reading.

### Windows webcam

Done: `MFCreateVirtualCamera` (Windows 11) registers a camera every app sees (Teams, Zoom,
browsers, OBS). Its **media source COM DLL** is the `desktop/camera` crate, installed and
registered by Setup; Dotlink decodes the JPEG to NV12 (`zune-jpeg`) and passes it through a
shared file. Windows 10 has no virtual camera API and isn't supported.

### Windows microphone

A real virtual audio device is a kernel driver, which needs an EV certificate and Microsoft's
attestation signing: too much to start with. Instead:

1. Setup asks the user to install **VB-CABLE** (free for users; redistributing it needs a
   licence from VB-Audio).
2. `MicSink` plays the PCM into "CABLE Input" through WASAPI (`cpal`), with a low buffer.
3. Apps pick "CABLE Output" as their microphone; the Devices page says so.

### macOS webcam

A **CoreMediaIO Camera Extension** (macOS 12.3+): a system extension inside the .app, in
Swift, built with Xcode next to the Rust binary. It publishes "Dotlink Camera" with a
source stream and a **sink stream**; the app (through a small Swift/ObjC bridge, or
`objc2` bindings to CoreMediaIO) pushes `CVPixelBuffer`s into the sink. Requirements: the app
in `/Applications`, Developer ID signing, the
`com.apple.developer.system-extension.install` entitlement, and the user's approval in System
Settings the first time. Older approaches (DAL plug-ins, like OBS used to have) no longer
load on macOS 14+.

### macOS microphone

Start with **BlackHole** (GPL-3; ask the user to install it, Homebrew:
`brew install blackhole-2ch`), and have `MicSink` play into "BlackHole 2ch" with `cpal`
(CoreAudio). Later, an own AudioServerPlugIn (user-space, no kext) named "Dotlink Mic",
installed by a .pkg into `/Library/Audio/Plug-Ins/HAL/` and fed over shared memory, removes
the dependency.

## Testing

- **A Windows or macOS PC and a phone on the same Wi-Fi** are needed from Step 2 on. Pair,
  send a file each way, copy text each way, open two Shell tabs, reconnect after toggling
  the PC's Wi-Fi (scrollback should come back), restart the PC's service and check the phone
  finds it again over mDNS.
- **Without a phone**: the Node script approach from
  [CONTRIBUTION.md](CONTRIBUTION.md#testing-without-disturbing-your-own-setup) works on any
  OS; set the config dir through `%APPDATA%` / `~/Library/Application Support` instead of
  `XDG_CONFIG_HOME`.
- **A second instance next to yours**: on Linux that's `XDG_CONFIG_HOME` + `XDG_RUNTIME_DIR`.
  On Windows and macOS the config dir comes from `dirs` and ignores those variables. Add a
  `DOTLINK_CONFIG_DIR` override in `config::config_dir()` (and a pipe/socket name derived
  from it) before relying on side-by-side testing.
- **Remote**: Notepad / TextEdit for `Text` (try `é`, `ß`, emoji), a browser for scrolling,
  a game or a drawing app for drags, ⌘Tab / Alt-Tab for combos.
- **Webcam / mic**: the OS's Camera app / Photo Booth, a browser call test page, OBS.

## Checklist

| | Windows | macOS |
|---|---|---|
| CI builds and tests on this OS | workflow written, not run yet | |
| `cargo build -p dotlink` and `--no-default-features` without warnings | ✅ | |
| Control socket private to the user (Windows ACL) | ✅ | |
| Pair, files both ways, clipboard both ways, Shell (incl. tab titles) | ✅ | |
| Graceful quit (tray, Settings, `dotlink quit`, OS shutdown) | ✅ | |
| Tray / menu bar with every `TrayAction` | ✅ | |
| App icon in the window, taskbar / Dock, notifications | ✅ | |
| Notifications under Dotlink's name, Open / Show in folder on received files | ✅ (buttons not clicked in a test) | |
| Start at login (`--background`) | ✅ | |
| Setup checks and fixes for this OS | ✅ | |
| Remote: move, click, drag, scroll, keys, combos, text | ✅ | |
| Cast: download scrcpy, USB and Wi-Fi | ✅ | |
| Link over a USB cable | ✅ | |
| Installer, signed (and notarized on macOS) | ✅ installer, not signed | |
| Webcam | ✅ (Windows 11) | |
| Microphone | ✅ (VB-CABLE) | |
| README platform table and ARCHITECTURE updated, `features()` flags on | ✅ | |
