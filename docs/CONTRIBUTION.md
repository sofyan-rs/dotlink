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

### Android

```sh
cd android
./gradlew installDebug     # build and install on the phone over adb
```

Some phones (ColorOS / realme, MIUI) refuse adb installs until you confirm a dialog on the
phone, or turn off *Verify apps over USB* in Developer options.

### Packages

```sh
cargo install cargo-packager cargo-generate-rpm
cd desktop
NO_STRIP=1 cargo packager --release -f deb,appimage   # target/packages/
cargo build --release -p dotlink && cargo generate-rpm -p app   # target/generate-rpm/
```

`NO_STRIP=1` works around linuxdeploy's `strip` being too old for current distros' libraries.
Packages need the glibc they were built with or newer: build releases on the oldest distro you
support.

## Testing without disturbing your own setup

The service keeps its config in `$XDG_CONFIG_HOME/dotlink` and its socket in
`$XDG_RUNTIME_DIR`, so a second instance can run next to yours:

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
- **New system requirement:** a check in `platform/linux/setup.rs`, fixable when it can be
  installed or set up with one root command.

## Windows and macOS

The stubs in `core/src/platform/windows` and `macos` were written on Linux and have not been
compiled for those systems yet. [PORTING.md](PORTING.md) has the plan: the order of work, the APIs to use for each
platform item, packaging and signing, and a checklist per OS.

## Commits

One topic per commit, subject `Area: what changed` (e.g. `Cast: download scrcpy when it's
missing`), body for the why when it isn't obvious.
