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

Suggested order: 1 and 2 first (a day together), then 3 (the most useful), then 4 and 5.

Not planned: browsing the phone's files (SFTP), presenter mode and run-command (Remote and
Shell already cover them).

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

**What.** **Ring** on the Phones page (and `dotlink ring [<phone>]`) makes the phone ring at full
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

## Docs to update when each lands

- README's feature table.
- [ARCHITECTURE.md](ARCHITECTURE.md): control messages and "Features, briefly".
- [PLATFORM_FEATURES.md](PLATFORM_FEATURES.md): a section per feature, per OS.
