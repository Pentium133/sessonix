//! Session notifications that navigate to their session when clicked.
//!
//! `tauri-plugin-notification` on desktop shows notifications fire-and-forget
//! and never reports clicks. On macOS we deliver `NSUserNotification`s
//! ourselves: each one carries its session id in `userInfo`, and a single
//! persistent delegate turns clicks into `notification-clicked` events.
//! Elsewhere (and in unbundled dev builds) we fall back to the plugin.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

const CLICK_EVENT: &str = "notification-clicked";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationClicked {
    session_id: u32,
}

/// Parse the session id stored in a notification's `userInfo`.
fn parse_session_id(raw: Option<&str>) -> Option<u32> {
    raw?.trim().parse().ok()
}

/// Bring the main window forward and tell the frontend which session to open.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn handle_click(app: &AppHandle, raw_session_id: Option<&str>) {
    let Some(session_id) = parse_session_id(raw_session_id) else {
        return;
    };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit(CLICK_EVENT, NotificationClicked { session_id });
}

/// Install the click delegate. Call once from `setup` (main thread).
pub fn install(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::attach_delegate(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Show a notification for `session_id`. Returns without waiting for interaction.
pub fn send(app: &AppHandle, title: String, body: String, session_id: u32) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            if !macos::deliver(&handle, &title, &body, session_id) {
                if let Err(e) = send_via_plugin(&handle, &title, &body) {
                    log::warn!("Failed to show notification: {}", e);
                }
            }
        })
        .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = session_id;
        send_via_plugin(app, &title, &body)
    }
}

fn send_via_plugin(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // NSUserNotification: UNUserNotificationCenter needs a signed bundle.
mod macos {
    use std::cell::OnceCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{
        ns_string, NSDictionary, NSString, NSUserNotification, NSUserNotificationActivationType,
        NSUserNotificationCenter, NSUserNotificationCenterDelegate,
    };
    use tauri::AppHandle;

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; Delegate does not implement Drop.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[name = "SessonixNotificationDelegate"]
        #[ivars = AppHandle]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl NSUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:didActivateNotification:))]
            fn did_activate(&self, center: &NSUserNotificationCenter, notification: &NSUserNotification) {
                if notification.activationType() != NSUserNotificationActivationType::ContentsClicked {
                    return;
                }
                center.removeDeliveredNotification(notification);
                let raw = notification
                    .userInfo()
                    .and_then(|info| info.objectForKey(ns_string!("sessionId")))
                    .and_then(|value| value.downcast::<NSString>().ok())
                    .map(|s| s.to_string());
                super::handle_click(self.ivars(), raw.as_deref());
            }

            #[unsafe(method(userNotificationCenter:shouldPresentNotification:))]
            fn should_present(&self, _center: &NSUserNotificationCenter, _notification: &NSUserNotification) -> bool {
                true
            }
        }
    );

    thread_local! {
        // The center holds its delegate weakly; keep ours alive for the app's lifetime.
        static DELEGATE: OnceCell<Retained<Delegate>> = const { OnceCell::new() };
    }

    /// `nil` when the main bundle has no identifier (unbundled dev binary).
    fn center() -> Option<Retained<NSUserNotificationCenter>> {
        unsafe { msg_send![NSUserNotificationCenter::class(), defaultUserNotificationCenter] }
    }

    /// Point the center at our delegate. Re-run before every delivery because
    /// the plugin's `mac-notification-sys` swaps in its own delegate when it
    /// shows a notification. Returns the center when available.
    pub fn attach_delegate(app: &AppHandle) -> Option<Retained<NSUserNotificationCenter>> {
        let mtm = MainThreadMarker::new()?;
        let center = center()?;
        DELEGATE.with(|cell| {
            let delegate = cell.get_or_init(|| {
                let this = Delegate::alloc(mtm).set_ivars(app.clone());
                // SAFETY: NSObject's `init` signature is correct.
                unsafe { msg_send![super(this), init] }
            });
            // SAFETY: the delegate is retained in DELEGATE for the app's lifetime.
            unsafe { center.setDelegate(Some(ProtocolObject::from_ref(&**delegate))) };
        });
        Some(center)
    }

    /// Deliver a notification tagged with `session_id`. Returns false when
    /// Notification Center is unavailable and the caller should fall back.
    pub fn deliver(app: &AppHandle, title: &str, body: &str, session_id: u32) -> bool {
        let Some(center) = attach_delegate(app) else {
            return false;
        };
        let notification = NSUserNotification::new();
        notification.setTitle(Some(&NSString::from_str(title)));
        notification.setInformativeText(Some(&NSString::from_str(body)));
        let id = NSString::from_str(&session_id.to_string());
        let user_info = NSDictionary::<NSString, AnyObject>::from_slices(
            &[ns_string!("sessionId")],
            &[&*id as &AnyObject],
        );
        // SAFETY: userInfo holds only property-list types (NSString).
        unsafe { notification.setUserInfo(Some(&user_info)) };
        center.deliverNotification(&notification);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_session_id() {
        assert_eq!(parse_session_id(Some("42")), Some(42));
    }

    #[test]
    fn rejects_missing_or_non_numeric_session_id() {
        assert_eq!(parse_session_id(None), None);
        assert_eq!(parse_session_id(Some("")), None);
        assert_eq!(parse_session_id(Some("abc")), None);
        assert_eq!(parse_session_id(Some("-1")), None);
    }
}
