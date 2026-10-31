# Contributing

How the code is built, tested and written. [ARCHITECTURE.md](ARCHITECTURE.md) explains how the
pieces fit together.

## Building

### Desktop (Rust)

Needs a recent stable Rust (edition 2024) and, for the window, the GPUI build libraries:

```sh
# Fedora
sudo dnf install libxkbcommon-devel libxkbcommon-x11-devel wayland-devel libxcb-devel fontconfig-devel freetype-devel
# Debian / Ubuntu
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libxcb1-dev libfontconfig-dev libfreetype-dev
```

```sh
cd desktop
cargo run -p dotlink                          # the app (window + tray)
cargo run -p dotlink -- daemon                # the service alone
cargo build -p dotlink --no-default-features  # without GPUI: service + CLI only
cargo test                                    # unit tests
```

`scripts/install.sh` builds a release and installs it for the current user (binary, app menu
entry, icon, autostart).

On Windows: Rust with the MSVC toolchain, nothing else. `cargo build` builds the workspace,
including `dotlink_camera.dll` (a development build finds it in `target/debug/deps/`). Setup's
webcam Fix installs that DLL into Program Files; after changing the `camera` crate, run Fix again
(Setup shows the camera as outdated). Fix stops the Camera Frame Server so the new DLL is the one
loaded; an app using the camera loses it for a moment.

### Android

```sh
cd android
./gradlew installDebug       # build and install on the phone over adb
./gradlew assembleRelease    # app/build/outputs/apk/release/
```

Release builds are signed with the key named in `android/keystore.properties` (`storeFile`,
`storePassword`, `keyAlias`, `keyPassword`; never in git), and with the debug key without it.
APKs signed with different keys can't update each other: switching means uninstalling once,
which forgets the pairing.

Some phones (ColorOS / realme, MIUI) refuse adb installs until you confirm a dialog on the
phone, or turn off *Verify apps over USB* in Developer options.

### Packages

```sh
cargo install cargo-packager cargo-generate-rpm
cd desktop/app
NO_STRIP=1 cargo packager --release -f deb,appimage   # Linux: target/packages/
cargo packager --release -f nsis                      # Windows: target/packages/*-setup.exe
cargo packager --release -f app                       # macOS: target/packages/Dotlink.app
cd .. && cargo build --release -p dotlink && cargo generate-rpm -p app   # target/generate-rpm/
```

The Windows icon (`app/assets/dotlink.ico`) is drawn by `scripts/make-windows-icon.py`, the
macOS one (`app/assets/dotlink.icns`) by `scripts/make-macos-icon.py`.
`.github/workflows/release.yml` builds every package (on macOS a universal .app, signed ad hoc,
in a .dmg); run it by hand to get them as artifacts. The first `cargo packager` on a machine
runs `cargo metadata`, which downloads every platform's crates, some large: run `cargo fetch`
first on a slow connection (it resumes).

### Releasing

One version for everything: `version` in `desktop/Cargo.toml`. The Android app reads it too and
derives its `versionCode` from it (`1.2.3-beta.4` is `1020304`, `1.2.3` is `1020399`), so a
pre-release's suffix ends in a number below 100 and grows: `beta.1`, `beta.2`, `rc.3`.

1. Bump `version` in `desktop/Cargo.toml` and run `cargo check` (updates `Cargo.lock`).
2. Optionally write the notes in `.github/release-notes/v<version>.md`; otherwise GitHub
   generates them from the commits.
3. Commit, tag `v<version>` and push the tag. A tag with a suffix (`v0.2.0-beta.1`) publishes a
   pre-release; without one, a release.

The workflow signs the APK with the release key from the repository secrets
(`ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`,
`ANDROID_KEY_PASSWORD`) and refuses to publish without them.

The apps check the releases for updates (`desktop/core/src/update.rs`,
`android/.../update/Updater.kt`): on start, every 12 hours on the PC, and from Settings. Windows
runs the new setup `.exe` and an AppImage replaces itself; `.deb` and `.rpm` installs get the
release page. Android downloads the APK and opens the system installer, which only accepts it
over an app signed with the same key. Installs of a pre-release are offered pre-releases too;
stable installs only stable releases. Everything from the `## Downloads` heading of the release
notes is hidden in the apps.

`NO_STRIP=1` works around linuxdeploy's `strip` being too old for current distros' libraries.
Packages need the glibc they were built with or newer: build releases on the oldest distro you
support.

## Testing without disturbing your own setup

The service keeps its config in `$XDG_CONFIG_HOME/dotlink` and its socket in
`$XDG_RUNTIME_DIR`, so a second instance can run next to yours. `DOTLINK_CONFIG_DIR` points the
config folder somewhere else directly (on every OS); the control socket and the mDNS name then
get a suffix of their own, and the second instance leaves the "Share with Dotlink" menu and the
`dotlink:` links of notification buttons alone (so a second instance's Open / Accept buttons
reach the usual instance, not it):

```sh
mkdir -p /tmp/dl-test/cfg/dotlink /tmp/dl-test/run && chmod 700 /tmp/dl-test/run
printf 'port = 47391\ndownload_dir = "/tmp/dl-test/downloads"\n' > /tmp/dl-test/cfg/dotlink/config.toml
export XDG_CONFIG_HOME=/tmp/dl-test/cfg XDG_RUNTIME_DIR=/tmp/dl-test/run
# keep the window on your compositor:
export WAYLAND_DISPLAY=/run/user/$(id -u)/wayland-1
cargo run -p dotlink
```

- **A phone without a phone.** The protocol is plain HTTPS + WebSocket, so a short Node script
  can pair (`POST /pair` with the secret from `dotlink pair`), open `/ws/control` and upload or
  download files. Node's built-in fetch is HTTP/1.1, which is what the server speaks.
- **GNOME without logging into GNOME.** `dbus-run-session -- gnome-shell --headless --wayland
  --virtual-monitor 1400x900 --wayland-display NAME --unsafe-mode` runs a full shell offscreen;
  `org.gnome.Shell.Screenshot` takes screenshots and `org.gnome.Shell.Eval` drives it. Give
  clients `DISPLAY` and `XAUTHORITY` (`$XDG_RUNTIME_DIR/.mutter-Xwaylandauth.*`) for XWayland,
  and point `XDG_DATA_HOME`/`XDG_CACHE_HOME` at a temp dir so your extensions aren't touched.
- **Hyprland.** `hyprctl output create headless NAME` adds an invisible monitor; move the test
  window there and capture it with `grim -o NAME`. Remove it afterwards
  (`hyprctl output remove NAME`).
- **Windows.** The config folder comes from `%APPDATA%` and ignores `XDG_*`; use
  `DOTLINK_CONFIG_DIR` for a second instance (PowerShell: `$env:DOTLINK_CONFIG_DIR =
  "$env:TEMP\dl-test"`, put `port = 47391` in its `config.toml`, then `cargo run -p dotlink`;
  `dotlink` commands from the same shell reach it). `cargo test -p dotlink-camera --test capture -- --ignored
  --nocapture` opens "Dotlink Camera" through the frame server like an app would.
  From Linux, `rustup target add x86_64-pc-windows-msvc` then `cargo check -p dotlink-camera
  --target x86_64-pc-windows-msvc` type-checks the camera DLL; the whole app needs MSVC's C
  compiler (ring, aws-lc), so build it on Windows. Some gpui APIs only exist on one OS (for
  example `WindowButtonLayout::linux_default`), so after changing the GUI on Linux, build on
  Windows too.
- **Driving the phone from the PC.** With USB debugging, `adb shell input tap/swipe/text`
  and `adb exec-out screencap -p` test the app's side; the Remote and Shell keyboards accept
  `input text`. Use a new Shell tab for tests: the first one may be a session in use.

## Code style

- **Rust:** formatted with `rustfmt --edition 2024 --config max_width=120,use_small_heuristics=Max`
  (there is no rustfmt.toml yet; format the files you touch, not the whole tree). No warnings.
- **Kotlin:** ktlint style (trailing commas, one argument per line when wrapped).
- **Comments** say *why*, in plain sentences: the constraint, the platform quirk, what would
  break. Doc comments on public items say what they're for. No commented-out code.
- **User-facing text** is short, concrete and sentence case: "Turn on Wireless debugging",
  not "Error: ADB WiFi disabled". Errors say what to do.
- **The Dotline look** (both apps): black or paper white, one red accent, Doto for display
  type, Geist for text, Geist Mono for labels and data. Android uses pill buttons and 16 px cards;
  the desktop uses 8 px corners on controls and 12 px on cards (`RADIUS`, `CARD_RADIUS` in
  `gui/window.rs`), with dots and badges round. Colors live
  in `android/…/ui/theme/Color.kt` and `desktop/app/src/gui/theme.rs`; keep them in sync.
  Icons are Lucide (Android: `DotlineIcons.kt`; desktop: `gui/assets.rs`).

## Where things go

- **New OS-specific behaviour:** add it to every module in `core/src/platform/` (a stub with
  a TODO is fine for the ones you can't test) and list it in `platform/mod.rs`.
- **New phone ↔ PC message:** handle it in `server.rs` (`control_session`) and in Android's
  `Link.onControl`; add it to the table in ARCHITECTURE.md. Older apps ignore unknown types,
  so new messages must be optional.
- **New thing the window shows:** expose it through `App::status()` and, if it changes,
  an `events::Event`; the window refreshes on events.
- **New system requirement:** a check in `platform/<os>/setup.rs` (macOS: the `setup` module in
  `platform/macos/mod.rs`), fixable when it can be installed or set up with one root command
  (Linux), one UAC prompt (Windows) or one administrator prompt (macOS).

## Windows and macOS

`core/src/platform/windows` is complete for Windows 11 and `core/src/platform/macos` for macOS
13 and newer, both tested with a phone; [PLATFORM_FEATURES.md](PLATFORM_FEATURES.md) has their
status and what's left.

On macOS, `cargo run -p dotlink` needs only the Xcode Command Line Tools. Some things only work
from an app bundle: notifications, the Local Network prompt (from Terminal the app gets
Terminal's), and the permissions in System Settings, which macOS grants per app. To test them,
build the bundle (`cd app && cargo packager --release -f app`), put it in `~/Applications`
(from `/tmp` notifications are refused without asking) and sign it with a stable identity,
`codesign --force --deep -s "Apple Development: …" ~/Applications/Dotlink.app`: an unsigned
build loses Accessibility on every rebuild. From `cargo run`, Accessibility goes to the
terminal app. `RUST_LOG=info,gpui_macos=debug` shows GPUI's own warnings (refused
notifications, for one), which the default log filter hides.

## Commits

Every commit follows [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/),
one topic per commit:

```
<type>(<scope>): <what changed>

<why, when it isn't obvious>
```

- **Type:** `feat` (new behaviour), `fix` (a bug), `docs`, `refactor` (same behaviour, other
  code), `perf`, `test`, `build` (Cargo, Gradle, packaging, installers), `ci`, `chore`
  (anything else: version bumps, assets).
- **Scope:** the area it touches, lower case: `cast`, `webcam`, `mic`, `remote`, `shell`,
  `files`, `clipboard`, `pairing`, `usb`, `tray`, `setup`, `devices`, `android`, `windows`,
  `linux`, `macos`, `docs`. Leave it out when a change spans many areas.
- **Subject:** imperative, lower case, no full stop, about 70 characters at most
  (`fix(cast): download scrcpy when it's missing`).
- **Body:** wrapped at 72 columns, says why; the diff already says what.
- **Breaking changes** (the phone and PC no longer understand each other, a config key is
  renamed): `!` after the scope and a `BREAKING CHANGE:` footer,
  e.g. `feat(pairing)!: pin the certificate per phone`.

Pull request titles use the same form. Commits before
this rule use `Area: what changed` and stay as they are.
