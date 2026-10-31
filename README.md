# Dotlink

Link your Android phone with your PC, over Wi-Fi or a USB cable. No account, no cloud.

On the PC it's one app: a window, a tray icon and a background service. Linux and Windows 11
are supported, and so is macOS (the webcam goes through OBS).

## What it does

| Feature | Phone → PC | PC → phone |
|---|---|---|
| **Files** | Share → Dotlink, or **Send** in the app | Drop files on the window, the tray's *Send files…*, right-click → *Share with Dotlink* (Windows) or *Quick Actions → Dotlink: …* (macOS), or `dotlink send` |
| **Clipboard** | When you open the app, or the notification's *Send clipboard* | Automatic |
| **Remote** | The phone as touchpad and keyboard | |
| **Shell** | A terminal on the PC; the session survives reconnects | |
| **Webcam** | The phone camera as **Dotlink Camera** in any app (calls, browsers, OBS) | |
| **Microphone** | The phone mic as a microphone on the PC | |
| **Cast** | | The phone's screen in a window, with mouse and keyboard ([scrcpy](https://github.com/Genymobile/scrcpy)) |

Received files land in `~/Downloads/Dotlink` on the PC and `Download/Dotlink` on the phone. Turn on
**Ask before receiving files** in Settings (PC or phone) to Accept or Decline each one first.

## Requirements

| | Needs | Tested on |
|---|---|---|
| **Phone** | Android 10 or newer; Cast over Wi-Fi needs Android 11+ | iQOO I2218, realme RMX5085 (Android 16) |
| **Linux** | x86_64, a desktop with PipeWire; any Wayland or X11 desktop | Fedora 44: GNOME, KDE Plasma, Hyprland |
| **Windows** | Windows 11, x64 (Windows 10 isn't supported) | Windows 11 Pro 26H2 (build 26300) |
| **macOS** | macOS 13 Ventura or newer, Apple Silicon or Intel | macOS 15.8 (MacBook Air M1) |

Other distros and desktops (Ubuntu, Debian, Arch, openSUSE; Sway, niri, X11 desktops) are expected to
work, since `install.sh` and Setup handle their packages, but haven't been tested on every release.
What has been tested per feature: [docs/PLATFORM_FEATURES.md](docs/PLATFORM_FEATURES.md).

## Install

Download the files from the [releases page](https://github.com/sofyan-rs/dotlink/releases).
Dotlink checks for new releases by itself (Settings → About on the PC, Settings → Check for
updates on the phone) and installs them on Windows, macOS, from the AppImage and on Android.

### Phone

Install `Dotlink-<version>.apk` from the releases page (allow your browser or file manager to
install apps when Android asks).

A debug build (`./gradlew installDebug`) can't be updated by a release APK, since they're signed
with different keys: uninstall it first.

### Linux

Pick the package for your distro:

| Distro | File | Install |
|---|---|---|
| Debian, Ubuntu, Mint, Pop!_OS | `dotlink_<version>_amd64.deb` | `sudo apt install ./dotlink_<version>_amd64.deb` |
| Fedora, openSUSE | `dotlink-<version>-1.x86_64.rpm` | `sudo dnf install ./dotlink-<version>-1.x86_64.rpm` (openSUSE: `sudo zypper install`) |
| Any other (Arch, …) | `dotlink_<version>_x86_64.AppImage` | `chmod +x` it and run it |

Then open **Dotlink** from the app menu. The AppImage updates itself; for the `.deb` and `.rpm`,
install the new package the same way.

Needs: `ffmpeg`, PipeWire, `libnotify`, `v4l2loopback` for the webcam, and `wl-clipboard` on
wlroots/KDE (the packages pull in what their distro has; Setup tells what's still missing). Works
on Hyprland, Sway, niri, KDE, GNOME and X11 desktops. Setup's fixes ask for your password through
polkit (on Hyprland, run an agent such as `hyprpolkitagent`).

Window managers that ignore autostart need one line:

```
Hyprland   exec-once = dotlink --background
Sway/i3    exec dotlink --background
niri       spawn-at-startup "dotlink" "--background"
```

**From source** instead:

```sh
scripts/install.sh --camera
```

It builds and installs `dotlink`, adds it to the app menu and starts it at login. It also checks
what's missing for your distro (dnf, apt, pacman, zypper). `--camera` sets up the webcam device
(asks for sudo); `--headless` builds without the window.

### Windows

Run `dotlink_<version>_x64-setup.exe` from the releases page: a per-user install (no
administrator rights) with a Start menu entry. Updates install from Settings → About. To build it
yourself (Rust with MSVC, `cargo install cargo-packager`):

```sh
cd desktop/app && cargo packager --release -f nsis   # desktop/target/packages/
```

The installer isn't code-signed yet: SmartScreen and the firewall show "Unknown publisher".

- **Webcam:** Setup → *Fix* installs **Dotlink Camera** (asks for administrator rights
  once).
- **Microphone:** install [VB-CABLE](https://vb-audio.com/Cable/) (free). In your app, pick
  **CABLE Output** as the microphone.

### macOS

Open `Dotlink-<version>.dmg` from the releases page and drag **Dotlink** to Applications. It isn't
signed with an Apple Developer ID, so the first time macOS refuses to open it: go to System
Settings → Privacy & Security and click **Open Anyway**. Updates install from Settings → About.

- **Allow** Dotlink to find devices on your local network when macOS asks: without it the phone
  and Cast can't find the Mac over Wi-Fi.
- **Remote:** Setup → *Fix* opens System Settings → Accessibility; switch Dotlink on. Without a
  Developer ID signature, macOS asks again after each update.
- **Microphone:** Setup → *Fix* installs [BlackHole](https://existential.audio/blackhole/) (free;
  asks for your password). In your app, pick **BlackHole 2ch** as the microphone.
- **Webcam:** through [OBS](https://obsproject.com) (free), whose virtual camera every app sees.
  macOS only accepts camera drivers signed with a Developer ID, so Dotlink can't add its own
  yet. Set it up once (below).

#### The phone camera as a webcam on macOS

1. Install OBS (Setup → *Webcam* → *Fix* opens its download page) and open it.
2. In Dotlink: **Phones → Webcam → Copy OBS link**. The link stays the same, so this is once.
3. In OBS: **Sources → + → Browser**, name it "Dotlink", then:
   - **URL:** paste the link;
   - **Width** 1280, **Height** 720;
   - OK, then right-click the source → **Transform → Fit to screen**.
4. In OBS: **Start Virtual Camera**. The first time, macOS asks to allow OBS's camera extension
   (System Settings → General → Login Items & Extensions).
5. In Dotlink: **Start** the Webcam. In your call app, pick **OBS Virtual Camera**.

A Browser source shows the phone with less delay than a Media Source. To use a Media Source
anyway: change the link's `.html` to `.mjpg`, untick **Local File**, paste it as **Input**, set
**Input Format** to `mpjpeg` and **Network Buffering** to 0 MB.

OBS has to keep running while you use the camera. While the phone camera is off, OBS shows
"Dotlink · phone camera off". The link only works on this Mac (`127.0.0.1`) and holds a secret,
so other users and programs can't read the camera.

### After installing

Open **Dotlink** and check its **Setup** page. It lists what each feature still needs and fixes most
of it in one click (firewall port, webcam, remote access, scrcpy; on macOS Accessibility,
BlackHole and OBS).

## Pairing

1. On the PC: **Pair** in Dotlink (or `dotlink pair`).
2. On the phone: scan the QR code (or paste the link).

With the phone on a USB cable (USB debugging on, this PC allowed), nothing needs scanning:
tap **Pair with <PC> over USB** in the phone app, or click **Pair over USB** on the PC (or
`dotlink pair --usb`) and tap **Pair** on the phone.

That's it. The phone finds the PC again by itself when its address changes. One phone can be paired
with several PCs: tap the PC's name at the top of the app to switch.

### Linking two phones

Two phones with Dotlink on the same Wi-Fi send each other files and the clipboard, with or
without a PC. On one phone, open the **Phones** tab → **Pair a phone** (or, without a PC, **Pair with
another phone**): it shows a QR code. On the other, do the same and tap **Scan the other phone's
code**. Then pick the phone in **Phones** to send files or the clipboard, or choose it in
**Share → Dotlink**. Phones on the same Wi-Fi show as Online there.

### Linking two PCs

Two PCs can send each other files and the clipboard. On the first PC, open **Pair**: it shows a
6-digit code. On the second, open **Pair → Join a PC**, pick the first PC, type the code and press
**Join** (or `dotlink nearby`, then `dotlink join "<name>" --code 123456`). Instead of the code,
**Copy link** on the first PC and paste the link on the second also works (`dotlink join "<link>"`). The PC then shows on the **PCs** page. Clipboard changes go
to it only with **Clipboard sync** on for that PC (`dotlink clip-auto <pc> on`); **Send clipboard**
works either way. A laptop's battery shows there too.

## Wi-Fi or USB

Everything works over both:

- **Wi-Fi:** phone and PC on the same network, or the PC on the phone's own hotspot (the phone
  looks for it on the hotspot's network itself).
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
- The webcam and mic start from the phone, or from **Phones** on the PC (`dotlink camera on`). If
  the phone app isn't open, the phone shows a notification: tap **Start**.
- Phone → PC clipboard happens when you open the app: Android only lets the app in front read the
  clipboard.
- **Phones** shows each connected phone's battery level and whether it's charging.

## CLI

```
dotlink                              open the app
dotlink pair [--usb]                 show a pairing QR code (--usb: open it on the phone on the cable)
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
camera_device = ""         # Linux: the v4l2loopback device (default: the one named Dotlink Camera)
cast_screen_off = false    # turn the phone screen off while casting
cast_audio = true          # play the phone's sound on the PC while casting
cast_max_size = 0          # longest side of the cast in pixels (0: full resolution)
usb_link = true            # use the USB cable when plugged in
theme = "system"           # "system", "light" or "dark"
```

## Security

The phone pins the PC's certificate when pairing, and every connection uses TLS plus a token.
**A paired phone can open a shell on your PC**: turn the Shell off or set a PIN if that worries you.
Forget a phone in **Phones** (or `dotlink unpair`) to cut it off right away.

## Limits

- Remote typing uses a US keyboard layout on Linux; on Windows and macOS any layout works (on
  macOS, letters in shortcuts such as ⌘C are the US keys' positions).
- One phone at a time streams the webcam (or the mic).
- Windows: Windows 11 only; the mic needs VB-CABLE.
- macOS: the webcam goes through OBS, which has to be running, and the mic through BlackHole
  (see Install). The app isn't signed with an Apple Developer ID, so macOS asks once before
  opening it, and asks again for Accessibility (Remote) after each update.

## Platforms

| | Linux | Windows | macOS |
|---|---|---|---|
| Files, clipboard, shell, remote, Cast | ✅ | ✅ | ✅ |
| Webcam, microphone | ✅ | ✅ (mic through VB-CABLE) | ✅ (webcam through OBS, mic through BlackHole) |
| Tray, start at login, Setup | ✅ | ✅ | ✅ menu bar, notifications |
| Installer | ✅ .deb, .rpm, AppImage | ✅ setup .exe (not signed yet) | ✅ .dmg (no Developer ID) |

How each feature is built per OS and what has been tested:
[docs/PLATFORM_FEATURES.md](docs/PLATFORM_FEATURES.md). Ideas for new features:
[docs/FEATURE_IDEAS.md](docs/FEATURE_IDEAS.md).

## Development

```
desktop/   Rust workspace: core/ (the service), app/ (window, tray or menu bar, CLI), camera/ (Windows webcam DLL)
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

Packages (into `desktop/target/`):

```sh
cargo install cargo-packager cargo-generate-rpm
cd desktop/app
NO_STRIP=1 cargo packager --release -f deb,appimage   # Linux
cargo packager --release -f nsis                      # Windows
cd .. && cargo generate-rpm -p app                    # Linux .rpm, after a release build
```

Build Linux releases on the oldest distro you want to support (they need its glibc or newer).
The Android release APK is signed with the key in `android/keystore.properties` (not in git),
or the debug key without it. `.github/workflows/release.yml` builds all of them; a `v*` tag
publishes them as a release (with a suffix like `v0.2.0-beta.1`, a pre-release). See
[Releasing](docs/CONTRIBUTION.md#releasing).

More: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/CONTRIBUTION.md](docs/CONTRIBUTION.md).

## License

Dotlink is free software: you can redistribute it and/or modify it under the terms of the
[GNU General Public License](LICENSE) as published by the Free Software Foundation, either version
3 of the License, or (at your option) any later version.

The fonts and scripts bundled with the Android app keep their own licenses (for example
`android/app/src/main/assets/term/symbols-mono-LICENSE.txt`). scrcpy, which Dotlink downloads for
Cast, is under the Apache License 2.0.
