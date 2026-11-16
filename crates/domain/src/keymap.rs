//! Actions and configurable key bindings, including multi-key sequences
//! like vim's `g g`.

use std::collections::BTreeMap;

macro_rules! actions {
    ($($variant:ident => $name:literal, $title:literal;)*) => {
        /// Everything the user can trigger from keys or the command palette.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $($variant),* }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$variant),*];

            /// Stable name used in config.toml.
            pub fn name(self) -> &'static str {
                match self { $(Action::$variant => $name),* }
            }

            /// Human-readable title for the palette and settings.
            pub fn title(self) -> &'static str {
                match self { $(Action::$variant => $title),* }
            }

            pub fn from_name(name: &str) -> Option<Action> {
                match name { $($name => Some(Action::$variant),)* _ => None }
            }
        }
    };
}

actions! {
    Open => "open", "Add to session…";
    ChooseFile => "choose-file", "Choose file…";
    UpdatePage => "update-page", "Update page from the web";
    ToggleRevoked => "toggle-revoked", "Show or hide revoked text";
    Libraries => "libraries", "Libraries…";
    NewLibrary => "new-library", "New library…";
    AddToLibrary => "add-to-library", "Add document to library…";
    CommandPalette => "command-palette", "Command palette";
    Search => "search", "Search in document";
    SearchSession => "search-session", "Search all tabs in session…";
    NextMatch => "next-match", "Next search match";
    PreviousMatch => "previous-match", "Previous search match";
    GoToPage => "go-to-page", "Go to page…";
    Settings => "settings", "Settings";
    ToggleSidebar => "toggle-sidebar", "Toggle sidebar";
    ShowThumbnails => "show-thumbnails", "Show page thumbnails";
    ShowOutline => "show-outline", "Show outline";
    ShowNotes => "show-notes", "Show notes list";
    FocusMode => "focus-mode", "Toggle focus mode";
    Fullscreen => "fullscreen", "Toggle fullscreen";
    ZoomIn => "zoom-in", "Zoom in";
    ZoomOut => "zoom-out", "Zoom out";
    ZoomReset => "zoom-reset", "Reset zoom";
    FitWidth => "fit-width", "Fit width";
    FitPage => "fit-page", "Fit page";
    PreviousPage => "previous-page", "Previous page";
    NextPage => "next-page", "Next page";
    FirstPage => "first-page", "First page";
    LastPage => "last-page", "Last page";
    ScrollDown => "scroll-down", "Scroll down";
    ScrollUp => "scroll-up", "Scroll up";
    ScrollLeft => "scroll-left", "Scroll left";
    ScrollRight => "scroll-right", "Scroll right";
    HalfPageDown => "half-page-down", "Scroll half a screen down";
    HalfPageUp => "half-page-up", "Scroll half a screen up";
    NextTab => "next-tab", "Next tab";
    PreviousTab => "previous-tab", "Previous tab";
    CloseTab => "close-tab", "Close tab";
    Home => "home", "Go to home screen";
    SwitchSession => "switch-session", "Switch session…";
    NewSession => "new-session", "New session…";
    RenameSession => "rename-session", "Rename session…";
    DeleteSession => "delete-session", "Delete session";
    Copy => "copy", "Copy selected text";
    Highlight => "highlight", "Highlight selection";
    AddNote => "add-note", "Add note to selection";
    ToggleStudyTimer => "toggle-study-timer", "Pause / resume study timer";
    MarkSection => "mark-section", "Mark current section read / reviewed";
    Color1 => "color-1", "Highlight with color 1";
    Color2 => "color-2", "Highlight with color 2";
    Color3 => "color-3", "Highlight with color 3";
    Color4 => "color-4", "Highlight with color 4";
    Color5 => "color-5", "Highlight with color 5";
    Undo => "undo", "Undo annotation change";
    Redo => "redo", "Redo annotation change";
    ExportNotes => "export-notes", "Export notes (Markdown, Anki, CSV)…";
    ReloadConfig => "reload-config", "Reload config and themes";
    OpenConfig => "open-config", "Open config.toml";
    OpenThemesFolder => "open-themes-folder", "Open themes folder";
    CheckForUpdates => "check-for-updates", "Check for updates";
    Escape => "escape", "Close overlay / clear selection";
}

impl Action {
    /// The highlight color a `ColorN` action picks, from zero.
    pub fn color_index(self) -> Option<usize> {
        const COLORS: [Action; 5] = [
            Action::Color1,
            Action::Color2,
            Action::Color3,
            Action::Color4,
            Action::Color5,
        ];
        COLORS.iter().position(|a| *a == self)
    }
}

const DEFAULT_KEYS: &[(Action, &str)] = &[
    (Action::Open, "ctrl+o"),
    (Action::Libraries, "ctrl+shift+l"),
    (Action::CommandPalette, "ctrl+k"),
    (Action::CommandPalette, "ctrl+shift+p"),
    (Action::Search, "ctrl+f"),
    (Action::SearchSession, "ctrl+shift+f"),
    (Action::GoToPage, "ctrl+p"),
    (Action::Settings, "ctrl+,"),
    (Action::ToggleSidebar, "ctrl+b"),
    (Action::ShowThumbnails, "ctrl+shift+t"),
    (Action::ShowOutline, "ctrl+shift+o"),
    (Action::ShowNotes, "ctrl+shift+a"),
    (Action::FocusMode, "ctrl+."),
    (Action::Fullscreen, "f11"),
    (Action::ZoomIn, "ctrl+="),
    (Action::ZoomIn, "ctrl++"),
    (Action::ZoomOut, "ctrl+-"),
    (Action::ZoomReset, "ctrl+0"),
    (Action::FitWidth, "ctrl+1"),
    (Action::FitPage, "ctrl+2"),
    (Action::PreviousPage, "left"),
    (Action::PreviousPage, "pageup"),
    (Action::NextPage, "right"),
    (Action::NextPage, "pagedown"),
    (Action::FirstPage, "home"),
    (Action::LastPage, "end"),
    (Action::ScrollDown, "down"),
    (Action::ScrollUp, "up"),
    (Action::NextTab, "ctrl+tab"),
    (Action::PreviousTab, "ctrl+shift+tab"),
    (Action::CloseTab, "ctrl+w"),
    (Action::Home, "alt+home"),
    (Action::SwitchSession, "ctrl+shift+s"),
    (Action::NewSession, "ctrl+shift+n"),
    (Action::Copy, "ctrl+c"),
    (Action::Highlight, "ctrl+h"),
    (Action::AddNote, "ctrl+n"),
    (Action::MarkSection, "ctrl+l"),
    (Action::ToggleStudyTimer, "ctrl+t"),
    (Action::Color1, "1"),
    (Action::Color2, "2"),
    (Action::Color3, "3"),
    (Action::Color4, "4"),
    (Action::Color5, "5"),
    (Action::Undo, "ctrl+z"),
    (Action::Redo, "ctrl+y"),
    (Action::Redo, "ctrl+shift+z"),
    (Action::ExportNotes, "ctrl+e"),
    (Action::ExportNotes, "ctrl+shift+e"),
    (Action::Escape, "escape"),
];

const VIM_KEYS: &[(Action, &str)] = &[
    (Action::ScrollDown, "j"),
    (Action::ScrollUp, "k"),
    (Action::ScrollLeft, "h"),
    (Action::ScrollRight, "l"),
    (Action::HalfPageDown, "ctrl+d"),
    (Action::HalfPageUp, "ctrl+u"),
    (Action::NextPage, "shift+j"),
    (Action::PreviousPage, "shift+k"),
    (Action::FirstPage, "g g"),
    (Action::LastPage, "shift+g"),
    (Action::Search, "/"),
    (Action::NextMatch, "n"),
    (Action::PreviousMatch, "shift+n"),
    (Action::CommandPalette, ":"),
    (Action::NextTab, "g t"),
    (Action::PreviousTab, "g shift+t"),
    (Action::ZoomIn, "z i"),
    (Action::ZoomOut, "z o"),
    (Action::FitWidth, "z w"),
    (Action::FitPage, "z p"),
    (Action::Copy, "y"),
    (Action::Undo, "u"),
    (Action::Redo, "ctrl+r"),
    (Action::ToggleSidebar, "t"),
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Lowercase key name: a character or a named key like `pagedown`.
    pub key: String,
}

impl Chord {
    /// Parses `ctrl+shift+p`, `shift+g`, `G`, `ctrl++`, `f11`, `,`.
    pub fn parse(text: &str) -> Option<Chord> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (mods, key) = match text.strip_suffix("++") {
            Some(mods) => (mods, "+"),
            None if text == "+" => ("", "+"),
            None => match text.rsplit_once('+') {
                Some((mods, key)) => (mods, key),
                None => ("", text),
            },
        };
        let mut chord = Chord {
            ctrl: false,
            shift: false,
            alt: false,
            key: String::new(),
        };
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                _ => return None,
            }
        }
        let mut chars = key.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if c.is_ascii_uppercase() {
                chord.shift = true;
            }
            chord.key = c.to_ascii_lowercase().to_string();
        } else {
            let name = key.to_ascii_lowercase();
            named_key(&name)?;
            chord.key = name;
        }
        Some(chord)
    }

    /// Builds a chord from a Slint key event.
    pub fn from_event(text: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Chord> {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return None;
        };
        let (key, shift) = if c == key::BACKTAB {
            ("tab".to_owned(), true)
        } else if let Some(name) = key_name(c) {
            (name.to_owned(), shift)
        } else if c.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&c) {
            // Modifier keys on their own and other unnamed special keys.
            return None;
        } else {
            (c.to_lowercase().to_string(), shift || c.is_uppercase())
        };
        Some(Chord {
            ctrl,
            shift,
            alt,
            key,
        })
    }

    /// Display form, e.g. `Ctrl+Shift+P`.
    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        let key = match self.key.as_str() {
            "left" => "←".to_owned(),
            "right" => "→".to_owned(),
            "up" => "↑".to_owned(),
            "down" => "↓".to_owned(),
            "pageup" => "PgUp".to_owned(),
            "pagedown" => "PgDn".to_owned(),
            "escape" => "Esc".to_owned(),
            k if k.len() > 1 => {
                let mut c = k.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
            k => k.to_uppercase(),
        };
        out.push_str(&key);
        out
    }
}

/// Special keys as the UI toolkit (Slint) sends them: one character from
/// its key codes. The app checks these against Slint's own in a test.
pub mod key {
    pub const BACKSPACE: char = '\u{0008}';
    pub const TAB: char = '\u{0009}';
    pub const RETURN: char = '\u{000a}';
    pub const SHIFT: char = '\u{0010}';
    pub const CONTROL: char = '\u{0011}';
    pub const BACKTAB: char = '\u{0019}';
    pub const ESCAPE: char = '\u{001b}';
    pub const SPACE: char = '\u{0020}';
    pub const DELETE: char = '\u{007f}';
    pub const UP: char = '\u{F700}';
    pub const DOWN: char = '\u{F701}';
    pub const LEFT: char = '\u{F702}';
    pub const RIGHT: char = '\u{F703}';
    /// F1; F2 to F12 follow.
    pub const F1: char = '\u{F704}';
    pub const HOME: char = '\u{F729}';
    pub const END: char = '\u{F72B}';
    pub const PAGE_UP: char = '\u{F72C}';
    pub const PAGE_DOWN: char = '\u{F72D}';
}

const NAMED: &[(char, &str)] = &[
    (key::LEFT, "left"),
    (key::RIGHT, "right"),
    (key::UP, "up"),
    (key::DOWN, "down"),
    (key::PAGE_UP, "pageup"),
    (key::PAGE_DOWN, "pagedown"),
    (key::HOME, "home"),
    (key::END, "end"),
    (key::ESCAPE, "escape"),
    (key::RETURN, "enter"),
    (key::TAB, "tab"),
    (key::BACKSPACE, "backspace"),
    (key::DELETE, "delete"),
    (key::SPACE, "space"),
];

const FUNCTION_KEYS: [&str; 12] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
];

fn key_name(c: char) -> Option<&'static str> {
    NAMED
        .iter()
        .find(|(key, _)| *key == c)
        .map(|(_, name)| *name)
        .or_else(|| {
            let i = (c as u32).checked_sub(key::F1 as u32)?;
            FUNCTION_KEYS.get(i as usize).copied()
        })
}

fn named_key(name: &str) -> Option<()> {
    (NAMED.iter().any(|(_, n)| *n == name) || FUNCTION_KEYS.contains(&name)).then_some(())
}

pub type Sequence = Vec<Chord>;

pub fn parse_sequence(text: &str) -> Option<Sequence> {
    text.split_whitespace().map(Chord::parse).collect()
}

/// Resolved bindings plus the state of a sequence being typed.
#[derive(Debug, Default)]
pub struct Keymap {
    bindings: Vec<(Sequence, Action)>,
    pending: Sequence,
}

pub enum KeyResult {
    Action(Action),
    /// Part of a longer sequence; wait for the next key.
    Pending,
    Unbound,
}

impl Keymap {
    /// Defaults, plus vim keys if enabled, with user overrides replacing an
    /// action's bindings. Returns problems with the overrides.
    pub fn new(vim: bool, overrides: &BTreeMap<String, Vec<String>>) -> (Self, Vec<String>) {
        let mut bindings: Vec<(Sequence, Action)> = DEFAULT_KEYS
            .iter()
            .chain(if vim { VIM_KEYS } else { &[] })
            .filter_map(|(action, keys)| Some((parse_sequence(keys)?, *action)))
            .collect();
        let mut problems = Vec::new();
        for (name, keys) in overrides {
            let Some(action) = Action::from_name(name) else {
                problems.push(format!("[keys] unknown action \"{name}\""));
                continue;
            };
            bindings.retain(|(_, a)| *a != action);
            for keys in keys {
                match parse_sequence(keys) {
                    Some(seq) => {
                        // A key belongs to one action: the override wins.
                        bindings.retain(|(s, _)| *s != seq);
                        bindings.push((seq, action));
                    }
                    None => problems.push(format!("[keys] {name}: can't parse \"{keys}\"")),
                }
            }
        }
        (
            Self {
                bindings,
                pending: Vec::new(),
            },
            problems,
        )
    }

    pub fn press(&mut self, chord: Chord) -> KeyResult {
        self.pending.push(chord.clone());
        if let Some(result) = self.resolve() {
            return result;
        }
        // The sequence went nowhere; try this key on its own.
        self.pending = vec![chord];
        self.resolve().unwrap_or_else(|| {
            self.pending.clear();
            KeyResult::Unbound
        })
    }

    fn resolve(&mut self) -> Option<KeyResult> {
        if let Some((_, action)) = self.bindings.iter().find(|(s, _)| *s == self.pending) {
            self.pending.clear();
            return Some(KeyResult::Action(*action));
        }
        let is_prefix = self
            .bindings
            .iter()
            .any(|(s, _)| s.len() > self.pending.len() && s.starts_with(&self.pending));
        is_prefix.then_some(KeyResult::Pending)
    }

    /// Display strings of every binding for an action, e.g. `["Ctrl+K", ":"]`.
    pub fn keys_for(&self, action: Action) -> Vec<String> {
        self.bindings
            .iter()
            .filter(|(_, a)| *a == action)
            .map(|(seq, _)| seq.iter().map(Chord::display).collect::<Vec<_>>().join(" "))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keymap: &mut Keymap, text: &str, ctrl: bool, shift: bool) -> Option<Action> {
        let chord = Chord::from_event(text, ctrl, shift, false)?;
        match keymap.press(chord) {
            KeyResult::Action(action) => Some(action),
            _ => None,
        }
    }

    fn key(k: char) -> String {
        k.to_string()
    }

    #[test]
    fn names_round_trip() {
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(*action));
        }
    }

    #[test]
    fn defaults_parse() {
        for (_, keys) in DEFAULT_KEYS.iter().chain(VIM_KEYS) {
            assert!(parse_sequence(keys).is_some(), "{keys}");
        }
    }

    #[test]
    fn default_bindings() {
        let (mut km, problems) = Keymap::new(false, &BTreeMap::new());
        assert!(problems.is_empty());
        assert_eq!(press(&mut km, "o", true, false), Some(Action::Open));
        assert_eq!(press(&mut km, "O", true, true), Some(Action::ShowOutline));
        assert_eq!(press(&mut km, "Q", true, true), None);
        assert_eq!(press(&mut km, "3", false, false), Some(Action::Color3));
        assert_eq!(Action::Color3.color_index(), Some(2));
        assert_eq!(Action::Open.color_index(), None);
        assert_eq!(
            press(&mut km, &key(key::PAGE_DOWN), false, false),
            Some(Action::NextPage)
        );
        assert_eq!(
            press(&mut km, &key(key::BACKTAB), true, true),
            Some(Action::PreviousTab)
        );
        assert_eq!(press(&mut km, "j", false, false), None);
    }

    #[test]
    fn vim_sequences() {
        let (mut km, _) = Keymap::new(true, &BTreeMap::new());
        assert_eq!(press(&mut km, "j", false, false), Some(Action::ScrollDown));
        assert_eq!(press(&mut km, "g", false, false), None);
        assert_eq!(press(&mut km, "g", false, false), Some(Action::FirstPage));
        assert_eq!(press(&mut km, "G", false, true), Some(Action::LastPage));
        // A dead-end sequence falls back to the last key alone.
        assert_eq!(press(&mut km, "g", false, false), None);
        assert_eq!(press(&mut km, "j", false, false), Some(Action::ScrollDown));
    }

    #[test]
    fn overrides_replace_and_unbind() {
        let mut overrides = BTreeMap::new();
        overrides.insert("open".to_owned(), vec!["ctrl+shift+o".to_owned()]);
        overrides.insert("search".to_owned(), vec![]);
        overrides.insert("bogus".to_owned(), vec!["x".to_owned()]);
        overrides.insert(
            "go-to-page".to_owned(),
            vec!["ctrl+o".to_owned(), "??".to_owned()],
        );
        let (mut km, problems) = Keymap::new(false, &overrides);
        assert_eq!(problems.len(), 2);
        assert_eq!(press(&mut km, "o", true, true), Some(Action::Open));
        assert_eq!(press(&mut km, "o", true, false), Some(Action::GoToPage));
        assert_eq!(press(&mut km, "f", true, false), None);
        assert_eq!(km.keys_for(Action::Open), vec!["Ctrl+Shift+O"]);
    }
}
