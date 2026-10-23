# Dotlink

Link your Android phone with your PC, over Wi-Fi or a USB cable. No account, no cloud.

On the PC it's one app: a window, a tray icon and a background service. Linux is complete,
Windows works (installer still to come), macOS is planned.

## What it does

| Feature | Phone → PC | PC → phone |
|---|---|---|
| **Files** | Share → Dotlink, or **Send** in the app | Drop files on the window, the tray's *Send files…*, or `dotlink send` |
| **Clipboard** | When you open the app, or the notification's *Send clipboard* | Automatic |
| **Remote** | The phone as touchpad and keyboard | |
| **Shell** | A terminal on the PC; the session survives reconnects | |
| **Webcam** | The phone camera as **Dotlink Camera** in any app (calls, browsers, OBS) | |
| **Microphone** | The phone mic as a microphone on the PC | |
| **Cast** | | The phone's screen in a window, with mouse and keyboard ([scrcpy](https://github.com/Genymobile/scrcpy)) |

Received files land in `~/Downloads/Dotlink` on the PC and `Download/Dotlink` on the phone.

## Install

### Phone

```sh
cd android && ./gradlew installDebug
```

Or copy `android/app/build/outputs/apk/debug/app-debug.apk` to the phone.

### Linux

```sh
scripts/install.sh --camera
```

It builds and installs `dotlink`, adds it to the app menu and starts it at login. It also checks
what's missing for your distro (dnf, apt, pacman, zypper). `--camera` sets up the webcam device
(asks for sudo); `--headless` builds without the window.

Needs: `ffmpeg`, PipeWire, `libnotify`, `v4l2loopback` for the webcam, and `wl-clipboard` on
wlroots/KDE. Works on Hyprland, Sway, niri, KDE, GNOME and X11 desktops. Setup's fixes ask for your
password through polkit (on Hyprland, run an agent such as `hyprpolkitagent`).

Window managers that ignore autostart need one line:

```
Hyprland   exec-once = dotlink --background
Sway/i3    exec dotlink --background
niri       spawn-at-startup "dotlink" "--background"
```

### Windows

No installer yet; build it (Rust, MSVC):

```sh
cd desktop && cargo build --release
```

Run `desktop/target/release/dotlink.exe`, keeping `dotlink_camera.dll` next to it.

- **Webcam:** Windows 11. Setup → *Fix* installs **Dotlink Camera** (asks for administrator rights
  once).
- **Microphone:** install [VB-CABLE](https://vb-audio.com/Cable/) (free). In your app, pick
  **CABLE Output** as the microphone.

### After installing

Open **Dotlink** and check its **Setup** page. It lists what each feature still needs and fixes most
of it in one click (firewall port, webcam, remote access, scrcpy).

## Pairing

1. On the PC: **Pair a phone** in Dotlink (or `dotlink pair`).
2. On the phone: scan the QR code (or paste the link).

That's it. The phone finds the PC again by itself when its address changes. One phone can be paired
with several PCs: tap the PC's name at the top of the app to switch.

## Wi-Fi or USB

Everything works over both:

- **Wi-Fi:** phone and PC on the same network.
- **USB cable:** turn on USB debugging on the phone (Developer options) and allow this PC once.
  When the cable is in, Dotlink uses it automatically and goes back to Wi-Fi when it's unplugged.
  No Wi-Fi or firewall rule needed, and it's faster.

The app shows which one it's using ("USB cable" or the PC's address).

**Cast** needs Developer options on the phone: USB debugging over the cable, or *Wireless
debugging* (Android 11+) over Wi-Fi, paired once with the 6-digit code the phone shows (type it in
Dotlink).

## Using it

- Closing the window keeps Dotlink in the tray, so the phone stays connected. Quit from the tray or
  Settings.
- The webcam and mic start from the phone, or from **Devices** on the PC (`dotlink camera on`). If
  the phone app isn't open, the phone shows a notification: tap **Start**.
- Phone → PC clipboard happens when you open the app: Android only lets the app in front read the
  clipboard.

## CLI

```
dotlink                              open the app
dotlink pair                         show a pairing QR code
dotlink send [--to PHONE] FILE...    send files
dotlink clip [--to PHONE]            send the PC clipboard
dotlink devices                      list paired phones
dotlink unpair PHONE                 forget a phone
dotlink camera on|off [--to PHONE]   phone camera as Dotlink Camera
dotlink mic on|off [--to PHONE]      phone mic as a microphone
dotlink cast [stop] [--to PHONE]     show a phone's screen
dotlink cast pair CODE               pair with the phone's Wireless debugging
dotlink daemon                       run without a window
dotlink quit                         stop Dotlink
```

## Settings

The **Settings** page, or `config.toml` in `~/.config/dotlink/` (Linux) or `%APPDATA%\dotlink\`
(Windows):

```toml
name = ""                  # what the phones call this PC (default: the hostname)
port = 47390
download_dir = "~/Downloads/Dotlink"
clipboard_sync = true
terminal = true            # false turns the Shell off
terminal_pin = ""          # require this PIN to open the Shell
shell = ""                 # default: your login shell (Linux), pwsh or powershell (Windows)
camera_size = "1920x1080"  # Dotlink Camera's fixed size
cast_screen_off = false    # turn the phone screen off while casting
cast_audio = true          # play the phone's sound on the PC while casting
cast_max_size = 0          # longest side of the cast in pixels (0: full resolution)
usb_link = true            # use the USB cable when plugged in
theme = "system"           # "system", "light" or "dark"
```

## Security

The phone pins the PC's certificate when pairing, and every connection uses TLS plus a token.
**A paired phone can open a shell on your PC**: turn the Shell off or set a PIN if that worries you.
Forget a phone in **Devices** (or `dotlink unpair`) to cut it off right away.

## Limits

- Remote typing uses a US keyboard layout on Linux; on Windows any layout works.
- One phone at a time streams the webcam (or the mic).
- Windows: the webcam needs Windows 11, the mic needs VB-CABLE.

## Platforms

| | Linux | Windows | macOS |
|---|---|---|---|
| Files, clipboard, shell, remote, Cast | ✅ | ✅ | planned |
| Webcam, microphone | ✅ | ✅ (Windows 11 / VB-CABLE) | planned |
| Tray, start at login, Setup | ✅ | ✅ | planned |
| Installer | ✅ .deb, .rpm, AppImage | not yet | planned |

How each feature is built per OS and what has been tested:
[docs/PLATFORM_FEATURES.md](docs/PLATFORM_FEATURES.md). Plan for the rest:
[docs/PORTING.md](docs/PORTING.md).

## Development

```
desktop/   Rust workspace: core/ (the service), app/ (window, tray, CLI), camera/ (Windows webcam DLL)
android/   Kotlin + Jetpack Compose app
scripts/   install.sh
docs/      architecture, porting, platform features, contributing
```

```sh
cd desktop
cargo run -p dotlink                          # the app
cargo run -p dotlink -- daemon                # headless
cargo build -p dotlink --no-default-features  # without the window
```

Linux packages (into `desktop/target/`):

```sh
cargo install cargo-packager cargo-generate-rpm
NO_STRIP=1 cargo packager --release -f deb,appimage
cargo generate-rpm -p app
```

Build releases on the oldest distro you want to support (they need its glibc or newer).

More: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/CONTRIBUTION.md](docs/CONTRIBUTION.md).
