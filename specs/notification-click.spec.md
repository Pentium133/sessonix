---
name: Notification Click Navigation (backend)
description: macOS session notifications that carry their session id and report clicks back to the frontend
targets:
  - ../src-tauri/src/notifications.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/Cargo.toml
---

# Notification Click Navigation (backend)

Clicking a session notification must open the session that sent it. Today it does not: on desktop `tauri-plugin-notification` shows notifications via `notify-rust` fire-and-forget, drops `extra`, and never fires `onAction` (that event exists only on mobile). The click just activates the app and leaves whatever session was active.

`mac-notification-sys`'s `wait_for_click` is not usable: every send replaces the single `NSUserNotificationCenter` delegate, so with two pending notifications the older one's waiting thread hangs and its click is reported to the newer one's delegate — wrong session again.

## Provided interface

```ts
// command: send_session_notification
invoke("send_session_notification", { title: string, body: string, sessionId: number }): Promise<void>
// event: notification-clicked { sessionId: number } — emitted when the user clicks a notification sent by the command
```

## Scope

In scope:
- New module `notifications.rs` with a macOS implementation and a non-macOS fallback.
- Registering the command and installing the delegate at app setup.

Out of scope:
- Deciding *whether* to notify (focus / active-project suppression, settings toggles) — stays in the frontend.
- Action buttons, replies, sounds.
- Click navigation on Windows/Linux.
- Replacing `tauri-plugin-notification` for permission handling and the one-time welcome/registration notification.

## Requirements

### macOS delivery

- One `NSUserNotificationCenterDelegate` instance is created on the main thread and kept alive for the app's lifetime. It is attached to `NSUserNotificationCenter::defaultUserNotificationCenter` during app setup and re-attached (same instance) before every delivery, because the plugin's `mac-notification-sys` swaps in its own delegate when it shows a notification.
- `send_session_notification` delivers an `NSUserNotification` with `title`, `informativeText = body`, and `userInfo = { "sessionId": "<id>" }`. Delivery happens on the main thread and the command returns without waiting for user interaction.
- The delegate's `shouldPresentNotification` returns YES, so notifications show while the window is focused (the frontend sends those for sessions in other projects).
- When `defaultUserNotificationCenter` is `nil` (unbundled dev binary without a bundle identifier) the command falls back to `tauri-plugin-notification` with no click navigation. Click navigation is verified on a bundled build.

### Click handling

- On `didActivateNotification` with activation type "contents clicked" the delegate reads `userInfo["sessionId"]`, parses it as `u32`, shows and focuses the main window, and emits `notification-clicked { sessionId }`.
  `[@test] ../src-tauri/src/notifications.rs`
- A missing or non-numeric `sessionId` emits nothing.
  `[@test] ../src-tauri/src/notifications.rs`
- The clicked notification is removed from Notification Center.
- Each notification resolves to its own session id regardless of how many are pending (id lives on the notification, not on the delegate).

### Non-macOS fallback

- `send_session_notification` shows the notification through `tauri-plugin-notification` (`title`, `body`) and never emits `notification-clicked`.

### Errors

- The command returns `Result<(), String>`; delivery failures are returned as `Err`, never panic.
