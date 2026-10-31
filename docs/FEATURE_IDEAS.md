# Feature ideas

Features worth adding, in the order they're worth building. 1, 2, 5, 7 and 8 come from a look at
[Velquo](https://github.com/vsnthdev/Velquo) (a native macOS client for KDE Connect) on
2026-10-31; the others are Dotlink's own. Velquo only writes the Mac side and relies on the KDE
Connect app on the phone. Dotlink has its own Android app, so each feature is built on both
ends and on all three desktops.

1 to 6 are built (not yet tested on a phone; see PLATFORM_FEATURES.md); the rest don't exist
yet. Each lands with its own control message (`{"t": …}` on `/ws/control`,
see [ARCHITECTURE.md](ARCHITECTURE.md)); where it shows in the window is given against the
device page (sidebar → a phone or PC: header, Send, Screen camera & mic, Sync, Recent).

## Summary

| # | Feature | Direction | Size | Needs |
|---|---|---|---|---|
| 1 | [Send links](#1-send-links) (built) | both | small | — |
| 2 | [Ring the phone](#2-ring-the-phone) (built) | PC → phone | small | — |
| 3 | [The laptop's battery on the phone](#3-the-laptops-battery-on-the-phone) (built) | PC → phone | small | — |
| 4 | [Presenter mode in the Remote](#4-presenter-mode-in-the-remote) (built) | phone → PC | small | phone only |
| 5 | [Phone notifications on the PC, codes copied](#5-phone-notifications-on-the-pc-codes-copied) (built) | phone → PC | medium | notification access |
| 6 | [Browse the phone's files](#6-browse-the-phones-files) (built) | PC → phone | medium | All files access |
| 7 | [Media control](#7-media-control) | both | large | notification access; MPRIS / SMTC / MediaRemote |
| 8 | [Pause the PC's media on calls](#8-pause-the-pcs-media-on-calls) | phone → PC | small after 7 | `READ_PHONE_STATE` |

1 to 3 take a day or two together. 5 is the one people will notice most. Not planned:
run-command, which the Shell covers.

## 1. Send links

**What.** A link shared to Dotlink on the phone opens in the PC's browser, and the other way
round.

**Today.** `ShareActivity.sendToPc` sends shared text as `clip`: the link lands on the PC's
clipboard and nothing opens.

| Direction | `t` | Fields |
|---|---|---|
| phone ↔ PC | `url` | `url`: open it in the default browser |

**Phone.**
- `ShareActivity`: when `EXTRA_TEXT` is a single `http(s)` URL, send `url` instead of `clip`;
  toast "Opened on <PC>".
- Incoming `url` (`Link.kt`): `ACTION_VIEW`. Android 10+ doesn't let a background app start an
  activity, so off screen it posts a notification with **Open** instead.

**PC.**
- Incoming `url` (`server.rs`): `http`/`https` only, then `xdg-open` / `ShellExecuteW` /
  `open`, and a notification "Opened <host> from <phone>" so a stray link is noticed.
- Sending: **Send link…** in the tray (filled in with the clipboard's URL), and
  `dotlink open <url> [--to <device>]` (`ipc.rs`). Linked PCs take `url` too.

**Done when.** Share → Dotlink on a link in Chrome opens it on each desktop; `dotlink open`
opens it on the phone, with the app open and from the notification.

## 2. Ring the phone

**What.** **Ring** makes the phone ring at full volume, even on silent, until it's stopped.

| Direction | `t` | Fields |
|---|---|---|
| PC → phone | `ring` | `on`: start or stop |

**Phone.**
- `MediaPlayer` with the default ringtone (alarm as fallback), `USAGE_ALARM` so silent and
  vibrate don't mute it, looping; `STREAM_ALARM` raised to max and put back after.
- A full-screen notification "Ringing from <PC>" with **Stop** (`USE_FULL_SCREEN_INTENT`;
  Android 14+ may show it as heads-up). Stops by itself after 60 s.

**PC.** **Ring** in a connected phone's ⋯ menu; while it rings, the header shows `RINGING`
with **Stop**. `dotlink ring [<phone>]`.

**Done when.** The phone rings in silent mode with the screen off; Stop works from both ends.

## 3. The laptop's battery on the phone

**What.** The phone shows the PC's battery and charging state when the PC has a battery. A
desktop PC shows nothing.

**Today.** Half done. `platform::battery()` is read once a minute (`App::watch_battery`) and
is `None` without a battery. The `battery` message already goes to linked PCs, but only to
them: `DeviceKind::Pc` filters it in the control loop (sent after `hello`) and in
`watch_battery`'s targets.

**PC.** Drop both filters. The message is the same (`battery`: `level`, `charging`).

**Phone.**
- `Link.kt`: keep the last `battery` in the link state, cleared on disconnect.
- Home (`LinkScreen.kt`): the level next to the PC's name as a dot meter, green while
  charging, as on the PC's device page.
- Optional: a notification at 20 % while not charging (Settings, off by default).

**Done when.** A laptop on each OS shows its level on the phone, updated within a minute of
plugging in or out; a desktop PC shows nothing.

## 4. Presenter mode in the Remote

**What.** A Remote mode for slides: large Next and Previous, Start, End, Black, a timer, and
the volume keys as Next / Previous.

**PC.** Nothing: it's keys over `/ws/input`, like the keyboard.

**Phone.**
- The Remote gets a switch: Touchpad / Presenter.
- Presenter: Previous on the left third, Next on the rest, so it works without looking. A row
  with Start, End (`Esc`), Black (`B`) and a timer (tap to start, hold to reset; optional
  vibration at a set minute).
- Keys: Next `→`, Previous `←`. Start follows the PC's OS from `hello`: `F5` (PowerPoint,
  LibreOffice; `Shift+F5` from the current slide), `⌥⌘P` for Keynote.
- Volume up / down as Next / Previous while it's open (`onKeyDown`, consumed). The screen
  stays on.
- Later: a laser pointer (the gyro moving a dot over the slides) needs an overlay window on the
  PC.

**Done when.** PowerPoint (Windows), Keynote (macOS), Impress (Linux) and Google Slides in a
browser go forward, back, start and end, from the screen and the volume keys.

## 5. Phone notifications on the PC, codes copied

**What.** The phone's notifications show on the PC. A verification code in one goes to the
PC's clipboard by itself. Dismissing on the PC dismisses on the phone.

| Direction | `t` | Fields |
|---|---|---|
| phone → PC | `notif` | `key`, `app`, `pkg`, `title`, `text`, `time`, `icon` (base64 PNG, the first time per `pkg`) |
| phone → PC | `notif_removed` | `key` |
| PC → phone | `notif_dismiss` | `key` |

**Phone.**
- A `NotificationListenerService`, turned on in Settings → **Show notifications on PC** (opens
  the system's Notification access screen).
- Forwards posted and removed notifications while linked. Skips ongoing ones, group summaries,
  Dotlink's own, and apps turned off in a per-app list (chat apps off by default: they repeat
  every message).
- `notif_dismiss` → `cancelNotification(key)`.

**Android 15+** hides the text of notifications holding codes or private messages from
listeners ("Sensitive notification content hidden"). It comes back after
`adb shell appops set com.dotlink RECEIVE_SENSITIVE_NOTIFICATIONS allow`, which stays after USB
debugging is off. Dotlink already runs adb for Cast, so this is a **Setup → Fix** item ("Show
verification codes") for a phone on USB or Wireless debugging. Velquo leaves it to manual steps.

**PC.**
- Shown through `platform::Notify` (notify-send, toasts, UNUserNotificationCenter) as
  "<app> · <phone>", with **Dismiss**.
- Codes: 4–8 digits (or `G-123456`) near "code", "OTP", "verification", "kode",
  "verifikasi" are copied, and the notification says "Code 123456 copied". Not echoed back to
  the phone as `clip`.
- Device page → Sync: **Notifications** and **Copy verification codes**, per phone.

**Privacy.** Text only travels over the paired TLS link and isn't written to disk; only the
icons are cached.

**Done when.** A chat, SMS or bank notification shows on each desktop within a second, and an
SMS code reaches the PC's clipboard on Android 14, and on 15+ after Setup's Fix.

## 6. Browse the phone's files

**What.** The phone's storage from the PC: folders and files, download, upload into the open
folder, rename and delete.

**Why not SFTP.** KDE Connect runs an SSH server on the phone and mounts it. Dotlink already has
a TLS link with transfers both ways, so browsing is a few messages on top: no SSH server, no
`sshfs` / macFUSE / WinFsp on the PC.

| Direction | `t` | Fields |
|---|---|---|
| PC → phone | `fs_list` | `req`, `path` (`/` is internal storage) |
| phone → PC | `fs_list` | `req`, `entries` (`name`, `dir`, `size`, `mtime`), or `error` |
| PC → phone | `fs_get` | `req`, `path`: the phone uploads it (`POST /upload` with `req`), saved where the PC asked |
| PC → phone | `fs_put` | `req`, `path`: the usual `file` offer, saved into `path` instead of `Download/Dotlink` |
| PC → phone | `fs_rm`, `fs_mv`, `fs_mkdir` | `req`, `path` (+ `to`) |
| phone → PC | `fs_ok` | `req`, or `error` |

**Phone.**
- **All files access** (`MANAGE_EXTERNAL_STORAGE`, Android 11+; legacy storage on 10): the
  whole storage as plain paths, simple and fast. Play restricts it to file managers, but
  Dotlink ships on GitHub. (The Storage Access Framework would limit it to folders the user
  picks, slower.)
- **Browse from PC** in Settings, off by default; the first request shows a notification
  leading to the system switch. `Android/data` and `Android/obb` stay hidden either way.
- Every path is canonicalized and kept under the storage root. Only paired PCs may ask, not
  linked phones.

**PC.**
- Device page → **Files**: a path bar, folders first, name / size / date. Double-click opens a
  folder, or downloads and opens a file; files dropped on the list go into that folder;
  right-click for Download, Rename, Delete. Image thumbnails later (`fs_thumb`).
- `dotlink ls <phone> <path>`, `dotlink get <phone>:<path> [<dest>]`.
- Later: mount it as a drive through a WebDAV server on `127.0.0.1` that forwards to these
  messages. All three desktops mount WebDAV without drivers (Windows' WebClient is fussy with
  large files).

**Done when.** On each desktop: browse internal storage, download a 1 GB video (SHA-256
checked), upload files into `DCIM`, rename and delete. With Browse from PC off, every request
fails.

## 7. Media control

**What.** The phone shows and controls what plays on the PC, and the PC shows and controls what
plays on the phone: title, artist, artwork, play/pause, next, previous, seek, volume.

**Today.** The Remote sends media keys to the PC, blind.

| Direction | `t` | Fields |
|---|---|---|
| phone ↔ PC | `media_state` | `player`, `title`, `artist`, `album`, `playing`, `position`, `duration`, `art` (base64 JPEG, when it changes) |
| phone ↔ PC | `media_cmd` | `cmd`: `play`, `pause`, `toggle`, `next`, `prev`, `seek` (+ `ms`), `volume` (+ `level`) |

(`media` is taken: camera and mic use it for `needs_tap`.)

**The PC's player.** A new `platform::media` (`now_playing()`, `command()`): MPRIS over D-Bus
on Linux, the System Media Transport Controls on Windows, MediaRemote on macOS (a private
framework; Velquo reaches it through an Objective-C bridge). The phone shows it on a **Media**
card in the app.

**The phone's player.** `MediaSessionManager.getActiveSessions`, which needs the notification
access from 5; commands go to `MediaController.transportControls`. The PC shows it on the
device page (Now playing, with the controls) and in the tray.

**Done when.** Spotify, and YouTube in a browser, on each desktop show on the phone and follow
its controls; the phone's player shows on the PC and follows the PC's.

## 8. Pause the PC's media on calls

**What.** When the phone rings or a call starts, the PC pauses what it plays, and resumes it
when the call ends, only if it was the one that paused it.

| Direction | `t` | Fields |
|---|---|---|
| phone → PC | `call` | `state`: `ringing`, `offhook`, `idle` |

**Phone.** `TelephonyCallback.CallStateListener` (Android 12+; `PhoneStateListener` before),
with `READ_PHONE_STATE` asked when the setting is turned on.

**PC.** On `ringing` or `offhook`, pause if `platform::media` (7) says something plays, and
remember it; on `idle`, resume. Device page → Sync: **Pause media during calls**. Without 7 it
could press the play/pause key, but couldn't tell whether anything was playing.

**Done when.** A call pauses music on each desktop and it resumes after hanging up; a call while
nothing plays starts nothing.

## When one lands

- README: the feature table.
- [ARCHITECTURE.md](ARCHITECTURE.md): the control message, and "Features, briefly".
- [PLATFORM_FEATURES.md](PLATFORM_FEATURES.md): how it's done on each OS, and what was tested.
