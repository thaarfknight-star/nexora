//! In-memory notification center (v0.1.0).
//!
//! The store lives for the process lifetime only (no persistence).
//! Producers are Nexora's own events only: "update found" (updater) and
//! "taskbar restored" (watchdog). Listening to Windows system toasts
//! (WinRT `UserNotificationListener` + permission) is explicitly out of
//! scope for v0.1.0.
//!
//! The `notifications-changed` event carries the full list; the frontend
//! replaces its state with it (it re-reads through `get_notifications`,
//! so the payload shape is not load-bearing). The emitter is wired once
//! from the `setup` hook (`set_emitter`); without it `add` still stores,
//! it just cannot emit (unit tests run that way).
//!
//! One pending event is one notification: adding a notification whose
//! kind and title already exist refreshes its body and timestamp in
//! place instead of stacking a duplicate (a re-check that finds the same
//! update does not spam the bell).
//!
//! The id is a string: the merged frontend types it as `id: string`
//! (`NotificationItem`) and passes it straight to `dismiss_notification`,
//! so the backend speaks strings end to end.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

/// One notification.
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub timestamp: u64,
}

/// Event emitted whenever the list changes; payload is the full list.
pub const CHANGED_EVENT: &str = "notifications-changed";

/// Kind for an available update.
pub const KIND_UPDATE: &str = "update";
/// Kind for a restored taskbar.
pub const KIND_TASKBAR: &str = "taskbar";

/// Maximum stored notifications; the oldest are evicted first.
const MAX_NOTIFICATIONS: usize = 50;

static NOTIFICATIONS: Mutex<Vec<Notification>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static EMITTER: OnceLock<AppHandle> = OnceLock::new();

/// Wire the emitter once, from the Tauri `setup` hook.
pub fn set_emitter(app: &AppHandle) {
    let _ = EMITTER.set(app.clone());
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn emit_changed() {
    if let Some(app) = EMITTER.get() {
        let _ = app.emit(CHANGED_EVENT, list());
    }
}

/// All notifications, oldest first.
pub fn list() -> Vec<Notification> {
    NOTIFICATIONS.lock().map(|g| g.clone()).unwrap_or_default()
}

/// Add a notification (or refresh an existing one with the same kind and
/// title) and emit `notifications-changed`. Returns the stored item, or
/// `None` only if the store lock is poisoned.
pub fn add(title: &str, body: &str, kind: &str) -> Option<Notification> {
    let notification = {
        let mut guard = NOTIFICATIONS.lock().ok()?;
        if let Some(existing) = guard
            .iter_mut()
            .find(|n| n.kind == kind && n.title == title)
        {
            existing.body = body.to_string();
            existing.timestamp = now_unix_secs();
            existing.clone()
        } else {
            let notification = Notification {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string(),
                title: title.to_string(),
                body: body.to_string(),
                kind: kind.to_string(),
                timestamp: now_unix_secs(),
            };
            guard.push(notification.clone());
            while guard.len() > MAX_NOTIFICATIONS {
                guard.remove(0);
            }
            notification
        }
    };
    emit_changed();
    Some(notification)
}

/// Remove all notifications and emit `notifications-changed`.
pub fn clear() {
    if let Ok(mut guard) = NOTIFICATIONS.lock() {
        guard.clear();
    }
    emit_changed();
}

/// Remove one notification by id. Returns `true` if one was removed.
pub fn dismiss(id: &str) -> bool {
    let removed = match NOTIFICATIONS.lock() {
        Ok(mut guard) => {
            let before = guard.len();
            guard.retain(|n| n.id != id);
            guard.len() != before
        }
        Err(_) => false,
    };
    if removed {
        emit_changed();
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test function: the store is process-global, so parallel test
    // functions in this module would see each other's items.
    #[test]
    fn store_add_upsert_dismiss_cap_clear() {
        let kind = "test-store";
        let first = add("T1", "B1", kind).expect("add stores");
        assert!(first.timestamp > 0);
        // Upsert: same kind+title refreshes in place, same id, no duplicate.
        let refreshed = add("T1", "B2", kind).expect("upsert returns the item");
        assert_eq!(refreshed.id, first.id);
        assert_eq!(refreshed.body, "B2");
        assert_eq!(list().iter().filter(|n| n.kind == kind).count(), 1);
        // Dismiss removes it; a second dismiss finds nothing.
        assert!(dismiss(&first.id));
        assert!(!dismiss(&first.id));
        assert_eq!(list().iter().filter(|n| n.kind == kind).count(), 0);
        // Cap: 55 distinct titles, only the newest 50 remain.
        for i in 0..55 {
            add(&format!("cap-{i}"), "B", kind);
        }
        let items: Vec<_> = list().into_iter().filter(|n| n.kind == kind).collect();
        assert_eq!(items.len(), 50);
        assert_eq!(items.first().unwrap().title, "cap-5");
        assert_eq!(items.last().unwrap().title, "cap-54");
        // Clear empties the store.
        clear();
        assert!(list().is_empty());
    }
}
