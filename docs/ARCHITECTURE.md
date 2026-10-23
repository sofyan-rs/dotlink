# Architecture

Dotlink is two programs that talk over the local network: an Android app and a desktop app.
No account and no cloud: the phone pins the PC's certificate when it pairs, and everything after
that is TLS straight to the PC.

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
    src/config.rs        config.toml, state.json (paired phones), paths
    src/tls.rs           self-signed certificate
    src/term.rs          shell sessions (PTY), kept across reconnects
    src/input.rs         Remote messages (protocol only)
    src/platform/        everything OS-specific
      mod.rs             the contract every platform module fills
      linux/             complete: uinput, v4l2loopback, PipeWire, wl-clipboard/X11,
                         notify-send, XDG/Hyprland autostart, Setup checks + fixes
      windows/ macos/    stubs with TODO lists
      polling_clipboard.rs  arboard-based clipboard shared by X11, Windows, macOS
  app/                   the `dotlink` binary
    src/main.rs          CLI; `dotlink` with no command opens the app
    src/gui/             window (GPUI + gpui-component), tray, theme
    assets/              icons, desktop file, tray pixmaps
android/                 Kotlin + Jetpack Compose app
  link/                  Link (connection), LinkService (foreground service), streamers,
                         Prefs (paired PCs, settings)
  ui/                    screens (pair/ also picks a PC; home/PcSheet switches PCs);
                         ui/dotline is the Dotline design system
scripts/install.sh       build + install for the current user
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
  (ksni over D-Bus), scrcpy/adb processes.
- **GPUI** owns the main thread. `gui/model.rs` holds app-wide state in an `Entity<Model>`
  that outlives the window; the window (`gui/window.rs`) is a view of it.
- A dedicated thread owns the X11/arboard clipboard (X11 serves pasted data from the owner);
  on Linux a second one waits for XFixes clipboard-owner events and wakes it.

## Phone ↔ PC protocol

All requests carry `Authorization: Bearer <token>` except `/pair`. The PC stores only the
token's SHA-256.

| Endpoint | Use |
|---|---|
| `POST /pair` | one-time secret from the QR code → token |
| `GET /ping` | PC name and Shell settings |
| `GET /ws/control` | long-lived JSON messages, both ways (below) |
| `GET /ws/input` | Remote: pointer, scroll, keys, text |
| `GET /ws/term`, `GET /terms`, `DELETE /terms/{id}` | Shell sessions |
| `GET /ws/camera` | MJPEG frames → Dotlink Camera |
| `GET /ws/mic` | 16-bit PCM → Dotlink Mic |
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
| phone → PC | `clip` | phone clipboard text |
| phone → PC | `info` | phone name, model, Wireless debugging on, Developer options on (for Cast) |
| phone → PC | `media` | `state: needs_tap`: starting camera/mic waits for a tap |

### Pairing and trust

`dotlink pair` (or the Pair page) makes a `dotlink://pair?h=…&p=…&fp=…&c=…` URI: the PC's
addresses, port, certificate fingerprint and a one-time secret valid for 5 minutes. The phone
pins `fp` and trades `c` for a token. When the PC's IP changes, the phone finds it again with
mDNS (`_dotlink._tcp`, matched by server id).

## Control socket (CLI and window)

`$XDG_RUNTIME_DIR/dotlink.sock` on Linux/macOS (user-only), a named pipe on Windows. One JSON
request line in, one or more reply lines out.

`pair` · `send` · `clip` · `devices` · `status` · `unpair` · `set_config` · `media` · `cast` ·
`cast_pair` · `watch` (event stream, see `events.rs`) · `show` · `quit`

## Features, briefly

- **Files.** Uploads go to a `.part` file, are SHA-256 checked, then renamed. Transfers are
  tracked (`events::Transfer`) with progress for the window; the phone shows its own progress
  notifications (`TransferNotifier`).
- **Clipboard.** Linux: `wl-paste --watch` where the compositor has data-control (Hyprland,
  Sway, KDE…), else the X11 clipboard through XWayland (GNOME), read when XFixes reports a new owner (with a
  5 s safety poll; 500 ms polling where XFixes isn't available). Echo loops are cut by
  remembering the last text either way. Password-manager entries are never sent.
- **Remote.** uinput virtual mouse + keyboard (Linux).
- **Shell.** One PTY per tab per phone, scrollback replayed on reconnect.
- **Webcam / mic.** ffmpeg writes MJPEG into v4l2loopback ("Dotlink Camera"); `pw-cat` is a
  PipeWire source ("Dotlink Mic"). One phone streams each at a time (`Streaming::claim`).
  Android 14+ only lets the app start them while it's on screen, so a request from the PC in
  the background becomes a notification with Start.
- **Latency.** Nothing queues behind a slow link. The phone sends a camera frame only when the
  socket is idle, mic audio in 10 ms chunks, with TCP_NODELAY and a Wi-Fi lock while linked.
  The PC hands ffmpeg only the newest frame (a `watch` channel, `mpjpeg` input), shrinks the
  `pw-cat` pipe to one page and drops mic chunks beyond a short backlog.
- **Several PCs.** The phone keeps a list of paired PCs (`Prefs`), connects to one at a time
  and can switch or forget any; a PC sends files and streams to one phone or all.
- **Cast.** `cast.rs` finds the phone in `adb devices` by model (USB first), or its Wireless
  debugging port over mDNS (`_adb-tls-connect._tcp`) by IP, `adb connect`s and runs scrcpy.
  Pairing uses the code from the phone; its port is found over mDNS too. Without scrcpy on the
  PC, the official release (with adb) is downloaded into the data folder, checked against a
  pinned SHA-256.
- **Setup.** `platform::setup::checks` lists what each feature needs; `fix_many` fixes the
  chosen ones in one pkexec run (packages in one install command, udev rule, camera service,
  firewalld port).
- **Autostart.** XDG autostart entry; on Hyprland also `~/.config/hypr/dotlink.lua` plus one
  marked `pcall(require, "dotlink")` line, removed again when turned off.

## Platforms

`core/src/platform/mod.rs` lists the items every platform module provides. Linux is the
reference. `windows/mod.rs` and `macos/mod.rs` are stubs that compile the rest of the app and
say what to build (SendInput / CGEvent, login items, virtual camera and mic drivers, tray).
Search for `TODO(windows)` and `TODO(macos)`.

Window decorations: GPUI asks for server-side decorations; where the compositor leaves them to
the app (GNOME), the window draws gpui-component's `TitleBar` and a resizable border.

## Files on disk

| Path | Content |
|---|---|
| `~/.config/dotlink/config.toml` | settings (also edited by Settings) |
| `~/.config/dotlink/state.json` | server id, paired phones (token hashes), 0600 |
| `~/.config/dotlink/cert.pem`, `key.pem` | the certificate the phone pins |
| `~/.local/share/dotlink/scrcpy/<version>/` | scrcpy + adb, when Dotlink downloaded them |
| `$XDG_RUNTIME_DIR/dotlink.sock` | control socket |
| `~/Downloads/Dotlink/` | files from the phone (configurable) |
