# macOS implementation plan

The plan for taking the desktop app from "stubs written on Linux" to a shipped, signed macOS
app. It turns [PORTING.md](PORTING.md) (the APIs to use) and the macOS column of
[PLATFORM_FEATURES.md](PLATFORM_FEATURES.md) (what's missing) into work items: which files
change, what "done" means for each, and in which order. Linux stays the reference; Windows 11
is the example of a finished port.

Target: **macOS 13 Ventura or newer, Apple Silicon first** (x86_64 as a second build, then a
universal binary). The Android app needs no changes: it already reads `os: "macos"` from
`hello` and labels the Remote's keys ⌘ / ⌥.

## Where macOS stands

From reading the code (2026-10-27):

| Piece | State | Where |
|---|---|---|
| Build | builds, tests and clippy clean; the window opens (Phase 1) | — |
| Service, pairing, files, mDNS, TLS, USB link | portable, untested | `core/src/server.rs`, `service.rs`, `usb.rs` |
| Control socket | Unix socket in `runtime_dir()` = `$XDG_RUNTIME_DIR` or `std::env::temp_dir()` | `core/src/ipc.rs`, `config.rs:226` |
| Clipboard | `polling_clipboard` (arboard, 500 ms) | `platform/macos/mod.rs` |
| Shell | login shell in a PTY; tab titles return `None` | `platform/macos/mod.rs`, `term.rs:94` |
| Notifications | `notify-rust`, text only, no bundle id set | `platform/macos/mod.rs:36` |
| Open / Show in folder | `open`, `open -R` | `platform/macos/mod.rs` |
| Battery | `pmset -g batt` | `platform/macos/mod.rs`, `platform/battery.rs` |
| Cast | scrcpy macOS assets and SHA-256s pinned; SDK path `~/Library/Android/sdk` | `core/src/cast.rs:526`, `:605` |
| Remote | `Input::new()` bails | stub |
| Tray / menu bar | `tray::start` returns `false` | `app/src/gui/tray.rs:245` |
| App menu, Dock | nothing | `app/src/gui/mod.rs` (TODO at the top) |
| Start at login | bails | stub |
| Setup | no checks | stub |
| Webcam, mic | bail | stub |
| Share menu | no-op | stub |
| Updates | `own_asset` is `false` off Windows/Linux: never offers an install | `core/src/update.rs:185` |
| Packaging, CI | nothing for macOS | `app/Cargo.toml`, `.github/workflows/release.yml` |

Gaps found while reading that the other docs don't list yet (1–3 fixed on `feat/macos-port`):

1. **Control socket path under launchd.** `std::env::temp_dir()` reads `$TMPDIR`. Terminal and
   apps launched by Finder get the per-user `/var/folders/…/T/`, but a LaunchAgent (start at
   login, Phase 3) doesn't always set `TMPDIR`, and then it is `/tmp`: shared by every user,
   and a different path from the one the CLI in Terminal looks at, so `dotlink send` would
   report "not running". Use `confstr(_CS_DARWIN_USER_TEMP_DIR)` (via `libc`) on macOS, which
   is what `TMPDIR` normally holds, independent of the environment.
2. **`PATH` of a GUI app** is `/usr/bin:/bin:/usr/sbin:/sbin`. `find_tool` doesn't look in
   `/opt/homebrew/bin` or `/usr/local/bin`, so Homebrew's `adb` and `scrcpy` aren't found from
   the .app (they are from `cargo run` in Terminal, which hides the bug).
3. **Setup intro text**: `window.rs:322` says "polkit" on every non-Windows OS.
4. **"Still running" notice**: `gui/mod.rs:134` only tells Windows users that closing the window
   didn't quit. macOS users expect the opposite (closing a window keeps the app), so nothing to
   say there, but the Dock icon has to go (Phase 3).
5. **Updates**: macOS got no in-app install; now it swaps in the .dmg's app (Phase 6).
6. **PC name**: on a Mac on DHCP the hostname can be its IP address (`192.168.1.3`), which the
   window, the phone and mDNS showed as the PC's name. `platform::computer_name()` now gives
   `scutil --get ComputerName` ("MacBook Air") on macOS; fixed.
7. **"Where is use_default?"**: with no bundle id set, notify-rust asks AppleScript for the id
   of an app named "use_default", and macOS shows an app picker on the first notification
   (seen on the Pair page). `notify()` now sets `dev.dotlink.Dotlink` and, outside an `.app`,
   only logs the notification; fixed (later replaced, see Phase 3's notifications).
8. **`cargo packager` hangs** at `cargo metadata`, which downloads every platform's crates:
   the Windows-only `windows` crates are ~9 MB each, and slow from some networks. Run
   `cargo fetch` once (it resumes); after that packaging takes as long as the release build.

## Phases

Each phase leaves a usable app and ends with docs updated (PLATFORM_FEATURES' macOS column,
PORTING's checklist) and `features()` flags flipped in the same commit as the feature.

### Phase 1: it builds (CI included)

Goal: `cargo build -p dotlink`, `--no-default-features`, `cargo test`, `cargo clippy
--all-targets -- -D warnings` clean on an Apple Silicon Mac.

First `cargo check -p dotlink --all-targets` on macOS 15.8, Apple Silicon, Rust 1.99
(2026-10-27): **it compiles**, with one warning:

```
app/src/gui/tray.rs:20:5: warning: variants `Open`, `Pair`, `SendFiles`, `SendClipboard`,
`Cast`, and `Quit` are never constructed
```

(`TrayAction` has no sender on macOS until the menu bar item exists.) Also a future-incompat
note for `block v0.1.6`, pulled in through gpui's dependencies: nothing to do here.

- [x] The `TrayAction` warning: gone with the menu bar item (done early, Phase 3).
- [x] `cargo build -p dotlink`, `--no-default-features`, `cargo test`, `cargo clippy
      --all-targets -- -D warnings` clean; the window opens. Newer clippy also flagged a tab in
      a doc comment (`battery.rs`), `Ok(…?)` (`peer.rs`), a needless `return` (`update.rs`)
      and the camera's unused offsets off Windows (`shared.rs`): fixed.
- [x] `.github/workflows/desktop.yml`: matrix `ubuntu-latest`, `windows-latest`,
      `macos-latest`, on PRs and pushes to `main`, running the four commands above in
      `desktop/`. This keeps the macOS code compiling while the other OSes move on.
- [ ] Rustfmt and clippy on the files touched.

Done when: the workflow is green on all three OSes.

### Phase 2: core works with a phone

Goal: pair, files both ways, clipboard both ways, Shell, USB link, Cast, from `cargo run`.

- [x] **Socket path** (gap 1): `config::runtime_dir()` on macOS returns
      `confstr(_CS_DARWIN_USER_TEMP_DIR)`, falling back to `temp_dir()`. Check the socket is
      mode 0700's child and that a second user gets their own.
- [x] **Tool lookup** (gap 2): `cast::find_tool` also tries `/opt/homebrew/bin` and
      `/usr/local/bin` on macOS.
- [x] **Shell tab titles**: `process_name(pid)` with `proc_name` (`libproc` crate, or
      `libc::proc_name` directly). `term.rs` already passes the foreground process group.
- [x] **`parent_process()`**: `proc_pidinfo` / `ppid` → `proc_name`, for the log.
- [ ] **Clipboard**: keep arboard, but compare `NSPasteboard.changeCount` first and only read
      the text when it changed (cheaper than reading every 500 ms), and skip items carrying
      `org.nspasteboard.ConcealedType` or `TransientType` (password managers), as Linux does.
      Needs `objc2-app-kit` (`NSPasteboard`).
- [ ] **Network permissions**: from Terminal the app inherits Terminal's Local Network and
      firewall answers; note it in CONTRIBUTION.md so testers aren't surprised when the
      bundled app asks again.
- [ ] Test with a phone: pair (QR and over USB), a file each way (SHA-256), clipboard each
      way, two Shell tabs (`vim`, resize, reconnect keeps scrollback), Wi-Fi ↔ USB switch,
      the PC's IP changing (mDNS next to mDNSResponder), Cast over USB and Wi-Fi with the
      downloaded scrcpy and with Homebrew's.

Done when: those tests pass; PLATFORM_FEATURES' Core, Control socket, Clipboard, Shell, Cast
and USB rows for macOS go from 🔧 to ✅.

### Phase 3: feels native

Goal: menu bar item, app menu, Dock behaviour, start at login, notifications with buttons.

- [x] **Menu bar item** (`app/src/gui/tray.rs`): add `tray-icon` to the macOS dependencies
      (`cfg(any(windows, target_os = "macos"))`), reuse the Windows code path for the menu
      (`muda` items → `TrayAction`). The icon is `glyph()` in black as a **template image**
      (`with_icon_as_template(true)`, 44 px for @2x). No left-click-opens-window on macOS: a
      click shows the menu, with Open Dotlink first. Created on the main thread inside
      `Application::run`, like Windows.
- [x] **App menu** (`gui/mod.rs`): `cx.set_menus` with About Dotlink, Settings… (⌘,),
      Hide Dotlink (⌘H), Hide Others, Quit Dotlink (⌘Q); Edit menu with Cut/Copy/Paste/Select
      All, or text fields lose ⌘C/⌘V. Quit stops the service like the tray's Quit.
- [x] **Dock** (`gui/mod.rs`): `NSApplication.setActivationPolicy` (via `objc2-app-kit`)
      `.regular` when the window opens, `.accessory` when it closes; start as `.accessory`
      with `--background`. `cx.on_reopen` shows the window when the Dock icon is clicked.
- [ ] **Window chrome**: confirm macOS takes the server-side path (never `title_bar.rs`);
      optionally a transparent title bar with the traffic lights over the sidebar.
- [x] **Setup intro** (gap 3): its own macOS text ("Fix opens System Settings…").
- [x] **Start at login** (`platform/macos/autostart.rs`): a LaunchAgent
      `~/Library/LaunchAgents/dev.dotlink.Dotlink.plist` with `ProgramArguments = [<exe>,
      --background]`, `RunAtLoad`, `ProcessType = Interactive`. Not SMAppService.mainApp: it
      takes no arguments, so Dotlink would open its window at every login. Off under Login
      Items → Allow in the Background (`launchctl print-disabled`) counts as off. Turning it
      off only deletes the file (`launchctl bootout` would stop the running app). Not yet
      tested across a real logout.
- [x] **Notifications**: the service hands them to the app (`platform::Notify`, a channel set
      by `gui/mac.rs`), which posts them with GPUI's `show_system_notification`
      (UNUserNotificationCenter): Accept / Decline on incoming files, Open / Show in Finder on
      received ones; clicking the notification opens the window. notify-rust is Windows-only
      now. Lessons: notifications are refused ("Notifications are not allowed for this
      application", no prompt) while the .app runs from `/tmp`; from `~/Applications` macOS
      asks. `gpui_component::init` registers its own response handler and replaces any
      registered before it, so ours comes after. GPUI's warnings only show with
      `RUST_LOG=gpui_macos=debug`: the default filter keeps `dotlink*` only.
- [x] Notification permission: asked by GPUI on the first notification.

Done when: Tray, Start at login and Notifications rows for macOS are ✅, tested from a
bundled app (Phase 6's `.app`, unsigned is fine for this).

### Phase 4: Remote

Goal: the phone as touchpad and keyboard.

- [x] `platform/macos/input.rs` (new), CoreGraphics through plain FFI (no new crates):
  - `Move`: current position (`CGEventGetLocation` of a null event) + delta, clamped to the
    union of `CGGetActiveDisplayList` bounds; `MouseMoved`, or `LeftMouseDragged` /
    `RightMouseDragged` while a button is held.
  - `Scroll`: `CGEventCreateScrollWheelEvent2` in pixel units (smooth), y negated as on
    Windows; horizontal as wheel 2.
  - `Btn`: down/up at the cursor; track click timing to set
    `kCGMouseEventClickState` (2 for double clicks, or Finder won't open things).
  - `Key` / `Combo`: map `key_by_name` names to `kVK_*` (`SUPER` = Command, `ALT` = Option);
    set `CGEventFlags` on every event of a combo, release in reverse order.
  - `Text`: `CGEventKeyboardSetUnicodeString` in chunks of ≤20 UTF-16 units: any character,
    any layout.
  - Post at `kCGHIDEventTap`.
- [x] `Input::new()` fails with "Allow Dotlink in System Settings → Privacy & Security →
      Accessibility" while `AXIsProcessTrusted()` is false.
- [x] Setup check `accessibility` (Remote): `AXIsProcessTrusted()`; Fix calls
      `AXIsProcessTrustedWithOptions` with the prompt, then opens
      `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`.
- [x] `features().remote = true`.
- [x] Media keys (volume, play/pause, next/previous) as NSEvent system-defined events;
      Print Screen as ⌘⇧3. Named letter keys in combos use US key positions; `Text` is
      layout-free.
- [ ] Dev note in CONTRIBUTION.md: sign debug builds with a stable identity
      (`codesign -s "Apple Development: …" target/debug/dotlink`) or TCC forgets the grant on
      every rebuild.
- [ ] Test: TextEdit (`é ß 😀 日本`), Safari scrolling, drag in Finder, ⌘Tab, ⌘Space, double
      click to open.

### Phase 5: Setup page

Started: `accessibility`, `adb` and `scrcpy` are in.

`platform/macos/setup.rs` (new), sections in the usual order:

| id | Feature | Check | Fix |
|---|---|---|---|
| `local_network` | Network | can't be queried: information on macOS 15+ | open `…?Privacy_LocalNetwork` |
| `firewall` | Network | application firewall on (`socketfilterfw --getglobalstate`) and Dotlink not blocked (`--getappblocked <exe>`) | not fixable without admin: explain; or `osascript … with administrator privileges` |
| `accessibility` | Remote | Phase 4 | Phase 4 |
| `camera_extension` | Webcam | Phase 7 | Phase 7 |
| `mic_driver` | Microphone | Phase 7 | Phase 7 |
| `adb`, `scrcpy` | Cast | `find_tool` | download (already portable) |
| `notifications` | Notifications | `UNUserNotificationCenter` authorization status | request, else open `…?Notifications` |

Done when: Setup checks row for macOS is ✅.

### Phase 6: packaging, signing, updates

- [x] **Icon**: `assets/dotlink.icns`, drawn by `scripts/make-macos-icon.py` (the Windows
      script's drawing on Apple's 824/1024 grid, through `iconutil`).
- [~] **`app/Cargo.toml`** `[package.metadata.packager.macos]` (minimum version and
      `assets/macos/Info.plist` done; signing and entitlements to do): `minimum-system-version =
      "13.0"`, `signing-identity`, `entitlements`, `info-plist-path` with:
      `CFBundleIdentifier = dev.dotlink.Dotlink`, `LSUIElement = false`,
      `NSLocalNetworkUsageDescription`, `NSBonjourServices = [_dotlink._tcp,
      _adb-tls-connect._tcp, _adb-tls-pairing._tcp]`, `CFBundleURLTypes` for `dotlink:`
      (the pairing / notification links), `NSCameraUsageDescription` not needed (we don't read
      the Mac's camera).
- [ ] **Entitlements**: hardened runtime; `com.apple.security.cs.allow-jit` only if the
      runtime-compiled Metal shaders need it (check); `system-extension.install` once Phase 7
      lands.
- [ ] **Signing and notarization** in CI. No Developer ID for now: the .dmg's app is signed ad
      hoc instead. Later: the Developer ID Application certificate as a secret
      (`MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`), `notarytool` with an App Store Connect
      API key (`MACOS_NOTARY_KEY`, `_KEY_ID`, `_ISSUER`), then `stapler staple`. The scrcpy
      binaries Dotlink downloads run outside the bundle, so they don't need re-signing.
- [x] **`release.yml`**: a `macos` job on `macos-14` building `aarch64-apple-darwin` and
      `x86_64-apple-darwin`, `lipo` into one universal binary, `cargo packager -f dmg`, sign,
      notarize; add `Dotlink-<version>.dmg` to the Downloads table ("macOS 13 or newer").
      Unsigned manual runs (no secrets) still produce an artifact, as with the Android job.
- [x] **Updates** (`core/src/update.rs`): `own_asset` matches `.dmg` on macOS; `download` puts
      it in `temp_dir`, mounts it (`hdiutil attach -nobrowse`) and `ditto`s its app next to the
      running one (`.Dotlink.app.update`), so errors show before quitting; `launch` waits for
      this process to exit, swaps the two and `open`s it. The release page instead when the
      app's folder isn't writable (`cargo run`, the mounted .dmg, /Applications of an admin).
- [ ] Test: drag to Applications on a clean user, Gatekeeper opens it without a warning, login
      item appears in System Settings → General → Login Items, update from the previous beta.

Done when: Installer row for macOS is ✅ signed and notarized.

### Phase 7: webcam and microphone

The service side stays as it is (`CameraHub::send` gets JPEG frames, `MicSink::write` PCM);
same rules as Linux: newest frame only, short mic backlog, placeholder picture while the phone
camera is off, fixed size while apps read.

**Microphone first (smaller):**

- [x] `platform/macos/media.rs`: `MicSink` plays into "BlackHole 2ch" with `cpal` (CoreAudio),
      converting rate and channels like the Windows sink. Share that conversion code with
      `windows/media.rs` instead of copying it (`platform/cpal_sink.rs`).
- [x] Setup check `blackhole` (was `mic_driver`): Fix downloads and installs the pkg itself (an
      app can't run `brew install`: the cask's pkg asks for the password in a terminal). a CoreAudio output device named "BlackHole 2ch"; Fix opens
      its page (or `brew install blackhole-2ch` when Homebrew is there). Apps pick
      "BlackHole 2ch" as their microphone; the Phones page says so.
- [x] `features().mic = true`.
- [ ] Later: an own AudioServerPlugIn "Dotlink Mic" (user space, no kext), installed into
      `/Library/Audio/Plug-Ins/HAL/` with one admin prompt, fed over shared memory. Removes
      the BlackHole dependency and its GPL-3 question for redistribution.

**Webcam, for now: through OBS** (no Developer ID). `platform/macos/camera.rs` serves the
phone's frames as multipart JPEG on `127.0.0.1:<port + 1000>/camera/<token>.mjpg` (the token in
`camera-token`, 0600), newest frame only, the placeholder (`core/assets/camera-off.jpg`, drawn
by `scripts/make-camera-placeholder.swift`) at 5 fps while the phone is off. OBS reads it as a
Media Source (Input format `mpjpeg`) and OBS Virtual Camera hands it to apps. Phones → Webcam
has **Copy OBS link**; Setup checks that OBS is installed. Steps for users: README, macOS.

- [x] The stream, the placeholder, the Setup check, Copy OBS link, `features().camera = true`.
- [ ] Tested with the phone in OBS and a call app.

**Webcam, later: a Camera Extension of our own** (needs the Apple Developer Program):

- [ ] `desktop/macos-camera/`: an Xcode project for a **CoreMediaIO Camera Extension** in
      Swift: a device "Dotlink Camera" with a source stream (what apps read) and a sink stream
      (what Dotlink writes), 1920×1080 at 30 and 60 fps, NV12 or BGRA, a placeholder frame
      when the sink is idle. Built by `xcodebuild` in `before-packaging-command` and copied to
      `Dotlink.app/Contents/Library/SystemExtensions/`.
- [ ] Activation: `OSSystemExtensionRequest.activationRequest` from the app (Setup → Fix);
      the user approves in System Settings once. Needs the app in `/Applications`, Developer
      ID signing, and the `com.apple.developer.system-extension.install` entitlement (plus the
      extension's own provisioning profile with the CMIO entitlement).
- [ ] `platform/macos/camera.rs`: find the device with the CoreMediaIO C API
      (`CMIOObjectGetPropertyData`), open the sink stream's `CMSimpleQueue`
      (`CMIOStreamCopyBufferQueue`), decode JPEG with `zune-jpeg` (already a Windows
      dependency), fill an IOSurface-backed `CVPixelBuffer`, wrap in `CMSampleBuffer`, enqueue.
      A small ObjC/Swift shim is fine if the bindings get heavy.
- [ ] Setup check `camera_extension`: activated and matching this build's version (like the
      Windows camera's `SOURCE` hash), Fix activates or replaces it.
- [ ] `features().camera = true`.
- [ ] Test: Photo Booth, FaceTime, a browser test page, OBS; 30 and 60 fps; phone camera off
      shows the placeholder.

### Later / optional

- **Share menu**: a Finder Share extension or Quick Action that runs `dotlink send`
  (`share_menu::set`). Low value next to drag and drop onto the window and the menu bar's
  Send files….
- **Universal Clipboard interplay**: the Mac's own Handoff clipboard can bounce text back;
  the echo check should cover it, verify.

## Order and size

| Phase | Ships | Rough size |
|---|---|---|
| 1 Build + CI | nothing visible | small |
| 2 Core | files, clipboard, Shell, Cast, USB from `cargo run` | small |
| 3 Native | menu bar, Dock, login, notifications | medium |
| 4 Remote | touchpad and keyboard | medium |
| 5 Setup | Setup page | small |
| 6 Packaging | a signed .dmg, updates | medium (needs an Apple Developer account) |
| 7 Mic, webcam | BlackHole mic; Camera Extension | mic small, camera large |

After Phase 6 macOS can be listed as supported in the README (without webcam and mic, like
the Windows mic was before VB-CABLE). Needs before Phase 6: an Apple Developer Program
membership (Developer ID certificate, notarization); before Phase 7's camera, the CMIO
system-extension entitlement on that account.

## Testing setup on a Mac

- `cargo run -p dotlink` from Terminal for Phases 1–2. A second instance:
  `DOTLINK_CONFIG_DIR=/tmp/dl-test port 47391` as in CONTRIBUTION.md.
- From Phase 3 on, test from the bundled `.app` too: permissions (Local Network, Accessibility,
  notifications, login items) behave differently from Terminal.
- `tccutil reset Accessibility dev.dotlink.Dotlink` and `tccutil reset All dev.dotlink.Dotlink`
  start the permission flows over.
- Phone side: same as on Linux (`adb shell input …`, `adb exec-out screencap -p`).

## Docs to update as phases land

- PLATFORM_FEATURES.md: the macOS column and its "Still to do" list.
- PORTING.md: the macOS column of the checklist.
- README.md: requirements table, install section (.dmg), "macOS is planned" line.
- ARCHITECTURE.md: macOS paths in "Files on disk", the menu bar in "Tray", threads (the
  camera's queue, the CoreAudio stream).
- CONTRIBUTION.md: building on macOS (Xcode or Command Line Tools, signing a debug build),
  testing notes.
