# Feature ideas

Features worth adding, picked from a look at [Velquo](https://github.com/vsnthdev/Velquo) (a
native macOS KDE Connect client) on 2026-10-31. Velquo only writes the Mac side and leans on the
KDE Connect app on the phone; Dotlink has its own Android app, so each feature here is built on
both ends and on all three desktops.

None of these exist yet: there is no `NotificationListenerService`, `MediaSession`, telephony or
"ring" code in `android/` or `desktop/`.

## Summary

| # | Feature | Direction | Size | Needs |
|---|---|---|---|---|
| 1 | [Send links](#1-send-links) | both | small | nothing new |
| 2 | [Ring the phone](#2-ring-the-phone) | PC → phone | small | nothing new |
| 3 | [Phone notifications on the PC, OTP auto-copy](#3-phone-notifications-on-the-pc-otp-auto-copy) | phone → PC | medium | notification access |
| 4 | [Media control](#4-media-control) | both | large | notification access; MPRIS / SMTC / MediaRemote |
| 5 | [Pause the PC's media on calls](#5-pause-the-pcs-media-on-calls) | phone → PC | small once 4 is done | `READ_PHONE_STATE` |
| 6 | [The laptop's battery on the phone](#6-the-laptops-battery-on-the-phone) | PC → phone | small | nothing new |
| 7 | [Presenter mode in the Remote](#7-presenter-mode-in-the-remote) | phone → PC | small | nothing new (phone only) |
| 8 | [Browse the phone's files from the PC](#8-browse-the-phones-files-from-the-pc) | PC → phone | medium | All files access on the phone |

Suggested order: 1, 2 and 6 first (small, a day or two together), then 7, then 3 (the most
useful), then 8, then 4 and 5.

Not planned: run-command (Shell covers it).

## 1. Send links

**What.** A link shared to Dotlink on the phone opens in the PC's default browser, and the
other way round.

**Today.** `ShareActivity.sendToPc` sends shared text as `clip`, so a link lands on the PC's
clipboard and nothing opens.

**Protocol.** New control message, both ways:

| Direction | `t` | Meaning |
|---|---|---|
| phone ↔ PC | `url` | `url`: open it in the default browser |

**Phone.**
- `ShareActivity`: when `EXTRA_TEXT` is a single `http(s)` URL (trimmed, no spaces), send `url`
  instead of `clip`. Toast "Opened on <PC>".
- Incoming `url` (`Link.kt`): open with `ACTION_VIEW`. Android 10+ blocks activity starts from
  the background, so post a notification with **Open** instead when the app isn't on screen.

**PC.**
- Incoming `url` (`server.rs`, control loop): only `http`/`https`, then `xdg-open` /
  `ShellExecuteW` / `open`. Opening isn't silent: show a notification "Opened <host> from
  <phone>" so a stray link is noticed.
- Sending: tray **Send link…** (opens with the clipboard's URL filled in), the window's phone
  menu, and `dotlink open <url> [--to <phone>]` through the control socket (`ipc.rs`).
- Linked PCs (`kind: pc`) take `url` too.

**Done when.** Share → Dotlink on a link from Chrome opens it on each desktop; `dotlink open`
opens it on the phone, from the foreground and from a notification.

## 2. Ring the phone

**What.** **Ring** on a phone's page (and `dotlink ring [<phone>]`) makes the phone ring at full
volume, even on silent, until it's stopped on the phone.

**Protocol.**

| Direction | `t` | Meaning |
|---|---|---|
| PC → phone | `ring` | `on: bool`: start or stop ringing |

**Phone.**
- `MediaPlayer` with the default ringtone (`RingtoneManager.TYPE_RINGTONE`, alarm as fallback),
  `AudioAttributes.USAGE_ALARM` so silent and vibrate modes don't mute it, looping; `STREAM_ALARM`
  raised to max and put back after.
- A full-screen notification "Ringing from <PC>" with **Stop** (`USE_FULL_SCREEN_INTENT`; on
  Android 14+ it may fall back to a heads-up notification). Stops after 60 s by itself.

**PC.** Button on each connected phone's card; while ringing it turns into **Stop**. CLI through
`ipc.rs`. Only for `kind: phone`.

**Done when.** The phone rings from silent mode with the screen off, and Stop works from both
ends.

## 3. Phone notifications on the PC, OTP auto-copy

**What.** The phone's notifications show on the PC. A verification code in one is copied to the
PC's clipboard by itself. Dismissing on the PC dismisses on the phone.

**Phone.**
- `NotificationListenerService` (`BIND_NOTIFICATION_LISTENER_SERVICE`), turned on from Settings
  → **Show notifications on PC**, which opens the system's Notification access screen.
- Forward `onNotificationPosted` while linked: app name, app icon (small PNG, cached on the PC
  by package), title, text, key, posted time. Skip ongoing, group-summary and Dotlink's own
  notifications, and any app the user turned off (a per-app list in Settings, chat apps such as
  WhatsApp off by default since they repeat every message).
- Forward `onNotificationRemoved` so the PC drops it too; an incoming `notif_dismiss` calls
  `cancelNotification(key)`.

**Android 15+.** The system hides the text of notifications with codes or private messages from
listeners ("Sensitive notification content hidden"). It shows again after
`adb shell appops set com.dotlink RECEIVE_SENSITIVE_NOTIFICATIONS allow`, which persists after
USB debugging is turned off. Dotlink already runs adb for Cast, so this becomes a **Setup → Fix**
item ("Show verification codes"), run while the phone is connected over USB or Wireless
debugging. Velquo only documents it as manual steps.

**Protocol.**

| Direction | `t` | Meaning |
|---|---|---|
| phone → PC | `notif` | `key`, `app`, `pkg`, `title`, `text`, `time`, `icon` (base64 PNG, only the first time per `pkg`) |
| phone → PC | `notif_removed` | `key` |
| PC → phone | `notif_dismiss` | `key` |

**PC.**
- Show through the existing notification path (`platform::Notify`): notify-send, toasts,
  UNUserNotificationCenter. Title "<app> · <phone>", with **Dismiss** as an action.
- OTP: when title or text has a code (4–8 digits, or `G-123456`-style, next to words like
  "code", "OTP", "verification", "kode", "verifikasi"), copy it to the clipboard and say so in
  the notification ("Code 123456 copied"). Not sent back to the phone as `clip`. Setting:
  **Copy verification codes** (on by default).
- Settings: **Show phone notifications** per phone.

**Privacy.** Notification text only travels over the paired TLS link and is never written to
disk; the icon cache holds only icons.

**Done when.** A WhatsApp/SMS/bank notification shows on each desktop within a second; an SMS
OTP lands on the PC's clipboard on Android 14 and, after Setup's Fix, on Android 15+.

## 4. Media control

**What.** The phone controls what plays on the PC (title, artist, art, play/pause, next,
previous, seek, volume) and the PC controls what plays on the phone.

**Today.** The Remote sends media keys to the PC: blind, no "now playing".

**Phone → controls the PC.**
- PC side reads and drives the active player: MPRIS over D-Bus (Linux, `zbus`), Global System
  Media Transport Controls (Windows, `windows` crate), MediaRemote's private framework
  (macOS; Velquo does this with an Objective-C bridge, `velquo_mr_bridge.m`). New
  `platform::media` with `now_playing()` and `command()`.
- A **Media** tab or card on the phone showing what the PC plays.

**PC → controls the phone.**
- `MediaSessionManager.getActiveSessions(listener component)`, so it needs the notification
  access from 3. Send the active session's metadata and state; take commands back through
  `MediaController.transportControls`.
- On the PC: a "Now playing on <phone>" card in the window and the tray.

**Protocol.**

| Direction | `t` | Meaning |
|---|---|---|
| phone ↔ PC | `media_state` | `player`, `title`, `artist`, `album`, `playing`, `position`, `duration`, `art` (base64 JPEG, only when it changes) |
| phone ↔ PC | `media_cmd` | `cmd`: `play`, `pause`, `toggle`, `next`, `prev`, `seek` (+ `ms`), `volume` (+ `level`) |

(`media` is taken: `state: needs_tap` for camera/mic.)

**Done when.** Spotify/YouTube in a browser on each desktop shows and is controlled from the
phone, and the phone's player from the PC.

## 5. Pause the PC's media on calls

**What.** The PC pauses what it plays when the phone rings or a call starts, and resumes when
the call ends, only if it paused it.

**Phone.** `TelephonyCallback.CallStateListener` (Android 12+; `PhoneStateListener` before),
needs `READ_PHONE_STATE`, asked when the setting is turned on. Sends the state while linked.

**Protocol.**

| Direction | `t` | Meaning |
|---|---|---|
| phone → PC | `call` | `state`: `ringing`, `offhook`, `idle` |

**PC.** On `ringing`/`offhook`: if `platform::media` reports playing, pause and remember. On
`idle`: resume what it paused. Setting: **Pause media during calls** per phone. Without 4,
a fallback sends the play/pause media key, but it can't tell whether anything was playing.

**Done when.** A call pauses music on each desktop and resumes it after hang-up; a call while
nothing plays leaves it that way.

## 6. The laptop's battery on the phone

**What.** The phone shows the PC's battery level and charging state, only when the PC has a
battery (a laptop). A desktop PC shows nothing.

**Today.** Half done: linked PCs already send each other `battery` (`platform::battery()`,
read once a minute by `App::watch_battery`; `None` without a battery, so no message). It's
filtered to `DeviceKind::Pc` in two places in `server.rs`: the first message after `hello` in
the control loop, and `watch_battery`'s list of targets.

**PC.** Drop the `kind == Pc` filter in both places, so phones get the same `battery` message.
Nothing new in the protocol: PC → phone `battery` with `level`, `charging`.

**Phone.**
- `Link.kt`: handle incoming `battery`, keep it in the link state (cleared on disconnect, as a
  PC without a battery never sends one).
- Home (`LinkScreen.kt`): next to the PC's name, a battery icon with the level and a bolt
  while charging. Nothing shown when no `battery` arrived.
- Optional: a notification when the laptop drops to 20% and isn't charging (Settings, off by
  default).

**Done when.** A laptop on each OS shows its level on the phone and updates within a minute of
plugging in or out; a desktop PC shows nothing.

## 7. Presenter mode in the Remote

**What.** A mode in the Remote for slides: big Next / Previous, Start / End, Black screen, a
timer, and the volume keys as Next / Previous.

**PC.** Nothing new: it sends keys through `/ws/input` like the keyboard does.

**Phone.**
- Remote gets a mode switch: Touchpad (today) / Presenter.
- Presenter screen: two large areas, Previous (left third) and Next (the rest), so it works
  without looking; a row with Start, End (`Esc`), Black (`B`) and a timer (tap to start,
  long-press to reset; optional vibration at a set minute).
- Keys: Next `→` / `Page Down`, Previous `←` / `Page Up`. Start depends on the PC's OS (from
  `hello`): `F5` (PowerPoint, LibreOffice, Google Slides with `Ctrl+F5` for "from current"),
  `⌥⌘P` for Keynote on macOS. A small "Start from beginning / from current slide" choice.
- Volume up / down as Next / Previous while the screen is open (`onKeyDown`, consumed so the
  volume doesn't change). Keep the screen on while in Presenter mode.
- Later: a laser pointer (the gyro moving a highlight over the slides) needs an overlay window
  on the PC; not part of the first version.

**Done when.** PowerPoint (Windows), Keynote (macOS), LibreOffice Impress (Linux) and Google
Slides in a browser go forward, back, start and end from the phone with the screen on and with
the volume keys.

## 8. Browse the phone's files from the PC

**What.** A **Files** tab for a connected phone in the PC's window: folders and files of the
phone's storage, download to the PC, upload into the open folder, delete and rename.

**Why not SFTP.** KDE Connect runs an SSH server on the phone and mounts it with SFTP. Dotlink
already has an authenticated TLS link and file transfer both ways, so browsing is a few
request/response messages on top; no SSH server, no `sshfs` / macFUSE / WinFsp on the PC.

**Storage access on the phone.** Two options:
- **All files access** (`MANAGE_EXTERNAL_STORAGE`, Android 11+; Android 10 uses
  `READ/WRITE_EXTERNAL_STORAGE` with `requestLegacyExternalStorage`). Whole storage as plain
  `java.io.File` paths: simplest and fastest. Google Play restricts it to file managers and
  similar apps, but Dotlink ships through GitHub releases, so it's fine.
- **Storage Access Framework**: the user picks folders (`ACTION_OPEN_DOCUMENT_TREE`) and only
  those are visible. Works without the special permission, slower (`DocumentFile`).

Recommended: All files access, asked when the user first opens Files on the PC (the phone
shows a notification leading to the system switch), with **Browse from PC** off by default in
the phone's Settings. `Android/data` and `Android/obb` stay hidden by Android either way.

**Protocol.**

| Direction | `t` | Meaning |
|---|---|---|
| PC → phone | `fs_list` | `req`, `path`: list a folder (`/` is internal storage) |
| phone → PC | `fs_list` | `req`, `entries`: `name`, `dir`, `size`, `mtime`; or `error` |
| PC → phone | `fs_get` | `req`, `path`: send this file; the phone uploads it through `POST /upload` with `req` so the PC saves it where asked instead of `~/Downloads/Dotlink` |
| PC → phone | `fs_put` | `req`, `path`, then the file goes the usual way (`file` offer + `/download/{id}`), saved into `path` instead of `Download/Dotlink` |
| PC → phone | `fs_rm`, `fs_mv`, `fs_mkdir` | `req`, `path` (+ `to`) |
| phone → PC | `fs_ok` | `req`, or `error` |

Paths are checked on the phone: canonicalized and kept under the storage root, so `..` can't
leave it. Only for `kind: pc` devices that are paired; refused while **Browse from PC** is off.

**PC.**
- Window: Files tab per connected phone: path bar, list with name / size / date, folders
  first; double-click a folder to open, a file to download and open; drop files on the list to
  upload into that folder; right-click for Download / Rename / Delete.
- Thumbnails for images (later: `fs_thumb` returning a small JPEG).
- CLI: `dotlink ls [<phone>] <path>`, `dotlink get <phone>:<path> [<dest>]`.

**Later.** Mount it as a drive (Finder / Explorer / Nautilus): a local WebDAV server on
`127.0.0.1` in the PC app that forwards to these messages. All three desktops can mount WebDAV
without extra drivers (Windows' WebClient has quirks with large files).

**Done when.** On each desktop: browse internal storage, download a 1 GB video (SHA-256
checked), upload a folder's files into `DCIM`, rename and delete; Browse from PC off makes
every request fail.

## Docs to update when each lands

- README's feature table.
- [ARCHITECTURE.md](ARCHITECTURE.md): control messages and "Features, briefly".
- [PLATFORM_FEATURES.md](PLATFORM_FEATURES.md): a section per feature, per OS.
