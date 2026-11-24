//! Configurable shortcuts: every command can have one keyboard shortcut and one mouse button
//! (the side Back / Forward buttons or the middle button). Defaults live here; the user's
//! changes are stored in `Config::shortcuts` and applied with [`apply`].

use gpui::{App, KeyBinding, Keystroke, MouseButton, NavigationDirection};

use super::workspace::{
    AddToQueueSel, DeleteSelected, Dismiss, EditFile, EditPath, GoBack, GoForward, GoUp, NewFolder, OpenSelected, ProcessQueue, Refresh,
    RenameSelected, SelectAllRows, ToggleTransfers,
};
use crate::core::config::Config;
use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Back,
    Forward,
    Up,
    Refresh,
    Open,
    Delete,
    Rename,
    NewFolder,
    Edit,
    EditPath,
    SelectAll,
    AddToQueue,
    ProcessQueue,
    ToggleTransfers,
}

impl Cmd {
    pub const ALL: [Cmd; 14] = [
        Cmd::Back,
        Cmd::Forward,
        Cmd::Up,
        Cmd::Refresh,
        Cmd::Open,
        Cmd::Delete,
        Cmd::Rename,
        Cmd::NewFolder,
        Cmd::Edit,
        Cmd::EditPath,
        Cmd::SelectAll,
        Cmd::AddToQueue,
        Cmd::ProcessQueue,
        Cmd::ToggleTransfers,
    ];

    /// Stable id used in the config file.
    pub fn id(self) -> &'static str {
        match self {
            Cmd::Back => "back",
            Cmd::Forward => "forward",
            Cmd::Up => "parent_folder",
            Cmd::Refresh => "refresh",
            Cmd::Open => "open",
            Cmd::Delete => "delete",
            Cmd::Rename => "rename",
            Cmd::NewFolder => "new_folder",
            Cmd::Edit => "view_edit",
            Cmd::EditPath => "type_path",
            Cmd::SelectAll => "select_all",
            Cmd::AddToQueue => "add_to_queue",
            Cmd::ProcessQueue => "process_queue",
            Cmd::ToggleTransfers => "toggle_transfers",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Cmd::Back => tr("Back (previous folder)"),
            Cmd::Forward => tr("Forward (next folder)"),
            Cmd::Up => tr("Parent folder"),
            Cmd::Refresh => tr("Refresh"),
            Cmd::Open => tr("Open folder / transfer selection"),
            Cmd::Delete => tr("Delete"),
            Cmd::Rename => tr("Rename"),
            Cmd::NewFolder => tr("New folder"),
            Cmd::Edit => tr("View / Edit"),
            Cmd::EditPath => tr("Type a path"),
            Cmd::SelectAll => tr("Select all"),
            Cmd::AddToQueue => tr("Add to queue"),
            Cmd::ProcessQueue => tr("Process queue"),
            Cmd::ToggleTransfers => tr("Show / hide the transfer panel"),
        }
    }

    pub fn default_key(self) -> Option<&'static str> {
        let primary = |mac: &'static str, other: &'static str| if cfg!(target_os = "macos") { mac } else { other };
        Some(match self {
            Cmd::Back => "alt-left",
            Cmd::Forward => "alt-right",
            Cmd::Up => "backspace",
            Cmd::Refresh => "f5",
            Cmd::Open => "enter",
            Cmd::Delete => "delete",
            Cmd::Rename => "f2",
            Cmd::NewFolder => "f7",
            Cmd::Edit => "f4",
            Cmd::EditPath => primary("cmd-l", "ctrl-l"),
            Cmd::SelectAll => primary("cmd-a", "ctrl-a"),
            Cmd::AddToQueue => return None,
            Cmd::ProcessQueue => primary("cmd-p", "ctrl-p"),
            Cmd::ToggleTransfers => primary("cmd-j", "ctrl-j"),
        })
    }

    pub fn default_mouse(self) -> Option<&'static str> {
        match self {
            Cmd::Back => Some("back"),
            Cmd::Forward => Some("forward"),
            _ => None,
        }
    }

    /// Commands that act on the focused file pane; the rest work anywhere in the window.
    fn pane_scoped(self) -> bool {
        !matches!(self, Cmd::Refresh | Cmd::ProcessQueue | Cmd::ToggleTransfers)
    }

    fn binding(self, keys: &str) -> KeyBinding {
        let ctx = if self.pane_scoped() { Some("FilePane") } else { None };
        match self {
            Cmd::Back => KeyBinding::new(keys, GoBack, ctx),
            Cmd::Forward => KeyBinding::new(keys, GoForward, ctx),
            Cmd::Up => KeyBinding::new(keys, GoUp, ctx),
            Cmd::Refresh => KeyBinding::new(keys, Refresh, ctx),
            Cmd::Open => KeyBinding::new(keys, OpenSelected, ctx),
            Cmd::Delete => KeyBinding::new(keys, DeleteSelected, ctx),
            Cmd::Rename => KeyBinding::new(keys, RenameSelected, ctx),
            Cmd::NewFolder => KeyBinding::new(keys, NewFolder, ctx),
            Cmd::Edit => KeyBinding::new(keys, EditFile, ctx),
            Cmd::EditPath => KeyBinding::new(keys, EditPath, ctx),
            Cmd::SelectAll => KeyBinding::new(keys, SelectAllRows, ctx),
            Cmd::AddToQueue => KeyBinding::new(keys, AddToQueueSel, ctx),
            Cmd::ProcessQueue => KeyBinding::new(keys, ProcessQueue, ctx),
            Cmd::ToggleTransfers => KeyBinding::new(keys, ToggleTransfers, ctx),
        }
    }
}

/// The keyboard shortcut in effect for `cmd` (user's choice, else the default).
pub fn key_for(cfg: &Config, cmd: Cmd) -> Option<String> {
    match cfg.shortcuts.keys.get(cmd.id()) {
        Some(k) if k.is_empty() => None,
        Some(k) => Some(k.clone()),
        None => cmd.default_key().map(str::to_string),
    }
}

/// The mouse button in effect for `cmd`: "back", "forward" or "middle".
pub fn mouse_for(cfg: &Config, cmd: Cmd) -> Option<String> {
    match cfg.shortcuts.mouse.get(cmd.id()) {
        Some(m) if m.is_empty() => None,
        Some(m) => Some(m.clone()),
        None => cmd.default_mouse().map(str::to_string),
    }
}

pub fn mouse_id(button: MouseButton) -> Option<&'static str> {
    match button {
        MouseButton::Navigate(NavigationDirection::Back) => Some("back"),
        MouseButton::Navigate(NavigationDirection::Forward) => Some("forward"),
        MouseButton::Middle => Some("middle"),
        _ => None,
    }
}

/// Command bound to a mouse button, if any.
pub fn command_for_mouse(cfg: &Config, button: MouseButton) -> Option<Cmd> {
    let id = mouse_id(button)?;
    Cmd::ALL.into_iter().find(|c| mouse_for(cfg, *c).as_deref() == Some(id))
}

fn valid_keystrokes(keys: &str) -> bool {
    !keys.trim().is_empty() && keys.split_whitespace().all(|k| Keystroke::parse(k).is_ok())
}

/// (Re)install every key binding: text inputs, Escape, and the configurable commands.
pub fn apply(cx: &mut App, cfg: &Config) {
    cx.clear_key_bindings();
    super::input::bind_keys(cx);
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some("Workspace"))]);
    let bindings: Vec<KeyBinding> =
        Cmd::ALL.into_iter().filter_map(|c| key_for(cfg, c).filter(|k| valid_keystrokes(k)).map(|k| c.binding(&k))).collect();
    cx.bind_keys(bindings);
}

/// "ctrl-shift-l" -> "Ctrl+Shift+L"
pub fn display_key(keys: &str) -> String {
    keys.split_whitespace()
        .map(|stroke| {
            let mut parts: Vec<String> = vec![];
            let mut rest = stroke;
            // modifiers come first, joined by '-' ("ctrl-alt-x"); a trailing '-' key is "ctrl--"
            while let Some((head, tail)) = rest.split_once('-') {
                if tail.is_empty() {
                    break;
                }
                parts.push(match head {
                    "ctrl" => "Ctrl".into(),
                    "alt" => "Alt".into(),
                    "shift" => "Shift".into(),
                    "cmd" | "platform" | "super" | "win" => {
                        if cfg!(target_os = "macos") {
                            "Cmd".into()
                        } else {
                            "Win".into()
                        }
                    }
                    "fn" => "Fn".into(),
                    other => other.to_string(),
                });
                rest = tail;
            }
            parts.push(match rest {
                "left" => "←".into(),
                "right" => "→".into(),
                "up" => "↑".into(),
                "down" => "↓".into(),
                "space" => tr("Space").into(),
                k if k.len() == 1 => k.to_uppercase(),
                k if k.starts_with('f') && k[1..].chars().all(|c| c.is_ascii_digit()) => k.to_uppercase(),
                k => {
                    let mut c = k.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                }
            });
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn display_mouse(id: &str) -> &'static str {
    match id {
        "back" => tr("Mouse back button"),
        "forward" => tr("Mouse forward button"),
        "middle" => tr("Middle click"),
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_keys() {
        assert_eq!(display_key("ctrl-shift-l"), "Ctrl+Shift+L");
        assert_eq!(display_key("alt-left"), "Alt+←");
        assert_eq!(display_key("f5"), "F5");
        assert_eq!(display_key("backspace"), "Backspace");
    }

    #[test]
    fn defaults_and_overrides() {
        let mut cfg = Config::default();
        assert_eq!(key_for(&cfg, Cmd::Refresh).as_deref(), Some("f5"));
        assert_eq!(mouse_for(&cfg, Cmd::Back).as_deref(), Some("back"));
        assert_eq!(command_for_mouse(&cfg, MouseButton::Navigate(NavigationDirection::Forward)), Some(Cmd::Forward));
        cfg.shortcuts.keys.insert("refresh".into(), String::new());
        cfg.shortcuts.mouse.insert("parent_folder".into(), "back".into());
        cfg.shortcuts.mouse.insert("back".into(), String::new());
        assert_eq!(key_for(&cfg, Cmd::Refresh), None);
        assert_eq!(command_for_mouse(&cfg, MouseButton::Navigate(NavigationDirection::Back)), Some(Cmd::Up));
        assert!(Cmd::ALL.iter().filter_map(|c| c.default_key()).all(valid_keystrokes));
    }
}
