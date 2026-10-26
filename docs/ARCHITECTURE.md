# Architecture

Dotlink is two programs that talk over the local network, or over a USB cable: an Android app
and a desktop app. No account and no cloud: the phone pins the PC's certificate when it pairs,
and everything after that is TLS straight to the PC.

```
 Android phone                                  PC
┌───────────────────────┐   HTTPS + WebSocket   ┌──────────────────────────────────────────┐
│ Link (control socket) │ ────────────────────▶ │ dotlink-core: server (axum)              │
│ LinkService           │   port 47390, TLS     │   pairing, files, clipboard, remote,     │
│ Camera / Mic streamer │   pinned certificate  │   shell, camera, mic, cast               │
│ UI (Compose, Dotline) │ ◀──── mDNS ────────── │ mDNS: _dotlink._tcp                      │
└───────────────────────┘                       │ control socket (JSON lines) ◀── CLI      │
                                                │                             ◀── window   │
                                                │ dotlink (app): GPUI window + tray        │
                                                └──────────────────────────────────────────┘
```

## Repository

```
desktop/                 Rust workspace
  core/                  dotlink-core: the service, no UI
    src/server.rs        App state + the HTTPS/WebSocket API for the phone
    src/service.rs       start/stop: control socket, clipboard watch, mDNS, HTTPS
    src/ipc.rs           control socket for the CLI and the window
    src/events.rs        what the window follows live (devices, transfers, …)
    src/cast.rs          Cast: finding the phone over adb, running scrcpy
    src/usb.rs           the link over a USB cable: `adb reverse` for each phone plugged in
    src/peer.rs          linking to another PC: the phone's side of the API, in Rust
    src/config.rs        config.toml, state.json (paired phones), paths
    src/tls.rs           self-signed certificate
    src/term.rs          shell sessions (PTY), kept across reconnects
    src/input.rs         Remote messages (protocol only)
    src/platform/        everything OS-specific
      mod.rs             the contract every platform module fills
      linux/             complete: uinput, v4l2loopback, PipeWire, wl-clipboard/X11,
                         notify-send, XDG/Hyprland autostart, Setup checks + fixes
      windows/           complete (Windows 11): SendInput, Run-key autostart, toasts,
                         virtual camera, VB-CABLE mic, Setup checks + UAC fixes
      macos/             stubs with TODO lists
      polling_clipboard.rs  arboard-based clipboard shared by X11, Windows, macOS
  app/                   the `dotlink` binary
    src/main.rs          CLI; `dotlink` with no command opens the app
    src/gui/             window (GPUI + gpui-component), tray, theme
    assets/              icons, desktop file, dotlink.ico (the tray icon is drawn in tray.rs)
    build.rs             Windows: embeds the icon in the .exe
  camera/                dotlink-camera: Windows 11 "Dotlink Camera", a media source DLL
                         the camera frame server loads (shared.rs: the frame file)
android/                 Kotlin + Jetpack Compose app
  link/                  Link (connection), LinkService (foreground service), streamers,
                         Prefs (paired PCs, settings)
  peer/                  phone ↔ phone: PhoneTls (Keystore key + certificate), PhoneServer
                         (a minimal HTTPS server), Phones (pairing, sending, NSD)
  ui/                    screens (pair/ also picks a PC; home/PcSheet switches PCs; phones/);
                         ui/dotline is the Dotline design system
scripts/install.sh       build + install for the current user (Linux)
scripts/make-windows-icon.py  draws desktop/app/assets/dotlink.ico
.github/workflows/release.yml installers for every OS (by hand or on a v* tag)
```

## Desktop: one process, three faces

The `dotlink` binary runs in one of three ways:

| Command | What runs |
|---|---|
| `dotlink` | the service, the window and the tray in one process |
| `dotlink --background` | the same, without opening the window (login) |
| `dotlink daemon` | the service only (headless) |

A second launch never starts a second service. It connects to the control socket first:
an app with a window brings its window up (`show`); a headless daemon gets a window that
controls it. Closing the window keeps the service (and the phone connection) running; Quit in
the tray or Settings, `dotlink quit` or SIGTERM stops it, and the process exits with it.

The window never calls the service directly: it uses the control socket like the CLI does
(`gui/client.rs`), so it works the same whether the service is in-process or a separate
daemon. Socket I/O runs on the tokio runtime; GPUI awaits plain futures/channels.

### Threads and runtimes

- **tokio** (multi-threaded): the server, the control socket, clipboard watching, the tray
  on Linux (ksni over D-Bus), scrcpy/adb processes, the USB link watcher.
- On Windows the tray (`tray-icon`) lives on the main thread, pumped by GPUI's message loop;
  the webcam has its own thread (decoding, the virtual camera's COM objects), the mic a thread
  that owns the WASAPI stream.
- **GPUI** owns the main thread. `gui/model.rs` holds app-wide state in an `Entity<Model>`
  that outlives the window; the window (`gui/window.rs`) is a view of it.
- A dedicated thread owns the X11/arboard clipboard (X11 serves pasted data from the owner);
  on Linux a second one waits for XFixes clipboard-owner events and wakes it.

## Phone ↔ PC protocol

All requests carry `Authorization: Bearer <token>` except `/pair`. The PC stores only the
token's SHA-256.

Each paired device has a kind, `phone` or `pc` (`/pair` takes `kind`; phones leave it out). A
PC linked to this one gets files and the clipboard only: `/ws/input`, `/ws/term`, `/terms`,
`/ws/camera` and `/ws/mic` answer it with 403, and camera, mic and Cast only pick phones.

| Endpoint | Use |
|---|---|
| `POST /pair` | one-time secret from the QR code (+ `kind`) → token; refused when `device_id` is this PC's own id, or a PC this one already joined |
| `DELETE /pair` | the caller unpairs itself (a linked PC that forgot this one) |
| `POST /pair/code`, `POST /pair/code/confirm` | a PC joining with the 6-digit code this PC shows: SPAKE2 key exchange, then both sides' proofs and the token (`pair_code.rs`) |
| `GET /pair/usb`, `POST /pair/usb` | over the cable only (loopback): "is a Dotlink PC there?", and "send me the pairing link". The PC opens the link in the app through adb, never on this connection |
| `GET /ping` | PC name and Shell settings |
| `GET /ws/control` | long-lived JSON messages, both ways (below) |
| `GET /ws/input` | Remote: pointer, scroll, keys, text |
| `GET /ws/term`, `GET /terms`, `DELETE /terms/{id}` | Shell sessions |
| `GET /ws/camera` | MJPEG frames → Dotlink Camera |
| `GET /ws/mic` | 16-bit PCM → the PC's microphone (Dotlink Mic / VB-CABLE) |
| `POST /upload?name=` | phone → PC file, `X-Sha256` checked |
| `GET /download/{id}` | PC → phone file, offered over the control socket |

The server speaks HTTP/1.1 only (ALPN): over HTTP/2 the phone's OkHttp had large downloads
reset halfway.

Control messages (`{"t": …}`):

| Direction | `t` | Meaning |
|---|---|---|
| PC → phone | `hello` | name, Shell on/off, OS (on connect; the Remote labels Super / Win / Cmd by it) |
| PC → phone | `clip` | PC clipboard text |
| PC → phone | `file` | a file offer: id, name, size; the phone downloads it |
| PC → phone | `camera`, `mic` | `on: bool`, from Start/Stop on the PC |
| PC → phone | `adb_wifi` | Cast needs Wireless debugging: show the notification |
| PC ↔ PC | `battery` | `level`, `charging`: a linked laptop's battery, on connect and when it changes |
| phone → PC | `clip` | phone clipboard text |
| phone → PC | `info` | phone name, model, Wireless debugging on, Developer options on (for Cast) |
| phone → PC | `decline` | `id`: the file offer it won't take ("Ask before receiving files"); the PC lists it as declined |
| phone → PC | `media` | `state: needs_tap`: starting camera/mic waits for a tap |
| phone → PC | `battery` | `level` (percent), `charging`; on connect and when either changes. The Phones page shows it while the phone is connected |

### Wi-Fi or USB

The phone tries `127.0.0.1:47390` first. With USB debugging allowed, the PC runs
`adb reverse tcp:47390 tcp:47390` for every phone plugged in (`usb.rs`, checked every 3 s), so
that address reaches the PC through the cable; otherwise the phone uses the PC's network
addresses, found again over mDNS. On Wi-Fi it checks the cable every 5 s and moves over; when
the cable goes, it reconnects over Wi-Fi. The PC counts connections from loopback as USB
(`PhoneInfo::usb`) and keeps the phone's Wi-Fi address for Cast. The pinned certificate makes
the cable no more trusted than the network.

### Pairing and trust

Over a USB cable the phone can ask instead: `POST /pair/usb` with a nonce, from the pair screen.
Any app on the phone can reach `127.0.0.1` while the cable is in, so the PC doesn't answer
with the secret; it sends the link, with `r=<nonce>` added, to Dotlink through `adb shell am
broadcast` (`usb::open_pairing`), which only a PC this phone allowed USB debugging for can do.
Every phone on the cable gets it; only the one whose request `r` matches pairs (without a tap),
the others ignore it. Each request gets its own one-time code (`usb_pairings`, 5 minutes), so
several phones can pair at once and the QR code on the PC's screen keeps working.

`dotlink pair` (or the Pair page) makes a `dotlink://pair?h=…&p=…&fp=…&c=…` URI: the PC's
addresses, port, certificate fingerprint and a one-time secret valid for 5 minutes. The phone
pins `fp` and trades `c` for a token. When the PC's IP changes, the phone finds it again with
mDNS (`_dotlink._tcp`, matched by server id).

### Phone ↔ phone

Two phones send each other files and the clipboard directly; there is no lasting connection
and no PC involved (`android/.../peer/`).

- **Server.** While a phone is paired with another (or shows its pairing code), LinkService runs
  `PhoneServer`: HTTPS on the first free port from 47391, one request per connection, bodies by
  Content-Length. Its certificate is the self-signed one the Android Keystore makes for an EC key
  (`PhoneTls`), so no crypto library. It is advertised over NSD as `_dotlink-phone._tcp` (TXT `id`,
  `fp`).
- **Pairing.** One phone shows a QR code, `dotlink://phone?h=…&p=…&fp=…&c=…&id=…&n=…` (a one-time
  secret, 5 minutes); the other scans it (or pastes it) and sends `POST /pair` over TLS pinned to
  `fp`, with the secret, its own fingerprint, addresses and a token for the shower to present. The
  reply carries the token the scanner presents. Each side stores the other in `Prefs.phones`:
  fingerprint, addresses, the token it presents and the SHA-256 of the one it accepts.
- **Endpoints** (Bearer token): `POST /upload?name=` (into Download/Dotlink, with a notification),
  `POST /clip` (`{"text"}`, set right away: Android only restricts reading the clipboard),
  `POST /forget`.
- **Sending** tries the stored addresses, then looks the phone up over NSD by id. A 401 means it
  forgot this phone, which then forgets it too.
- The Phones tab lists paired phones, Online while NSD finds them (looked for only while the
  list is on screen), and the files sent between phones; the PC tab keeps to the PC. With phones
  but no PC, only the Phones and Settings tabs show. Share → Dotlink asks where to send when
  phones are paired.
- After the phone restarts, it receives again once Dotlink has been opened (no boot receiver).

### Linking two PCs

`dotlink join` (`peer.rs`) pairs with another PC as `kind: pc`, with its `dotlink://pair` link or
with the 6-digit code its Pair page shows (below),
and from then on behaves like a phone towards it: it pins the other PC's certificate, keeps
`/ws/control` open (reconnecting with backoff, through the stored addresses, then mDNS by id)
and moves files with `/upload` and `/download/{id}`. Only the PC that joined dials, so a pair
of PCs has one link; the other PC lists it as a paired device of kind `pc`, and refuses to
join it back. Either side can unpair: the joiner calls `DELETE /pair`, and the other PC's
refusal (401) makes the joiner forget it.

Phones get clipboard changes while `clipboard_sync` is on; a PC only with `clipboard_auto` set
for it (`dotlink clip-auto <pc> on`), since two PCs copying would keep overwriting each other.
`dotlink clip` and `dotlink send` reach linked PCs like phones.

**Joining with a code.** The joining PC lists the others from mDNS (`nearby`: `_dotlink._tcp`
with `id`, `fp` = the first 16 hex digits of the fingerprint, and `n` = the name), connects to the
chosen one accepting any certificate, and checks it against `fp`. A 6-digit code can't simply be
sent: a fake PC on the network would get it, and it is guessed offline in a moment. It is the
password of a SPAKE2 exchange whose identities include the certificate fingerprint the joiner
sees, so both PCs only get the same key with the right code and no one in between. The joiner
proves the key, gets its token and checks the other PC's proof; both requests use one
connection. Each wrong code counts; after 3 the code is dropped and the Pair page needs a new one.

## Control socket (CLI and window)

`$XDG_RUNTIME_DIR/dotlink.sock` on Linux/macOS (user-only), on Windows the named pipe
`\\.\pipe\dotlink-<user SID>` with an ACL that only lets that user in. One JSON
request line in, one or more reply lines out.

`pair` (also returns the code) · `pair_usb` (opens the pairing link on the phones on the cable, `usb.rs`) · `join` · `nearby` · `join_code` · `send` · `clip` · `clipboard_auto` · `devices` · `status` · `unpair` · `set_config` · `media` · `cast` ·
`cast_pair` · `watch` (event stream, see `events.rs`) · `show` · `quit`

## Features, briefly

- **Files.** With "Ask before receiving files" on (`ask_before_receiving`, and the phone's own
  setting), a file waits for Accept: `App::ask_incoming` shows a notification with Accept /
  Decline (Windows: `dotlink:accept/<id>` links; Linux: notify-send actions) and a banner in
  the window, and declines after 2 minutes. An upload isn't read until then (senders allow 3
  minutes); an offer is declined with a `decline` message. Uploads go to a `.part` file, are SHA-256 checked, then renamed. Transfers are
  tracked (`events::Transfer`) with progress for the window; the phone shows its own progress
  notifications (`TransferNotifier`).
- **Clipboard.** Linux: `wl-paste --watch` where the compositor has data-control (Hyprland,
  Sway, KDE…), else the X11 clipboard through XWayland (GNOME), read when XFixes reports a new owner (with a
  5 s safety poll; 500 ms polling where XFixes isn't available). Echo loops are cut by
  remembering the last text either way. Password-manager entries are never sent.
  Windows polls the clipboard through arboard every 500 ms.
- **Remote.** uinput virtual mouse + keyboard (Linux); SendInput, with Unicode text in any
  layout (Windows).
- **Shell.** One PTY per tab per phone (ConPTY and pwsh on Windows), scrollback replayed on
  reconnect. The phone draws it with xterm.js (WebGL): Geist Mono, then "Dotlink Symbols Mono"
  (a JuliaMono subset of the symbol blocks) and Symbols Nerd Font Mono scaled to one cell.
- **Webcam / mic.** Linux: ffmpeg writes MJPEG into v4l2loopback ("Dotlink Camera"); `pw-cat` is
  a PipeWire source ("Dotlink Mic"). Windows 11: a virtual camera (`MFCreateVirtualCamera`)
  whose media source is `dotlink_camera.dll`, fed NV12 through `%ProgramData%\Dotlink\camera.bin`;
  the mic plays into VB-CABLE through WASAPI. One phone streams each at a time
  (`Streaming::claim`). Android 14+ only lets the app start them while it's on screen, so a
  request from the PC in the background becomes a notification with Start. Phones whose
  voice-processing input is silent outside a call (vivo / iQOO) fall back to the plain mic.
  The phone offers 30 or 60 fps, only what the lens in use lists; the Windows camera offers
  both rates and paces by the one the app picks.
- **Battery.** The phone reports its level and charging state (`battery`); the Phones page
  shows it for connected phones. Linked PCs send each other the same message when they have
  a battery (`platform::battery()`: `GetSystemPowerStatus` on Windows, `/sys/class/power_supply`
  on Linux, `pmset -g batt` on macOS), on connect and when it changes (read once a minute).
  "Charging" means on power, as on the phone.
- **Tray.** The logo's dots in one colour (`tray.rs`, `glyph`): a symbolic SVG the panel
  recolours on Linux, white or black with the taskbar's mode on Windows. Started before the
  bar at login (Hyprland), it registers once a tray appears.
- **Latency.** Nothing queues behind a slow link. The phone sends a camera frame only when the
  socket is idle, mic audio in 10 ms chunks, with TCP_NODELAY and a Wi-Fi lock while linked.
  The PC hands ffmpeg only the newest frame (a `watch` channel, `mpjpeg` input), shrinks the
  `pw-cat` pipe to one page and drops mic chunks beyond a short backlog.
- **Several PCs.** The phone keeps a list of paired PCs (`Prefs`), connects to one at a time
  and can switch or forget any; a PC sends files and streams to one phone or all.
- **Cast.** `cast.rs` finds the phone in `adb devices` by model (USB first), or its Wireless
  debugging port over mDNS (`_adb-tls-connect._tcp`) by IP, `adb connect`s and runs scrcpy.
  Each paired phone has its own cast (state, scrcpy, adb serial), so several run at once; a
  serial in use by one phone isn't matched to another, and with two phones of the same model
  only the IP (Wi-Fi) identifies them. Status reports `casts`: device id → state.
  Pairing uses the code from the phone; its port is found over mDNS too. Without scrcpy on the
  PC, the official release (with adb) is downloaded into the data folder, checked against a
  pinned SHA-256.
- **Setup.** `platform::setup::checks` lists what each feature needs; `fix_many` fixes the
  chosen ones. Linux: one pkexec run (packages in one install command, udev rule, camera
  service, firewalld port). Windows: one UAC prompt each for the firewall rule and installing
  the camera DLL (each build under its own `dotlink_camera-<hash>.dll`; the frame server is
  stopped afterwards, or it keeps serving the old DLL); scrcpy/adb download without admin
  rights. The camera counts as installed while its registry key's `DotlinkSource` matches
  `shared::SOURCE`, a hash of the camera crate's source (`camera/build.rs`): a release that
  doesn't touch the camera asks for no new install, though its DLL's bytes differ.
- **Autostart.** Linux: XDG autostart entry; on Hyprland also `~/.config/hypr/dotlink.lua` plus
  one marked `pcall(require, "dotlink")` line, removed again when turned off. Windows: the
  `Dotlink` value under `HKCU\…\CurrentVersion\Run`.

## Platforms

`core/src/platform/mod.rs` lists the items every platform module provides. Linux is the
reference. `windows/` is complete for Windows 11. `macos/mod.rs` is a stub that compiles the
rest of the app and says what to build (CGEvent, login items, virtual camera and mic drivers,
tray).
Search for `TODO(windows)` and `TODO(macos)`; [PORTING.md](PORTING.md) is the plan, step by step, and [PLATFORM_FEATURES.md](PLATFORM_FEATURES.md) what each OS does today.

Window decorations: GPUI asks for server-side decorations; where the compositor leaves them to
the app (GNOME), the window draws its own title bar (`gui/title_bar.rs`: libadwaita's look, the
buttons and sides from GNOME's `button-layout`, followed live) and gpui-component's resizable
border.

## Files on disk

Linux paths; on Windows the config folder is `%APPDATA%\dotlink\` and the control socket the
named pipe above.

| Path | Content |
|---|---|
| `~/.config/dotlink/config.toml` | settings (also edited by Settings) |
| `~/.config/dotlink/dotlink.log` | what each launch did: who started it, with which arguments, and why the window opened (moves to `dotlink.old.log` at 1 MB) |
| `~/.config/dotlink/state.json` | server id, paired devices (kind, token hashes), linked PCs (fingerprint, addresses, token), 0600 |
| `~/.config/dotlink/cert.pem`, `key.pem` | the certificate the phone pins |
| `~/.local/share/dotlink/scrcpy/<version>/` | scrcpy + adb, when Dotlink downloaded them |
| `$XDG_RUNTIME_DIR/dotlink.sock` | control socket |
| `~/Downloads/Dotlink/` | files from the phone (configurable) |
