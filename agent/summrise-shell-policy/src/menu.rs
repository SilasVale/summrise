//! THE NATIVE APPLICATION MENU — its labels, accelerators and command ids.
//!
//! `Menu.buildFromTemplate` and the `role` strings are Electron's and stay in `main.ts`; what moves here
//! is the TABLE, because the table is the contract. Every row that is not a role sends a `summrise-menu`
//! command to the SPA, and the SPA dispatches those ids from its own listener — so a renamed id, a
//! dropped row or a changed accelerator breaks the desktop app and nothing else, in the same way the
//! bridge member names do. The accelerator strings are the second half of the same contract: the SPA
//! handles the same chords from its own keydown map when it runs in a plain browser.
//!
//! THE SHAPE IS JSON, and the reason is the generated `.d.ts`: a `wasm-bindgen` `JsValue` return
//! declares `any`, while a `string` this crate built from a typed table declares `string` — and the
//! parse at the one call site is also where a malformed table would be refused rather than half-applied.
//!
//! TWO ROWS ARE NOT COMMANDS, and the table says which: `Reload` calls a host method
//! (`win.webContents.reload()`) and the File menu's last row is the platform's quit/close ROLE. A port
//! that flattened all three into "command" would have invented a channel the SPA never registered.

use serde_json::{json, Value};

/// What a menu row DOES when it is clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// A `type: "separator"` row.
    Separator,
    /// A row that sends this command id to the SPA over `summrise-menu`.
    Command(&'static str),
    /// An Electron `role`, which the host passes through unchanged.
    Role(&'static str),
    /// Reload the main window — a host method, not a command and not a role.
    Reload,
}

/// One row of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem {
    /// `None` for a separator, for a role Electron labels itself (`about`, `hide`, `undo`, …), and for
    /// the mac app menu's `quit`.
    pub label: Option<&'static str>,
    pub accelerator: Option<&'static str>,
    pub action: MenuAction,
}

/// One top-level menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSection {
    pub label: String,
    pub items: Vec<MenuItem>,
}

const fn command(label: &'static str, accelerator: &'static str, id: &'static str) -> MenuItem {
    MenuItem {
        label: Some(label),
        accelerator: Some(accelerator),
        action: MenuAction::Command(id),
    }
}

const fn role(label: Option<&'static str>, role: &'static str) -> MenuItem {
    MenuItem {
        label,
        accelerator: None,
        action: MenuAction::Role(role),
    }
}

const SEPARATOR: MenuItem = MenuItem {
    label: None,
    accelerator: None,
    action: MenuAction::Separator,
};

/// `buildMenu()` — the whole table.
///
/// `platform` is `process.platform` and `app_name` is `app.name`; both are host FACTS, and the two
/// branches they drive are decisions (`darwin` gets the application menu, and its File menu closes the
/// window where the others quit the app).
pub fn app_menu(platform: &str, app_name: &str) -> Vec<MenuSection> {
    let is_mac = platform == "darwin";
    let mut sections: Vec<MenuSection> = Vec::with_capacity(5);

    if is_mac {
        sections.push(MenuSection {
            label: app_name.to_string(),
            items: vec![
                role(None, "about"),
                SEPARATOR,
                role(None, "hide"),
                role(None, "hideOthers"),
                role(None, "unhide"),
                SEPARATOR,
                role(None, "quit"),
            ],
        });
    }

    sections.push(MenuSection {
        label: "File".to_string(),
        items: vec![
            command("New Terminal", "CmdOrCtrl+Shift+T", "new-pty"),
            command("New SSH Connection\u{2026}", "CmdOrCtrl+Shift+S", "new-ssh"),
            command(
                "New Serial Connection\u{2026}",
                "CmdOrCtrl+Shift+P",
                "new-serial",
            ),
            command(
                "New Browser Session\u{2026}",
                "CmdOrCtrl+Shift+B",
                "new-browser",
            ),
            SEPARATOR,
            command("Close Session", "CmdOrCtrl+W", "close-session"),
            SEPARATOR,
            if is_mac {
                role(None, "close")
            } else {
                role(Some("Exit"), "quit")
            },
        ],
    });

    sections.push(MenuSection {
        label: "Edit".to_string(),
        items: vec![
            role(None, "undo"),
            role(None, "redo"),
            SEPARATOR,
            role(None, "cut"),
            role(None, "copy"),
            role(None, "paste"),
            role(None, "selectAll"),
        ],
    });

    sections.push(MenuSection {
        label: "View".to_string(),
        items: vec![
            command(
                "Toggle Trajectory",
                "CmdOrCtrl+Shift+Y",
                "toggle-trajectory",
            ),
            SEPARATOR,
            MenuItem {
                label: Some("Reload"),
                accelerator: Some("CmdOrCtrl+R"),
                action: MenuAction::Reload,
            },
            role(None, "togglefullscreen"),
            role(None, "resetZoom"),
            role(None, "zoomIn"),
            role(None, "zoomOut"),
            role(None, "toggleDevTools"),
        ],
    });

    // TWO ROWS LEFT THIS MENU BEFORE THE PORT AND THE ABSENCE IS THE DECISION, recorded where `main.ts`
    // recorded it rather than silently dropped in the move: "List Sessions" dispatched `list-sessions`,
    // a command with NO SPA-side handler, and "Export Session Log…" duplicated the per-tab export mark
    // that IS the action. The accelerator the second one carried is still handled by the SPA's own
    // keydown map in a plain browser — so it is a row that must NOT come back here.
    sections.push(MenuSection {
        label: "Session".to_string(),
        items: vec![
            command("Next Session", "Ctrl+Tab", "next-session"),
            command("Previous Session", "Ctrl+Shift+Tab", "prev-session"),
        ],
    });

    sections
}

/// The table as JSON — `[{ label, items: [{ label?, accelerator?, role? | command? | reload? |
/// separator? }] }]`, which is exactly what `main.ts` maps onto Electron's
/// `MenuItemConstructorOptions`.
///
/// `skip_serializing_if` is not used: an absent key and a `null` are the same test at the call site
/// (`if (item.role)`), and a fixed key set is what lets the JSON be compared as a whole in the tests.
pub fn app_menu_json(platform: &str, app_name: &str) -> String {
    let sections: Vec<Value> = app_menu(platform, app_name)
        .into_iter()
        .map(|section| {
            let items: Vec<Value> = section
                .items
                .iter()
                .map(|item| {
                    let mut row = json!({ "label": item.label, "accelerator": item.accelerator });
                    match item.action {
                        MenuAction::Separator => row["separator"] = json!(true),
                        MenuAction::Command(id) => row["command"] = json!(id),
                        MenuAction::Role(name) => row["role"] = json!(name),
                        MenuAction::Reload => row["reload"] = json!(true),
                    }
                    row
                })
                .collect();
            json!({ "label": section.label, "items": items })
        })
        .collect();
    Value::Array(sections).to_string()
}
