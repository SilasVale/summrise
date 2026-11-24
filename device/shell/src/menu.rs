//! The desktop chrome's menu rows, as data.
//!
//! The legacy host built its menu from a wasm table; this shell draws the same rows in the tray.
//! Nothing here knows about `muda`, a window, or an event loop, so the row contract — ids, labels,
//! chords and the one action each row performs — is tested in CI's Linux job where no tray exists.

/// What a row does. One variant per console surface this shell can reach; the legacy ids with no
/// console surface (close/next/prev session, toggle trajectory, export, undo) are deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    NewTerminal,
    NewSsh,
    NewSerial,
    NewBrowser,
    CloseWindow,
    Reload,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleDevTools,
}

/// One menu row: the stable id (also the tray item's id), the label and chord the operator sees, and
/// the action an event with that id performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: &'static str,
    pub label: &'static str,
    pub accelerator: Option<&'static str>,
    pub action: Action,
}

/// A named group of rows. Sections render flat in the tray, separated, never as submenus.
pub struct Section {
    pub label: &'static str,
    pub rows: &'static [Row],
}

/// The rows in display order: the legacy Session ids and chords, then the window roles.
static SECTIONS: [Section; 2] = [
    Section {
        label: "Session",
        rows: &[
            Row {
                id: "new-pty",
                label: "New Terminal",
                accelerator: Some("CmdOrCtrl+Shift+T"),
                action: Action::NewTerminal,
            },
            Row {
                id: "new-ssh",
                label: "New SSH Connection\u{2026}",
                accelerator: Some("CmdOrCtrl+Shift+S"),
                action: Action::NewSsh,
            },
            Row {
                id: "new-serial",
                label: "New Serial Connection\u{2026}",
                accelerator: Some("CmdOrCtrl+Shift+P"),
                action: Action::NewSerial,
            },
            Row {
                id: "new-browser",
                label: "New Browser Session\u{2026}",
                accelerator: Some("CmdOrCtrl+Shift+B"),
                action: Action::NewBrowser,
            },
        ],
    },
    Section {
        label: "Window",
        rows: &[
            Row {
                id: "close-window",
                label: "Close Window",
                accelerator: Some("Ctrl+W"),
                action: Action::CloseWindow,
            },
            Row {
                id: "reload",
                label: "Reload Window",
                accelerator: Some("Ctrl+R"),
                action: Action::Reload,
            },
            Row {
                id: "zoom-in",
                label: "Zoom In",
                accelerator: Some("CmdOrCtrl+="),
                action: Action::ZoomIn,
            },
            Row {
                id: "zoom-out",
                label: "Zoom Out",
                accelerator: Some("CmdOrCtrl+-"),
                action: Action::ZoomOut,
            },
            Row {
                id: "zoom-reset",
                label: "Reset Zoom to 100%",
                accelerator: Some("CmdOrCtrl+0"),
                action: Action::ZoomReset,
            },
            Row {
                id: "toggle-devtools",
                label: "Open Developer Tools",
                accelerator: Some("CmdOrCtrl+Shift+I"),
                action: Action::ToggleDevTools,
            },
        ],
    },
];

/// Every row, in display order.
pub fn sections() -> &'static [Section] {
    &SECTIONS
}

/// The row with `id`, or none when the id has no surface in this shell.
pub fn row(id: &str) -> Option<&'static Row> {
    sections()
        .iter()
        .flat_map(|section| section.rows)
        .find(|candidate| candidate.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_session_section_is_the_legacy_table() {
        let rows = sections()[0].rows;
        assert_eq!(sections()[0].label, "Session");
        assert_eq!(rows.len(), 4);
        assert_eq!(
            (rows[0].id, rows[0].label, rows[0].accelerator),
            ("new-pty", "New Terminal", Some("CmdOrCtrl+Shift+T"))
        );
        assert_eq!(
            (rows[1].id, rows[1].accelerator),
            ("new-ssh", Some("CmdOrCtrl+Shift+S"))
        );
        assert_eq!(
            (rows[2].id, rows[2].accelerator),
            ("new-serial", Some("CmdOrCtrl+Shift+P"))
        );
        assert_eq!(
            (rows[3].id, rows[3].accelerator),
            ("new-browser", Some("CmdOrCtrl+Shift+B"))
        );
        assert_eq!(rows[1].label, "New SSH Connection\u{2026}");
        assert_eq!(rows[3].label, "New Browser Session\u{2026}");
        assert_eq!(
            rows.iter().map(|r| r.action).collect::<Vec<_>>(),
            vec![
                Action::NewTerminal,
                Action::NewSsh,
                Action::NewSerial,
                Action::NewBrowser
            ]
        );
    }

    #[test]
    fn the_window_section_is_the_legacy_roles() {
        let section = &sections()[1];
        assert_eq!(section.label, "Window");
        let rows = section.rows;
        assert_eq!(rows.len(), 6);
        assert_eq!(
            rows.iter()
                .map(|r| (r.id, r.label, r.accelerator, r.action))
                .collect::<Vec<_>>(),
            vec![
                (
                    "close-window",
                    "Close Window",
                    Some("Ctrl+W"),
                    Action::CloseWindow
                ),
                ("reload", "Reload Window", Some("Ctrl+R"), Action::Reload),
                ("zoom-in", "Zoom In", Some("CmdOrCtrl+="), Action::ZoomIn),
                ("zoom-out", "Zoom Out", Some("CmdOrCtrl+-"), Action::ZoomOut),
                (
                    "zoom-reset",
                    "Reset Zoom to 100%",
                    Some("CmdOrCtrl+0"),
                    Action::ZoomReset
                ),
                (
                    "toggle-devtools",
                    "Open Developer Tools",
                    Some("CmdOrCtrl+Shift+I"),
                    Action::ToggleDevTools
                ),
            ]
        );
        assert_eq!(rows[0].label, "Close Window");
    }

    #[test]
    fn every_id_and_action_is_unique_and_the_legacy_dead_rows_are_absent() {
        let ids: Vec<_> = sections()
            .iter()
            .flat_map(|s| s.rows)
            .map(|r| r.id)
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "every row id is used once");
        let actions: HashSet<_> = sections()
            .iter()
            .flat_map(|s| s.rows)
            .map(|r| r.action)
            .collect();
        assert_eq!(actions.len(), ids.len(), "each action names one row");
        for r in sections().iter().flat_map(|s| s.rows) {
            assert!(row(r.id).is_some());
        }
        for gone in [
            "close-session",
            "toggle-trajectory",
            "next-session",
            "prev-session",
            "export-session",
            "undo",
        ] {
            assert!(row(gone).is_none(), "{gone} has no console surface in B1a");
        }
    }
}
