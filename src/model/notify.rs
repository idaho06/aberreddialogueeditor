//! Typed notifications queued on the [`Workspace`](crate::model::Workspace) and drained one by
//! one by the GUI's modal (`scenes/editor/modals.rs`). The kind decides the modal's title and
//! body: a save rejected by the engine's gate gets its own "Cannot save" modal that points at
//! the Problems panel, everything else is a plain error.

/// What a [`Notification`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    /// A plain failure (io, json, already-open, ...): shown in the "Error" modal.
    Error,
    /// The engine's save gate rejected the doc: shown in the "Cannot save" modal with the
    /// engine's own message and a pointer to the Problems panel.
    SaveRejected,
}

/// One queued notification for the GUI's modal. The message of a [`NotificationKind::SaveRejected`]
/// is the engine's own error text; other kinds carry the `Display` text of the failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Which modal the GUI should show for this notification.
    pub kind: NotificationKind,
    /// The message text (for [`NotificationKind::SaveRejected`], the engine's own error text).
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_kind_is_copy_and_comparable() {
        let kind = NotificationKind::SaveRejected;
        let copied = kind; // Copy
        assert_eq!(kind, copied);
        assert_ne!(NotificationKind::Error, NotificationKind::SaveRejected);
    }

    #[test]
    fn notification_stores_kind_and_message() {
        let note = Notification {
            kind: NotificationKind::Error,
            message: "io error: cannot read x.json".to_owned(),
        };
        assert_eq!(note.kind, NotificationKind::Error);
        assert_eq!(note.message, "io error: cannot read x.json");
        assert_eq!(
            note.clone(),
            note,
            "Notification is Clone + PartialEq (Workspace derives them)"
        );
    }
}
