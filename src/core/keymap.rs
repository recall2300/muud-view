//! Key bindings: a default table plus the user's overrides. Modifiers are
//! matched exactly, so `Ctrl+C` and `Ctrl+Shift+C` never trigger each other.

use eframe::egui::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    OpenFile,
    OpenFolder,
    CloseFile,
    Exit,
    Escape,
    Next,
    Prev,
    First,
    Last,
    Forward10,
    Back10,
    RandomNext,
    RandomPrev,
    FitWindow,
    FitWidth,
    Original,
    ZoomIn,
    ZoomOut,
    LockZoom,
    StretchSmall,
    Fullscreen,
    Maximize,
    AlwaysOnTop,
    SmoothOn,
    SmoothOff,
    CycleBackground,
    CycleOsd,
    RotateCw,
    RotateCcw,
    Rotate180,
    ResetRotation,
    FlipH,
    FlipV,
    ToggleInfo,
    Minimap,
    Compare,
    CompareMode,
    PixelInfo,
    ShareCopy,
    CopyVisible,
    Star,
    StarredOnly,
    Grid,
    NextFolder,
    PrevFolder,
    TwoPage,
    Webtoon,
    SaveRotation,
    CopyImage,
    CopyResized,
    CopyPath,
    CopyExif,
    Paste,
    ShowInExplorer,
    Rename,
    Trash,
    DeletePermanently,
    CopyToFolder(u8),
    MoveToFolder(u8),
    Undo,
    Slideshow(u8),
    StopSlideshow,
    Folders,
    Help,
    Setup,
    PinTop,
    PinBottom,
}

impl Cmd {
    /// Stable name used in settings files.
    pub fn id(self) -> String {
        match self {
            Cmd::CopyToFolder(n) => format!("copy_to_folder_{n}"),
            Cmd::MoveToFolder(n) => format!("move_to_folder_{n}"),
            Cmd::Slideshow(n) => format!("slideshow_{n}"),
            other => {
                // CamelCase variant name -> snake_case.
                let name = format!("{other:?}");
                let mut out = String::new();
                for (i, c) in name.chars().enumerate() {
                    if c.is_uppercase() && i > 0 {
                        out.push('_');
                    }
                    out.push(c.to_ascii_lowercase());
                }
                out
            }
        }
    }

    pub fn from_id(id: &str) -> Option<Cmd> {
        ALL.iter().copied().find(|c| c.id() == id)
    }
}

/// Every command, in the order the key settings list them.
pub const ALL: &[Cmd] = &[
    Cmd::Next,
    Cmd::Prev,
    Cmd::First,
    Cmd::Last,
    Cmd::Forward10,
    Cmd::Back10,
    Cmd::RandomNext,
    Cmd::RandomPrev,
    Cmd::FitWindow,
    Cmd::FitWidth,
    Cmd::Original,
    Cmd::ZoomIn,
    Cmd::ZoomOut,
    Cmd::LockZoom,
    Cmd::StretchSmall,
    Cmd::Fullscreen,
    Cmd::Maximize,
    Cmd::AlwaysOnTop,
    Cmd::SmoothOn,
    Cmd::SmoothOff,
    Cmd::CycleBackground,
    Cmd::CycleOsd,
    Cmd::ToggleInfo,
    Cmd::Minimap,
    Cmd::Compare,
    Cmd::CompareMode,
    Cmd::PixelInfo,
    Cmd::ShareCopy,
    Cmd::CopyVisible,
    Cmd::Star,
    Cmd::StarredOnly,
    Cmd::Grid,
    Cmd::NextFolder,
    Cmd::PrevFolder,
    Cmd::TwoPage,
    Cmd::Webtoon,
    Cmd::SaveRotation,
    Cmd::PinTop,
    Cmd::PinBottom,
    Cmd::RotateCw,
    Cmd::RotateCcw,
    Cmd::Rotate180,
    Cmd::ResetRotation,
    Cmd::FlipH,
    Cmd::FlipV,
    Cmd::CopyImage,
    Cmd::CopyResized,
    Cmd::CopyExif,
    Cmd::CopyPath,
    Cmd::Paste,
    Cmd::CopyToFolder(1),
    Cmd::MoveToFolder(1),
    Cmd::CopyToFolder(2),
    Cmd::MoveToFolder(2),
    Cmd::CopyToFolder(3),
    Cmd::CopyToFolder(4),
    Cmd::CopyToFolder(5),
    Cmd::CopyToFolder(6),
    Cmd::CopyToFolder(7),
    Cmd::CopyToFolder(8),
    Cmd::CopyToFolder(9),
    Cmd::Undo,
    Cmd::Trash,
    Cmd::DeletePermanently,
    Cmd::Rename,
    Cmd::ShowInExplorer,
    Cmd::OpenFile,
    Cmd::OpenFolder,
    Cmd::Folders,
    Cmd::CloseFile,
    Cmd::Slideshow(1),
    Cmd::Slideshow(2),
    Cmd::Slideshow(3),
    Cmd::Slideshow(4),
    Cmd::Slideshow(5),
    Cmd::Slideshow(6),
    Cmd::Slideshow(7),
    Cmd::Slideshow(8),
    Cmd::Slideshow(9),
    Cmd::StopSlideshow,
    Cmd::Help,
    Cmd::Setup,
    Cmd::Exit,
    Cmd::Escape,
];

/// Default bindings, close to what Korean image viewers use.
pub const DEFAULTS: &[(&str, Cmd)] = &[
    ("F2", Cmd::OpenFile),
    ("Ctrl+O", Cmd::OpenFile),
    ("F", Cmd::OpenFolder),
    ("F3", Cmd::Folders),
    ("Ctrl+B", Cmd::Folders),
    ("F4", Cmd::CloseFile),
    ("X", Cmd::Exit),
    ("Ctrl+W", Cmd::Exit),
    ("Escape", Cmd::Escape),
    ("PageDown", Cmd::Next),
    ("Right", Cmd::Next),
    ("Down", Cmd::Next),
    ("Space", Cmd::Next),
    ("PageUp", Cmd::Prev),
    ("Left", Cmd::Prev),
    ("Up", Cmd::Prev),
    ("Backspace", Cmd::Prev),
    ("Shift+Space", Cmd::Prev),
    ("Home", Cmd::First),
    ("End", Cmd::Last),
    ("Ctrl+PageDown", Cmd::Forward10),
    ("Ctrl+PageUp", Cmd::Back10),
    ("Ctrl+Alt+PageDown", Cmd::RandomNext),
    ("Ctrl+Alt+PageUp", Cmd::RandomPrev),
    ("9", Cmd::FitWindow),
    ("1", Cmd::FitWindow),
    ("8", Cmd::FitWidth),
    ("0", Cmd::Original),
    ("Shift+8", Cmd::Original),
    ("Plus", Cmd::ZoomIn),
    ("Equals", Cmd::ZoomIn),
    ("Minus", Cmd::ZoomOut),
    ("Ctrl+K", Cmd::LockZoom),
    ("Z", Cmd::StretchSmall),
    ("Enter", Cmd::Fullscreen),
    ("F11", Cmd::Fullscreen),
    ("N", Cmd::Fullscreen),
    ("Alt+Enter", Cmd::Fullscreen),
    ("M", Cmd::Maximize),
    ("Ctrl+A", Cmd::AlwaysOnTop),
    ("I", Cmd::SmoothOn),
    ("U", Cmd::SmoothOff),
    ("B", Cmd::CycleBackground),
    ("O", Cmd::CycleOsd),
    ("Tab", Cmd::ToggleInfo),
    ("Slash", Cmd::Minimap),
    ("C", Cmd::Compare),
    ("V", Cmd::CompareMode),
    ("P", Cmd::PixelInfo),
    ("Ctrl+S", Cmd::ShareCopy),
    ("Alt+C", Cmd::CopyVisible),
    ("S", Cmd::Star),
    ("Shift+S", Cmd::StarredOnly),
    ("G", Cmd::Grid),
    ("CloseBracket", Cmd::NextFolder),
    ("OpenBracket", Cmd::PrevFolder),
    ("D", Cmd::TwoPage),
    ("W", Cmd::Webtoon),
    ("Ctrl+Shift+S", Cmd::SaveRotation),
    ("Backtick", Cmd::PinTop),
    ("Shift+Backtick", Cmd::PinBottom),
    ("Ctrl+R", Cmd::RotateCw),
    ("Alt+Right", Cmd::RotateCw),
    ("Ctrl+L", Cmd::RotateCcw),
    ("Alt+Left", Cmd::RotateCcw),
    ("Alt+Down", Cmd::Rotate180),
    ("Alt+Up", Cmd::ResetRotation),
    ("Ctrl+M", Cmd::FlipH),
    ("Ctrl+F", Cmd::FlipV),
    ("Ctrl+C", Cmd::CopyImage),
    ("Ctrl+Alt+C", Cmd::CopyResized),
    ("Ctrl+Shift+C", Cmd::CopyExif),
    ("Ctrl+Alt+Shift+C", Cmd::CopyPath),
    ("Ctrl+V", Cmd::Paste),
    ("Ctrl+Enter", Cmd::ShowInExplorer),
    ("Shift+F2", Cmd::Rename),
    ("Delete", Cmd::Trash),
    ("Shift+Delete", Cmd::DeletePermanently),
    ("Insert", Cmd::CopyToFolder(1)),
    ("Ctrl+Insert", Cmd::MoveToFolder(1)),
    ("Shift+Insert", Cmd::CopyToFolder(2)),
    ("Alt+1", Cmd::CopyToFolder(1)),
    ("Alt+2", Cmd::CopyToFolder(2)),
    ("Alt+3", Cmd::CopyToFolder(3)),
    ("Alt+4", Cmd::CopyToFolder(4)),
    ("Alt+5", Cmd::CopyToFolder(5)),
    ("Alt+6", Cmd::CopyToFolder(6)),
    ("Alt+7", Cmd::CopyToFolder(7)),
    ("Alt+8", Cmd::CopyToFolder(8)),
    ("Alt+9", Cmd::CopyToFolder(9)),
    ("Ctrl+Z", Cmd::Undo),
    ("Ctrl+1", Cmd::Slideshow(1)),
    ("Ctrl+2", Cmd::Slideshow(2)),
    ("Ctrl+3", Cmd::Slideshow(3)),
    ("Ctrl+4", Cmd::Slideshow(4)),
    ("Ctrl+5", Cmd::Slideshow(5)),
    ("Ctrl+6", Cmd::Slideshow(6)),
    ("Ctrl+7", Cmd::Slideshow(7)),
    ("Ctrl+8", Cmd::Slideshow(8)),
    ("Ctrl+9", Cmd::Slideshow(9)),
    ("Ctrl+0", Cmd::StopSlideshow),
    ("F1", Cmd::Help),
    ("F5", Cmd::Setup),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Binding {
    pub fn from_event(key: Key, mods: Modifiers) -> Self {
        Self {
            key,
            ctrl: mods.ctrl || mods.command || mods.mac_cmd,
            shift: mods.shift,
            alt: mods.alt,
        }
    }

    /// Parses "Ctrl+Alt+C", "Shift+Insert", "PageDown", "Plus".
    pub fn parse(s: &str) -> Option<Self> {
        let mut b = Binding {
            key: Key::A,
            ctrl: false,
            shift: false,
            alt: false,
        };
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => b.ctrl = true,
                "shift" => b.shift = true,
                "alt" => b.alt = true,
                _ => return None,
            }
        }
        b.key = Key::from_name(key)?;
        Some(b)
    }

    /// Text for the screen: symbols instead of key names where clearer.
    pub fn display(&self) -> String {
        let key = match self.key {
            Key::Backtick if self.shift => return self.with_mods("~", false),
            Key::Backtick => "`",
            Key::Plus => "+",
            Key::Minus => "−",
            Key::Equals => "=",
            Key::Slash => "/",
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::ArrowRight => "→",
            Key::ArrowLeft => "←",
            Key::ArrowUp => "↑",
            Key::ArrowDown => "↓",
            Key::PageDown => "PgDn",
            Key::PageUp => "PgUp",
            Key::Backspace => "Bksp",
            Key::Escape => "Esc",
            k => k.name(),
        };
        self.with_mods(key, self.shift)
    }

    fn with_mods(&self, key: &str, shift: bool) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if shift {
            s.push_str("Shift+");
        }
        s.push_str(key);
        s.to_uppercase()
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.name());
        s
    }
}

/// A titled group of help rows: (commands, label).
pub type HelpSection = (&'static str, &'static [(&'static [Cmd], &'static str)]);

/// What the F1 sheet lists; the keys shown come from the active keymap.
pub const HELP: &[HelpSection] = &[
    (
        "MOVE",
        &[
            (&[Cmd::Next], "NEXT"),
            (&[Cmd::Prev], "PREVIOUS"),
            (&[Cmd::First, Cmd::Last], "FIRST / LAST"),
            (&[Cmd::Forward10, Cmd::Back10], "± 10 IMAGES"),
            (&[Cmd::RandomNext], "RANDOM"),
            (&[Cmd::Slideshow(3), Cmd::StopSlideshow], "SLIDESHOW / STOP"),
            (&[Cmd::Folders], "RECENT FOLDERS"),
        ],
    ),
    (
        "VIEW",
        &[
            (&[Cmd::FitWindow], "FIT WINDOW"),
            (&[Cmd::FitWidth], "FIT WIDTH"),
            (&[Cmd::Original], "ORIGINAL 100%"),
            (&[Cmd::ZoomIn, Cmd::ZoomOut], "ZOOM"),
            (&[Cmd::LockZoom], "LOCK ZOOM"),
            (&[Cmd::Fullscreen], "FULLSCREEN"),
            (&[Cmd::CycleBackground], "BACKGROUND"),
            (&[Cmd::CycleOsd], "ON-SCREEN INFO"),
            (&[Cmd::ToggleInfo], "INFO PANEL"),
            (&[Cmd::Minimap], "MINIMAP"),
            (&[Cmd::Compare], "COMPARE"),
            (&[Cmd::CompareMode], "COMPARE VIEW"),
            (&[Cmd::PixelInfo], "PIXEL INFO"),
            (&[Cmd::ShareCopy], "SAVE FOR SHARING"),
            (&[Cmd::CopyVisible], "COPY VISIBLE AREA"),
            (&[Cmd::Grid], "THUMBNAILS"),
            (
                &[Cmd::NextFolder, Cmd::PrevFolder],
                "NEXT / PREVIOUS FOLDER",
            ),
            (&[Cmd::Star], "STAR"),
            (&[Cmd::StarredOnly], "STARRED ONLY"),
            (&[Cmd::PinTop, Cmd::PinBottom], "PIN TOP / BOTTOM BAR"),
        ],
    ),
    (
        "TRANSFORM",
        &[
            (&[Cmd::RotateCw], "ROTATE CW"),
            (&[Cmd::RotateCcw], "ROTATE CCW"),
            (&[Cmd::Rotate180, Cmd::ResetRotation], "180° / RESET"),
            (&[Cmd::FlipH, Cmd::FlipV], "FLIP H / V"),
        ],
    ),
    (
        "FILE",
        &[
            (&[Cmd::OpenFile, Cmd::OpenFolder], "OPEN FILE / FOLDER"),
            (&[Cmd::CopyImage], "COPY IMAGE + FILE"),
            (&[Cmd::CopyResized], "COPY SMALL"),
            (&[Cmd::Paste], "VIEW CLIPBOARD"),
            (&[Cmd::Rename], "RENAME"),
            (&[Cmd::Trash, Cmd::DeletePermanently], "RECYCLE / DELETE"),
            (&[Cmd::Undo], "UNDO"),
            (&[Cmd::Setup], "SETTINGS"),
            (&[Cmd::Exit], "EXIT"),
        ],
    ),
    (
        "PHOTO FOLDERS",
        &[
            (&[Cmd::CopyToFolder(1)], "COPY TO FOLDER 1"),
            (&[Cmd::MoveToFolder(1)], "MOVE TO FOLDER 1"),
            (&[Cmd::CopyToFolder(2)], "COPY TO FOLDER 2"),
            (&[Cmd::CopyToFolder(3)], "COPY TO FOLDER 3"),
        ],
    ),
];

/// The active bindings.
#[derive(Clone)]
pub struct Keymap {
    pub bindings: Vec<(Binding, Cmd)>,
}

/// User changes: for each listed command, its complete new set of keys.
pub type Overrides = Vec<(String, Vec<String>)>;

impl Keymap {
    pub fn defaults() -> Self {
        Self {
            bindings: DEFAULTS
                .iter()
                .filter_map(|(k, c)| Binding::parse(k).map(|b| (b, *c)))
                .collect(),
        }
    }

    pub fn with_overrides(overrides: &Overrides) -> Self {
        let mut map = Self::defaults();
        for (id, keys) in overrides {
            let Some(cmd) = Cmd::from_id(id) else {
                continue;
            };
            let parsed: Vec<Binding> = keys.iter().filter_map(|k| Binding::parse(k)).collect();
            // A key belongs to one command: taking it removes it elsewhere.
            map.bindings
                .retain(|(b, c)| *c != cmd && !parsed.contains(b));
            map.bindings.extend(parsed.into_iter().map(|b| (b, cmd)));
        }
        map
    }

    pub fn lookup(&self, key: Key, mods: Modifiers) -> Option<Cmd> {
        let b = Binding::from_event(key, mods);
        if let Some((_, c)) = self.bindings.iter().find(|(x, _)| *x == b) {
            return Some(*c);
        }
        // `+` usually needs Shift (Shift+=); accept zoom keys either way.
        if matches!(key, Key::Plus | Key::Equals | Key::Minus) && !b.ctrl && !b.alt {
            let plain = Binding { shift: false, ..b };
            return self
                .bindings
                .iter()
                .find(|(x, _)| *x == plain)
                .map(|(_, c)| *c);
        }
        None
    }

    pub fn keys_for(&self, cmd: Cmd) -> Vec<Binding> {
        self.bindings
            .iter()
            .filter(|(_, c)| *c == cmd)
            .map(|(b, _)| *b)
            .collect()
    }

    /// Short key text for the help sheet: up to two keys per command.
    pub fn short_keys(&self, cmds: &[Cmd]) -> String {
        cmds.iter()
            .map(|c| {
                let keys: Vec<String> = self
                    .keys_for(*c)
                    .iter()
                    .take(2)
                    .map(Binding::display)
                    .collect();
                if keys.is_empty() {
                    "—".to_string()
                } else {
                    keys.join("  ")
                }
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }

    pub fn owner_of(&self, b: Binding) -> Option<Cmd> {
        self.bindings.iter().find(|(x, _)| *x == b).map(|(_, c)| *c)
    }
}

/// Puts `binding` on `cmd` (removing it from whatever had it) and returns
/// the new overrides plus the command that lost the key, if any.
pub fn assign(
    overrides: &Overrides,
    cmd: Cmd,
    binding: Binding,
    replace: bool,
) -> (Overrides, Option<Cmd>) {
    let current = Keymap::with_overrides(overrides);
    let previous_owner = current.owner_of(binding).filter(|c| *c != cmd);
    let mut keys: Vec<Binding> = if replace {
        Vec::new()
    } else {
        current.keys_for(cmd)
    };
    if !keys.contains(&binding) {
        keys.push(binding);
    }
    let mut out: Overrides = overrides
        .iter()
        .filter(|(id, _)| *id != cmd.id())
        .cloned()
        .collect();
    if let Some(owner) = previous_owner {
        let rest: Vec<String> = current
            .keys_for(owner)
            .into_iter()
            .filter(|b| *b != binding)
            .map(|b| b.label())
            .collect();
        out.retain(|(id, _)| *id != owner.id());
        out.push((owner.id(), rest));
    }
    out.push((cmd.id(), keys.iter().map(Binding::label).collect()));
    (out, previous_owner)
}

/// Removes the user's changes for one command.
pub fn reset(overrides: &Overrides, cmd: Cmd) -> Overrides {
    overrides
        .iter()
        .filter(|(id, _)| *id != cmd.id())
        .cloned()
        .collect()
}

/// Text form for export: `action = Key, Key` per line.
pub fn export(map: &Keymap) -> String {
    let mut out = String::from("# muud view keymap — edit and import from Settings → Keys\n");
    for cmd in ALL {
        let keys: Vec<String> = map.keys_for(*cmd).iter().map(Binding::label).collect();
        out.push_str(&format!("{} = {}\n", cmd.id(), keys.join(", ")));
    }
    out
}

/// Parses an exported keymap into overrides (every listed action is set).
pub fn import(text: &str) -> Overrides {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let (id, keys) = l.split_once('=')?;
            let id = id.trim();
            Cmd::from_id(id)?;
            let keys = keys
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty() && Binding::parse(k).is_some())
                .map(str::to_owned)
                .collect();
            Some((id.to_owned(), keys))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: false,
            command: ctrl,
        }
    }

    fn cmd(key: Key, ctrl: bool, shift: bool, alt: bool) -> Option<Cmd> {
        Keymap::defaults().lookup(key, mods(ctrl, shift, alt))
    }

    #[test]
    fn every_default_parses() {
        for (k, _) in DEFAULTS {
            assert!(Binding::parse(k).is_some(), "{k}");
        }
        assert_eq!(Keymap::defaults().bindings.len(), DEFAULTS.len());
    }

    #[test]
    fn defaults_have_no_duplicate_keys() {
        let map = Keymap::defaults();
        for (i, (b, _)) in map.bindings.iter().enumerate() {
            assert!(
                !map.bindings[i + 1..].iter().any(|(x, _)| x == b),
                "{}",
                b.label()
            );
        }
    }

    #[test]
    fn ids_round_trip() {
        for c in ALL {
            assert_eq!(Cmd::from_id(&c.id()), Some(*c), "{}", c.id());
        }
        assert_eq!(Cmd::CopyToFolder(3).id(), "copy_to_folder_3");
        assert_eq!(Cmd::ToggleInfo.id(), "toggle_info");
    }

    #[test]
    fn exact_modifiers() {
        assert_eq!(cmd(Key::C, true, false, false), Some(Cmd::CopyImage));
        assert_eq!(cmd(Key::C, true, true, false), Some(Cmd::CopyExif));
        assert_eq!(cmd(Key::C, true, true, true), Some(Cmd::CopyPath));
        assert_eq!(cmd(Key::C, true, false, true), Some(Cmd::CopyResized));
        // Plain C is a different command (compare), not a copy.
        assert_eq!(cmd(Key::C, false, false, false), Some(Cmd::Compare));
    }

    #[test]
    fn familiar_defaults() {
        assert_eq!(cmd(Key::Space, false, false, false), Some(Cmd::Next));
        assert_eq!(cmd(Key::Space, false, true, false), Some(Cmd::Prev));
        assert_eq!(
            cmd(Key::Insert, true, false, false),
            Some(Cmd::MoveToFolder(1))
        );
        assert_eq!(
            cmd(Key::Num7, false, false, true),
            Some(Cmd::CopyToFolder(7))
        );
        assert_eq!(cmd(Key::Num3, true, false, false), Some(Cmd::Slideshow(3)));
        assert_eq!(cmd(Key::Backtick, false, true, false), Some(Cmd::PinBottom));
        assert_eq!(cmd(Key::PageDown, true, false, true), Some(Cmd::RandomNext));
    }

    #[test]
    fn zoom_ignores_shift() {
        assert_eq!(cmd(Key::Plus, false, true, false), Some(Cmd::ZoomIn));
        assert_eq!(cmd(Key::Equals, false, false, false), Some(Cmd::ZoomIn));
        assert_eq!(cmd(Key::Minus, false, false, false), Some(Cmd::ZoomOut));
    }

    #[test]
    fn overrides_move_keys_between_commands() {
        // Put Delete on "copy to folder 1".
        let (ov, lost) = assign(
            &Vec::new(),
            Cmd::CopyToFolder(1),
            Binding::parse("Delete").unwrap(),
            false,
        );
        assert_eq!(lost, Some(Cmd::Trash));
        let map = Keymap::with_overrides(&ov);
        assert_eq!(
            map.lookup(Key::Delete, mods(false, false, false)),
            Some(Cmd::CopyToFolder(1))
        );
        assert!(map.keys_for(Cmd::Trash).is_empty());
        // Insert still works for folder 1 because we added, not replaced.
        assert_eq!(
            map.lookup(Key::Insert, mods(false, false, false)),
            Some(Cmd::CopyToFolder(1))
        );
        // Reset brings the default back for folder 1 only.
        let map = Keymap::with_overrides(&reset(&ov, Cmd::CopyToFolder(1)));
        assert_eq!(map.lookup(Key::Delete, mods(false, false, false)), None);
    }

    #[test]
    fn export_import_round_trip() {
        let (ov, _) = assign(&Vec::new(), Cmd::Next, Binding::parse("J").unwrap(), true);
        let map = Keymap::with_overrides(&ov);
        let text = export(&map);
        let again = Keymap::with_overrides(&import(&text));
        for c in ALL {
            let mut a = map.keys_for(*c);
            let mut b = again.keys_for(*c);
            a.sort_by_key(|x| x.label());
            b.sort_by_key(|x| x.label());
            assert_eq!(a, b, "{}", c.id());
        }
        assert_eq!(
            again.lookup(Key::J, mods(false, false, false)),
            Some(Cmd::Next)
        );
        assert_eq!(again.lookup(Key::Space, mods(false, false, false)), None);
    }

    #[test]
    fn display_uses_symbols() {
        assert_eq!(Binding::parse("Shift+Backtick").unwrap().display(), "~");
        assert_eq!(Binding::parse("Alt+Right").unwrap().display(), "ALT+→");
        assert_eq!(
            Binding::parse("Ctrl+PageDown").unwrap().display(),
            "CTRL+PGDN"
        );
    }

    #[test]
    fn labels() {
        assert_eq!(
            Binding::parse("ctrl+shift+c").unwrap().label(),
            "Ctrl+Shift+C"
        );
        assert_eq!(Binding::parse("Alt+Right").unwrap().label(), "Alt+Right");
        assert!(Binding::parse("Hyper+Q").is_none());
    }
}
