use std::collections::HashMap;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::{Action, Direction, ProfileDirection};
use crate::config::KeybindsConfig;

// ─── Key string parser ──────────────────────────────────────────────────────

/// Parse a human-readable key string (e.g. "ctrl+c", "shift+tab", "q") into a
/// crossterm `KeyEvent`.
pub fn parse_key_string(s: &str) -> Result<KeyEvent> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return Err(eyre!("Empty key string"));
    }

    let parts: Vec<&str> = s.split('+').collect();
    let key_part = parts.last().unwrap();
    let modifier_parts = &parts[..parts.len() - 1];

    let mut modifiers = KeyModifiers::NONE;
    for part in modifier_parts {
        match *part {
            "ctrl" => modifiers |= KeyModifiers::CONTROL,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "alt" => modifiers |= KeyModifiers::ALT,
            other => return Err(eyre!("Unknown modifier: '{}'", other)),
        }
    }

    let code = match *key_part {
        "tab" => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                KeyCode::BackTab
            } else {
                KeyCode::Tab
            }
        }
        "escape" | "esc" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        s if s.len() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        other => return Err(eyre!("Unknown key: '{}'", other)),
    };

    Ok(KeyEvent::new(code, modifiers))
}

// ─── InputMode ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Interact,
    Edit,
    HelpOverlay,
}

// ─── BoundAction ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundAction {
    Quit,
    ForceQuit,
    Reload,
    Help,
    FocusNext,
    FocusPrev,
    FocusUp,
    FocusDown,
    FocusLeft,
    FocusRight,
    EnterInteract,
    ExitInteract,
    EditMode,
    ProfileNext,
    ProfilePrev,
}

// ─── KeyDispatcher ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct KeyDispatcher {
    bindings: HashMap<KeyEvent, BoundAction>,
}

/// Default key binding strings for each action.
struct Defaults;

impl Defaults {
    const QUIT: &str = "q";
    const FORCE_QUIT: &str = "ctrl+c";
    const RELOAD: &str = "r";
    const HELP: &str = "?";
    const FOCUS_NEXT: &str = "tab";
    const FOCUS_PREV: &str = "shift+tab";
    const FOCUS_UP: &str = "up";
    const FOCUS_DOWN: &str = "down";
    const FOCUS_LEFT: &str = "left";
    const FOCUS_RIGHT: &str = "right";
    const ENTER_INTERACT: &str = "enter";
    const EXIT_INTERACT: &str = "escape";
    const EDIT_MODE: &str = "e";
    const PROFILE_NEXT: &str = "ctrl+]";
    const PROFILE_PREV: &str = "ctrl+[";
}

impl KeyDispatcher {
    /// Build a new dispatcher from optional user config.
    ///
    /// `ctrl+c` is always bound to `ForceQuit` and cannot be remapped.
    /// Returns an error if two actions map to the same key.
    pub fn new(config: &KeybindsConfig) -> Result<Self> {
        let mut bindings: HashMap<KeyEvent, BoundAction> = HashMap::new();
        // Track which BoundAction owns each KeyEvent for error messages
        let mut reverse: HashMap<KeyEvent, &str> = HashMap::new();

        // Helper: resolve user override or default, parse, insert, check dups
        let mut bind =
            |action: BoundAction, name: &'static str, user: &Option<String>, default: &str| -> Result<()> {
                let key_str = user.as_deref().unwrap_or(default);
                let key_event = parse_key_string(key_str)?;

                // ctrl+c is reserved for ForceQuit
                if action != BoundAction::ForceQuit
                    && key_event
                        == KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
                {
                    return Err(eyre!(
                        "Cannot bind '{}' to ctrl+c (reserved for force quit)",
                        name
                    ));
                }

                if let Some(existing_name) = reverse.get(&key_event) {
                    return Err(eyre!(
                        "Duplicate keybind: '{}' and '{}' both map to '{}'",
                        existing_name,
                        name,
                        key_str
                    ));
                }

                reverse.insert(key_event, name);
                bindings.insert(key_event, action);
                Ok(())
            };

        // Force-quit is always ctrl+c (not remappable via config)
        bind(
            BoundAction::ForceQuit,
            "force_quit",
            &None,
            Defaults::FORCE_QUIT,
        )?;

        bind(BoundAction::Quit, "quit", &config.quit, Defaults::QUIT)?;
        bind(
            BoundAction::Reload,
            "reload",
            &config.reload,
            Defaults::RELOAD,
        )?;
        bind(BoundAction::Help, "help", &config.help, Defaults::HELP)?;
        bind(
            BoundAction::FocusNext,
            "focus_next",
            &config.focus_next,
            Defaults::FOCUS_NEXT,
        )?;
        bind(
            BoundAction::FocusPrev,
            "focus_prev",
            &config.focus_prev,
            Defaults::FOCUS_PREV,
        )?;
        bind(
            BoundAction::FocusUp,
            "focus_up",
            &config.focus_up,
            Defaults::FOCUS_UP,
        )?;
        bind(
            BoundAction::FocusDown,
            "focus_down",
            &config.focus_down,
            Defaults::FOCUS_DOWN,
        )?;
        bind(
            BoundAction::FocusLeft,
            "focus_left",
            &config.focus_left,
            Defaults::FOCUS_LEFT,
        )?;
        bind(
            BoundAction::FocusRight,
            "focus_right",
            &config.focus_right,
            Defaults::FOCUS_RIGHT,
        )?;
        bind(
            BoundAction::EnterInteract,
            "enter_interact",
            &config.enter_interact,
            Defaults::ENTER_INTERACT,
        )?;
        bind(
            BoundAction::ExitInteract,
            "exit_interact",
            &config.exit_interact,
            Defaults::EXIT_INTERACT,
        )?;
        bind(
            BoundAction::EditMode,
            "edit_mode",
            &config.edit_mode,
            Defaults::EDIT_MODE,
        )?;
        bind(
            BoundAction::ProfileNext,
            "profile_next",
            &config.profile_next,
            Defaults::PROFILE_NEXT,
        )?;
        bind(
            BoundAction::ProfilePrev,
            "profile_prev",
            &config.profile_prev,
            Defaults::PROFILE_PREV,
        )?;

        Ok(Self { bindings })
    }

    /// Resolve a key event in the given input mode to an optional `Action`.
    ///
    /// Returns `None` when the key is not handled (e.g. forwarded to widget in
    /// interact mode, or ignored in help overlay).
    pub fn resolve(&self, key: KeyEvent, mode: InputMode) -> Option<Action> {
        let bound = self.bindings.get(&key).copied();

        match mode {
            InputMode::HelpOverlay => match bound {
                Some(BoundAction::Help) => Some(Action::ToggleHelp),
                Some(BoundAction::ExitInteract) => Some(Action::ToggleHelp),
                _ => None,
            },

            InputMode::Interact => match bound {
                Some(BoundAction::ForceQuit) => Some(Action::Quit),
                Some(BoundAction::ExitInteract) => Some(Action::ExitInteract),
                Some(BoundAction::Help) => Some(Action::ToggleHelp),
                _ => None,
            },

            InputMode::Edit => match bound {
                Some(BoundAction::ForceQuit) => Some(Action::Quit),
                Some(BoundAction::Help) => Some(Action::ToggleHelp),
                _ => Some(Action::EditKey(key)),
            },

            InputMode::Normal => bound.map(bound_action_to_action),
        }
    }
}

/// Convert a `BoundAction` to the corresponding `Action`.
fn bound_action_to_action(bound: BoundAction) -> Action {
    match bound {
        BoundAction::Quit => Action::Quit,
        BoundAction::ForceQuit => Action::Quit,
        BoundAction::Reload => Action::ReloadConfig,
        BoundAction::Help => Action::ToggleHelp,
        BoundAction::FocusNext => Action::FocusNext,
        BoundAction::FocusPrev => Action::FocusPrev,
        BoundAction::FocusUp => Action::FocusDirection(Direction::Up),
        BoundAction::FocusDown => Action::FocusDirection(Direction::Down),
        BoundAction::FocusLeft => Action::FocusDirection(Direction::Left),
        BoundAction::FocusRight => Action::FocusDirection(Direction::Right),
        BoundAction::EnterInteract => Action::EnterInteract,
        BoundAction::ExitInteract => Action::ExitInteract,
        BoundAction::EditMode => Action::EnterEditMode,
        BoundAction::ProfileNext => Action::SwitchProfile(ProfileDirection::Next),
        BoundAction::ProfilePrev => Action::SwitchProfile(ProfileDirection::Prev),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_key_string tests ───────────────────────────────────────────────

    #[test]
    fn parse_simple_char() {
        let ke = parse_key_string("q").unwrap();
        assert_eq!(ke.code, KeyCode::Char('q'));
        assert_eq!(ke.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn parse_special_char() {
        let ke = parse_key_string("?").unwrap();
        assert_eq!(ke.code, KeyCode::Char('?'));
        assert_eq!(ke.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn parse_ctrl_c() {
        let ke = parse_key_string("ctrl+c").unwrap();
        assert_eq!(ke.code, KeyCode::Char('c'));
        assert_eq!(ke.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn parse_shift_tab() {
        let ke = parse_key_string("shift+tab").unwrap();
        assert_eq!(ke.code, KeyCode::BackTab);
        // BackTab implies shift, but modifiers field still has SHIFT set
        assert!(ke.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn parse_alt_x() {
        let ke = parse_key_string("alt+x").unwrap();
        assert_eq!(ke.code, KeyCode::Char('x'));
        assert_eq!(ke.modifiers, KeyModifiers::ALT);
    }

    #[test]
    fn parse_tab() {
        let ke = parse_key_string("tab").unwrap();
        assert_eq!(ke.code, KeyCode::Tab);
        assert_eq!(ke.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn parse_escape() {
        let ke = parse_key_string("escape").unwrap();
        assert_eq!(ke.code, KeyCode::Esc);
    }

    #[test]
    fn parse_esc_alias() {
        let ke = parse_key_string("esc").unwrap();
        assert_eq!(ke.code, KeyCode::Esc);
    }

    #[test]
    fn parse_enter() {
        let ke = parse_key_string("enter").unwrap();
        assert_eq!(ke.code, KeyCode::Enter);
    }

    #[test]
    fn parse_arrow_keys() {
        assert_eq!(parse_key_string("up").unwrap().code, KeyCode::Up);
        assert_eq!(parse_key_string("down").unwrap().code, KeyCode::Down);
        assert_eq!(parse_key_string("left").unwrap().code, KeyCode::Left);
        assert_eq!(parse_key_string("right").unwrap().code, KeyCode::Right);
    }

    #[test]
    fn parse_ctrl_bracket() {
        let ke = parse_key_string("ctrl+]").unwrap();
        assert_eq!(ke.code, KeyCode::Char(']'));
        assert_eq!(ke.modifiers, KeyModifiers::CONTROL);

        let ke2 = parse_key_string("ctrl+[").unwrap();
        assert_eq!(ke2.code, KeyCode::Char('['));
        assert_eq!(ke2.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn parse_case_insensitive() {
        let ke = parse_key_string("Ctrl+C").unwrap();
        assert_eq!(ke.code, KeyCode::Char('c'));
        assert_eq!(ke.modifiers, KeyModifiers::CONTROL);

        let ke2 = parse_key_string("ESCAPE").unwrap();
        assert_eq!(ke2.code, KeyCode::Esc);
    }

    #[test]
    fn parse_whitespace_trimmed() {
        let ke = parse_key_string("  q  ").unwrap();
        assert_eq!(ke.code, KeyCode::Char('q'));
    }

    #[test]
    fn parse_empty_string_errors() {
        assert!(parse_key_string("").is_err());
        assert!(parse_key_string("   ").is_err());
    }

    #[test]
    fn parse_unknown_modifier_errors() {
        assert!(parse_key_string("super+q").is_err());
    }

    #[test]
    fn parse_unknown_key_name_errors() {
        assert!(parse_key_string("invalid_key_name").is_err());
    }

    #[test]
    fn parse_trailing_plus_errors() {
        // "ctrl+" has empty key part
        assert!(parse_key_string("ctrl+").is_err());
    }

    // ── KeyDispatcher tests ──────────────────────────────────────────────────

    fn default_dispatcher() -> KeyDispatcher {
        KeyDispatcher::new(&KeybindsConfig::default()).unwrap()
    }

    #[test]
    fn default_normal_quit() {
        let d = default_dispatcher();
        let key = parse_key_string("q").unwrap();
        assert_eq!(d.resolve(key, InputMode::Normal), Some(Action::Quit));
    }

    #[test]
    fn default_normal_focus_next() {
        let d = default_dispatcher();
        let key = parse_key_string("tab").unwrap();
        assert_eq!(d.resolve(key, InputMode::Normal), Some(Action::FocusNext));
    }

    #[test]
    fn default_normal_focus_prev() {
        let d = default_dispatcher();
        let key = parse_key_string("shift+tab").unwrap();
        assert_eq!(d.resolve(key, InputMode::Normal), Some(Action::FocusPrev));
    }

    #[test]
    fn default_normal_focus_directions() {
        let d = default_dispatcher();
        assert_eq!(
            d.resolve(parse_key_string("up").unwrap(), InputMode::Normal),
            Some(Action::FocusDirection(Direction::Up))
        );
        assert_eq!(
            d.resolve(parse_key_string("down").unwrap(), InputMode::Normal),
            Some(Action::FocusDirection(Direction::Down))
        );
        assert_eq!(
            d.resolve(parse_key_string("left").unwrap(), InputMode::Normal),
            Some(Action::FocusDirection(Direction::Left))
        );
        assert_eq!(
            d.resolve(parse_key_string("right").unwrap(), InputMode::Normal),
            Some(Action::FocusDirection(Direction::Right))
        );
    }

    #[test]
    fn default_normal_help() {
        let d = default_dispatcher();
        let key = parse_key_string("?").unwrap();
        assert_eq!(d.resolve(key, InputMode::Normal), Some(Action::ToggleHelp));
    }

    #[test]
    fn default_normal_reload() {
        let d = default_dispatcher();
        let key = parse_key_string("r").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Normal),
            Some(Action::ReloadConfig)
        );
    }

    #[test]
    fn default_normal_enter_interact() {
        let d = default_dispatcher();
        let key = parse_key_string("enter").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Normal),
            Some(Action::EnterInteract)
        );
    }

    #[test]
    fn default_normal_edit_mode() {
        let d = default_dispatcher();
        let key = parse_key_string("e").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Normal),
            Some(Action::EnterEditMode)
        );
    }

    #[test]
    fn default_normal_unbound_key_returns_none() {
        let d = default_dispatcher();
        let key = parse_key_string("z").unwrap();
        assert_eq!(d.resolve(key, InputMode::Normal), None);
    }

    // ── Interact mode ────────────────────────────────────────────────────────

    #[test]
    fn interact_escape_exits() {
        let d = default_dispatcher();
        let key = parse_key_string("escape").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Interact),
            Some(Action::ExitInteract)
        );
    }

    #[test]
    fn interact_ctrl_c_quits() {
        let d = default_dispatcher();
        let key = parse_key_string("ctrl+c").unwrap();
        assert_eq!(d.resolve(key, InputMode::Interact), Some(Action::Quit));
    }

    #[test]
    fn interact_help_toggles() {
        let d = default_dispatcher();
        let key = parse_key_string("?").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Interact),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn interact_q_returns_none() {
        let d = default_dispatcher();
        let key = parse_key_string("q").unwrap();
        assert_eq!(d.resolve(key, InputMode::Interact), None);
    }

    #[test]
    fn interact_unbound_key_returns_none() {
        let d = default_dispatcher();
        let key = parse_key_string("a").unwrap();
        assert_eq!(d.resolve(key, InputMode::Interact), None);
    }

    // ── Help overlay ─────────────────────────────────────────────────────────

    #[test]
    fn help_question_mark_toggles() {
        let d = default_dispatcher();
        let key = parse_key_string("?").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::HelpOverlay),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn help_escape_toggles() {
        let d = default_dispatcher();
        let key = parse_key_string("escape").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::HelpOverlay),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn help_q_returns_none() {
        let d = default_dispatcher();
        let key = parse_key_string("q").unwrap();
        assert_eq!(d.resolve(key, InputMode::HelpOverlay), None);
    }

    #[test]
    fn help_any_other_key_returns_none() {
        let d = default_dispatcher();
        let key = parse_key_string("a").unwrap();
        assert_eq!(d.resolve(key, InputMode::HelpOverlay), None);
    }

    // ── Edit mode ────────────────────────────────────────────────────────────

    #[test]
    fn edit_ctrl_c_quits() {
        let d = default_dispatcher();
        let key = parse_key_string("ctrl+c").unwrap();
        assert_eq!(d.resolve(key, InputMode::Edit), Some(Action::Quit));
    }

    #[test]
    fn edit_help_toggles() {
        let d = default_dispatcher();
        let key = parse_key_string("?").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Edit),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn edit_regular_key_becomes_edit_key() {
        let d = default_dispatcher();
        let key = parse_key_string("a").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Edit),
            Some(Action::EditKey(key))
        );
    }

    #[test]
    fn edit_arrow_key_becomes_edit_key() {
        let d = default_dispatcher();
        let key = parse_key_string("up").unwrap();
        // "up" is bound to FocusUp, but in Edit mode it should become EditKey
        assert_eq!(
            d.resolve(key, InputMode::Edit),
            Some(Action::EditKey(key))
        );
    }

    // ── Custom keybinds ──────────────────────────────────────────────────────

    #[test]
    fn custom_quit_key() {
        let config = KeybindsConfig {
            quit: Some("x".to_string()),
            ..Default::default()
        };
        let d = KeyDispatcher::new(&config).unwrap();

        // x should now quit
        let key_x = parse_key_string("x").unwrap();
        assert_eq!(d.resolve(key_x, InputMode::Normal), Some(Action::Quit));

        // q should no longer be bound
        let key_q = parse_key_string("q").unwrap();
        assert_eq!(d.resolve(key_q, InputMode::Normal), None);
    }

    #[test]
    fn duplicate_bindings_error() {
        let config = KeybindsConfig {
            quit: Some("r".to_string()),
            // reload defaults to "r" too
            ..Default::default()
        };
        let result = KeyDispatcher::new(&config);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Duplicate keybind"));
    }

    #[test]
    fn cannot_remap_ctrl_c_to_other_action() {
        let config = KeybindsConfig {
            quit: Some("ctrl+c".to_string()),
            ..Default::default()
        };
        let result = KeyDispatcher::new(&config);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("ctrl+c"));
    }

    // ── Profile switching ────────────────────────────────────────────────────

    #[test]
    fn profile_next() {
        let d = default_dispatcher();
        let key = parse_key_string("ctrl+]").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Normal),
            Some(Action::SwitchProfile(ProfileDirection::Next))
        );
    }

    #[test]
    fn profile_prev() {
        let d = default_dispatcher();
        let key = parse_key_string("ctrl+[").unwrap();
        assert_eq!(
            d.resolve(key, InputMode::Normal),
            Some(Action::SwitchProfile(ProfileDirection::Prev))
        );
    }

    // ── Force quit always works ──────────────────────────────────────────────

    #[test]
    fn force_quit_in_all_modes() {
        let d = default_dispatcher();
        let key = parse_key_string("ctrl+c").unwrap();

        assert_eq!(d.resolve(key, InputMode::Normal), Some(Action::Quit));
        assert_eq!(d.resolve(key, InputMode::Interact), Some(Action::Quit));
        assert_eq!(d.resolve(key, InputMode::Edit), Some(Action::Quit));
        // In HelpOverlay, ctrl+c is not handled (returns None)
        // because HelpOverlay only responds to help/esc
        assert_eq!(d.resolve(key, InputMode::HelpOverlay), None);
    }
}
