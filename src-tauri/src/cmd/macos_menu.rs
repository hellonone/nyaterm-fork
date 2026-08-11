#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

const ACTION_PREFIX: &str = "macos::action::";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacosAppMenuSpec {
    menus: Vec<MacosMenu>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MacosMenu {
    id: String,
    label: String,
    items: Vec<MacosMenuItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum MacosMenuItem {
    Item {
        id: String,
        label: String,
        enabled: bool,
        accelerator: Option<String>,
    },
    Check {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
        accelerator: Option<String>,
    },
    Submenu {
        id: String,
        label: String,
        enabled: bool,
        items: Vec<MacosMenuItem>,
    },
    Separator,
    Predefined {
        role: MacosPredefinedRole,
        label: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MacosPredefinedRole {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Maximize,
    Fullscreen,
    CloseWindow,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MacosMenuActionPayload {
    action_id: String,
    target_window_label: Option<String>,
}

#[tauri::command]
pub fn set_macos_app_menu(app: AppHandle, spec: MacosAppMenuSpec) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        build_and_set_menu(&app, spec).map_err(|error| error.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, spec);
        Ok(())
    }
}

pub fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let Some(action_id) = event.id.as_ref().strip_prefix(ACTION_PREFIX) else {
        return;
    };

    let _ = app.emit(
        "macos-menu-action",
        MacosMenuActionPayload {
            action_id: action_id.to_string(),
            target_window_label: crate::app::focused_or_first_main_window(app)
                .map(|window| window.label().to_string()),
        },
    );
}

#[cfg(target_os = "macos")]
fn build_and_set_menu(app: &AppHandle, spec: MacosAppMenuSpec) -> tauri::Result<()> {
    use tauri::menu::{Menu, Submenu};

    let menu = Menu::new(app)?;
    for top_level in spec.menus {
        let submenu = Submenu::with_id(
            app,
            menu_id(&top_level.id),
            escape_menu_text(&top_level.label),
            true,
        )?;
        append_items(app, &submenu, &top_level.items)?;
        menu.append(&submenu)?;
    }

    app.set_menu(menu)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn append_items(
    app: &AppHandle,
    parent: &tauri::menu::Submenu<tauri::Wry>,
    items: &[MacosMenuItem],
) -> tauri::Result<()> {
    use tauri::menu::{CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};

    for item in items {
        match item {
            MacosMenuItem::Item {
                id,
                label,
                enabled,
                accelerator,
            } => {
                let item = MenuItem::with_id(
                    app,
                    action_menu_id(id),
                    escape_menu_text(label),
                    *enabled,
                    accelerator.as_deref(),
                )?;
                parent.append(&item)?;
            }
            MacosMenuItem::Check {
                id,
                label,
                enabled,
                checked,
                accelerator,
            } => {
                let item = CheckMenuItem::with_id(
                    app,
                    action_menu_id(id),
                    escape_menu_text(label),
                    *enabled,
                    *checked,
                    accelerator.as_deref(),
                )?;
                parent.append(&item)?;
            }
            MacosMenuItem::Submenu {
                id,
                label,
                enabled,
                items,
            } => {
                let submenu =
                    Submenu::with_id(app, menu_id(id), escape_menu_text(label), *enabled)?;
                append_items(app, &submenu, items)?;
                parent.append(&submenu)?;
            }
            MacosMenuItem::Separator => {
                parent.append(&PredefinedMenuItem::separator(app)?)?;
            }
            MacosMenuItem::Predefined { role, label } => {
                let label = label.as_deref();
                let item = match role {
                    MacosPredefinedRole::Undo => PredefinedMenuItem::undo(app, label)?,
                    MacosPredefinedRole::Redo => PredefinedMenuItem::redo(app, label)?,
                    MacosPredefinedRole::Cut => PredefinedMenuItem::cut(app, label)?,
                    MacosPredefinedRole::Copy => PredefinedMenuItem::copy(app, label)?,
                    MacosPredefinedRole::Paste => PredefinedMenuItem::paste(app, label)?,
                    MacosPredefinedRole::SelectAll => PredefinedMenuItem::select_all(app, label)?,
                    MacosPredefinedRole::Services => PredefinedMenuItem::services(app, label)?,
                    MacosPredefinedRole::Hide => PredefinedMenuItem::hide(app, label)?,
                    MacosPredefinedRole::HideOthers => PredefinedMenuItem::hide_others(app, label)?,
                    MacosPredefinedRole::ShowAll => PredefinedMenuItem::show_all(app, label)?,
                    MacosPredefinedRole::Minimize => PredefinedMenuItem::minimize(app, label)?,
                    MacosPredefinedRole::Maximize => PredefinedMenuItem::maximize(app, label)?,
                    MacosPredefinedRole::Fullscreen => PredefinedMenuItem::fullscreen(app, label)?,
                    MacosPredefinedRole::CloseWindow => {
                        PredefinedMenuItem::close_window(app, label)?
                    }
                };
                parent.append(&item)?;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn action_menu_id(id: &str) -> String {
    format!("{ACTION_PREFIX}{id}")
}

#[cfg(target_os = "macos")]
fn menu_id(id: &str) -> String {
    format!("macos::menu::{id}")
}

#[cfg(target_os = "macos")]
fn escape_menu_text(text: &str) -> String {
    text.replace('&', "&&")
}
