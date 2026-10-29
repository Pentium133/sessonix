---
name: Notification Click Navigation (frontend)
description: Send session notifications through the backend command and navigate to the clicked session
targets:
  - ../src/lib/notifications.ts
  - ../src/lib/api.ts
---

# Notification Click Navigation (frontend)

The backend (`specs/notification-click.spec.md`) now sends session notifications itself and reports clicks. This spec moves the frontend off `sendNotification({ extra })` + `onAction`, which never fire on desktop.

## Consumed backend interface

```ts
// command: send_session_notification
invoke("send_session_notification", { title: string, body: string, sessionId: number }): Promise<void>
// event: notification-clicked { sessionId: number }
```

## Scope

In scope:
- `sendSessionNotification` delivery path.
- `setupNotificationClickHandler` listener.
- `api.ts` wrapper `sendSessionNotificationNative(title, body, sessionId)`.

Out of scope:
- Suppression rules (focused window + active project), permission hint toast, settings toggles — unchanged.
- `initNotifications` / welcome notification — still via `tauri-plugin-notification`.
- Callers in `useStatusPolling.ts` and `usePtyOutput.ts` — signature of `sendSessionNotification` unchanged.

## Requirements

### Sending

- When not suppressed and permission is ready, `sendSessionNotification(title, body, workingDir, sessionId)` with a `sessionId` calls `send_session_notification`; rejection is swallowed (logged), never thrown.
  `[@test] ../src/__tests__/notifications.test.ts`
- Without a `sessionId` it falls back to the plugin's `sendNotification({ title, body })`.
  `[@test] ../src/__tests__/notifications.test.ts`

### Click navigation

- `setupNotificationClickHandler` listens to `notification-clicked` (registered once). On an event for an existing session in another project it saves the current project's last active session, sets `activeProjectPath` to the session's `working_dir`, and calls `switchSession(sessionId)`.
  `[@test] ../src/__tests__/notifications.test.ts`
- For a session in the active project it only calls `switchSession(sessionId)`.
  `[@test] ../src/__tests__/notifications.test.ts`
- For an unknown `sessionId` (session deleted) it does nothing.
  `[@test] ../src/__tests__/notifications.test.ts`
- The `onAction` subscription is removed.
