//! Key bindings. Modifiers are matched exactly, so `Ctrl+C` and
//! `Ctrl+Shift+C` never trigger each other.

use eframe::egui::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    FitWindow,
    FitWidth,
    Original,
    ZoomIn,
    ZoomOut,
    StretchSmall,
    Fullscreen,
    Maximize,
    AlwaysOnTop,
    SmoothOn,
    SmoothOff,
    CycleBackground,
    RotateCw,
    RotateCcw,
    Rotate180,
    ResetRotation,
    FlipH,
    FlipV,
    ToggleInfo,
    CopyImage,
    CopyPath,
    CopyExif,
    ShowInExplorer,
    Trash,
    DeletePermanently,
    Slideshow(u8),
    StopSlideshow,
    Help,
    PinTop,
    PinBottom,
    CopyResized,
    Setup,
    Paste,
    CopyToFolder(u8),
    MoveToFolder(u8),
    Undo,
}

#[derive(Clone, Copy, Default)]
struct Mods {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
};
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};
const ALT: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: true,
};
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    alt: false,
};
const CTRL_ALT: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: true,
};
const CTRL_ALT_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    alt: true,
};

pub fn command_for(key: Key, mods: Modifiers) -> Option<Cmd> {
    use Cmd::*;
    use Key as K;
    let m = Mods {
        ctrl: mods.ctrl || mods.command || mods.mac_cmd,
        shift: mods.shift,
        alt: mods.alt,
    };
    let is = |p: Mods| m.ctrl == p.ctrl && m.shift == p.shift && m.alt == p.alt;

    // `+` usually needs Shift (Shift+=), so ignore Shift for the zoom keys.
    if !m.ctrl && !m.alt && matches!(key, K::Plus | K::Equals) {
        return Some(ZoomIn);
    }
    if !m.ctrl && !m.alt && key == K::Minus {
        return Some(ZoomOut);
    }

    let cmd = if is(NONE) {
        match key {
            K::F2 => OpenFile,
            K::F => OpenFolder,
            K::F4 => CloseFile,
            K::X => Exit,
            K::Escape => Escape,
            K::PageDown | K::ArrowRight | K::ArrowDown | K::Space => Next,
            K::PageUp | K::ArrowLeft | K::ArrowUp | K::Backspace => Prev,
            K::Home => First,
            K::End => Last,
            K::Num9 | K::Num1 => FitWindow,
            K::Num8 => FitWidth,
            K::Num0 => Original,
            K::Z => StretchSmall,
            K::Enter | K::F11 | K::N => Fullscreen,
            K::M => Maximize,
            K::I => SmoothOn,
            K::U => SmoothOff,
            K::B => CycleBackground,
            K::Tab => ToggleInfo,
            K::Delete => Trash,
            K::Insert => CopyToFolder(1),
            K::F1 => Help,
            K::F5 => Setup,
            K::Backtick => PinTop,
            _ => return None,
        }
    } else if is(CTRL) {
        match key {
            K::O => OpenFile,
            K::W => Exit,
            K::PageDown => Forward10,
            K::PageUp => Back10,
            K::A => AlwaysOnTop,
            K::R => RotateCw,
            K::L => RotateCcw,
            K::M => FlipH,
            K::F => FlipV,
            K::C => CopyImage,
            K::V => Paste,
            K::Enter => ShowInExplorer,
            K::Insert => MoveToFolder(1),
            K::Z => Undo,
            K::Num0 => StopSlideshow,
            K::Num1 => Slideshow(1),
            K::Num2 => Slideshow(2),
            K::Num3 => Slideshow(3),
            K::Num4 => Slideshow(4),
            K::Num5 => Slideshow(5),
            K::Num6 => Slideshow(6),
            K::Num7 => Slideshow(7),
            K::Num8 => Slideshow(8),
            K::Num9 => Slideshow(9),
            _ => return None,
        }
    } else if is(SHIFT) {
        match key {
            K::Space => Prev,
            K::Delete => DeletePermanently,
            K::Insert => CopyToFolder(2),
            K::Num8 => Original,      // `*` on a US keyboard
            K::Backtick => PinBottom, // `~`
            _ => return None,
        }
    } else if is(ALT) {
        match key {
            K::Enter => Fullscreen,
            K::ArrowRight => RotateCw,
            K::ArrowLeft => RotateCcw,
            K::ArrowDown => Rotate180,
            K::ArrowUp => ResetRotation,
            K::Num1 => CopyToFolder(1),
            K::Num2 => CopyToFolder(2),
            K::Num3 => CopyToFolder(3),
            K::Num4 => CopyToFolder(4),
            K::Num5 => CopyToFolder(5),
            K::Num6 => CopyToFolder(6),
            K::Num7 => CopyToFolder(7),
            K::Num8 => CopyToFolder(8),
            K::Num9 => CopyToFolder(9),
            _ => return None,
        }
    } else if is(CTRL_SHIFT) {
        match key {
            K::C => CopyExif,
            _ => return None,
        }
    } else if is(CTRL_ALT) {
        match key {
            K::C => CopyResized,
            _ => return None,
        }
    } else if is(CTRL_ALT_SHIFT) {
        match key {
            K::C => CopyPath,
            _ => return None,
        }
    } else {
        return None;
    };
    Some(cmd)
}

/// A titled group of (keys, action) rows.
pub type HelpSection = (&'static str, &'static [(&'static str, &'static str)]);

/// Rows for the F1 help sheet.
pub const HELP: &[HelpSection] = &[
    (
        "MOVE",
        &[
            ("PGDN  RIGHT  SPACE  WHEEL", "NEXT"),
            ("PGUP  LEFT  BKSP", "PREVIOUS"),
            ("HOME / END", "FIRST / LAST"),
            ("CTRL+PGDN / PGUP", "± 10 IMAGES"),
            ("CTRL+1…9 / CTRL+0", "SLIDESHOW / STOP"),
        ],
    ),
    (
        "VIEW",
        &[
            ("9  1", "FIT WINDOW"),
            ("8", "FIT WIDTH"),
            ("0  SHIFT+8", "ORIGINAL 100%"),
            ("+  −  CTRL+WHEEL", "ZOOM"),
            ("Z", "STRETCH SMALL"),
            ("ENTER  F11", "FULLSCREEN"),
            ("I / U", "SMOOTH / PIXEL"),
            ("B", "BACKGROUND"),
            ("TAB", "INFO PANEL"),
            ("` / ~", "PIN TOP / BOTTOM BAR"),
        ],
    ),
    (
        "TRANSFORM",
        &[
            ("CTRL+R  ALT+RIGHT", "ROTATE CW"),
            ("CTRL+L  ALT+LEFT", "ROTATE CCW"),
            ("ALT+DOWN / ALT+UP", "180° / RESET"),
            ("CTRL+M / CTRL+F", "FLIP H / V"),
        ],
    ),
    (
        "PHOTO FOLDERS",
        &[
            ("INSERT", "COPY TO FOLDER 1"),
            ("CTRL+INSERT", "MOVE TO FOLDER 1"),
            ("SHIFT+INSERT", "COPY TO FOLDER 2"),
            ("ALT+1…9", "COPY TO FOLDER 1–9"),
        ],
    ),
    (
        "FILE",
        &[
            ("CTRL+O  F2 / F", "OPEN FILE / FOLDER"),
            ("CTRL+C", "COPY IMAGE + FILE"),
            ("CTRL+ALT+C", "COPY SMALL (1600 PX)"),
            ("CTRL+V", "VIEW CLIPBOARD"),
            ("CTRL+SHIFT+C", "COPY EXIF"),
            ("CTRL+ALT+SHIFT+C", "COPY PATH"),
            ("CTRL+ENTER", "SHOW IN EXPLORER"),
            ("DEL / SHIFT+DEL", "RECYCLE / DELETE"),
            ("CTRL+Z", "UNDO MOVE / DELETE"),
            ("F5", "SETUP · WINDOWS"),
            ("ESC  X  CTRL+W", "EXIT"),
        ],
    ),
];

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

    #[test]
    fn exact_modifiers() {
        assert_eq!(
            command_for(Key::C, mods(true, false, false)),
            Some(Cmd::CopyImage)
        );
        assert_eq!(
            command_for(Key::C, mods(true, true, false)),
            Some(Cmd::CopyExif)
        );
        assert_eq!(
            command_for(Key::C, mods(true, true, true)),
            Some(Cmd::CopyPath)
        );
        assert_eq!(command_for(Key::C, mods(false, false, false)), None);
    }

    #[test]
    fn navigation() {
        assert_eq!(
            command_for(Key::Space, mods(false, false, false)),
            Some(Cmd::Next)
        );
        assert_eq!(
            command_for(Key::Space, mods(false, true, false)),
            Some(Cmd::Prev)
        );
        assert_eq!(
            command_for(Key::PageDown, mods(true, false, false)),
            Some(Cmd::Forward10)
        );
        assert_eq!(
            command_for(Key::ArrowRight, mods(false, false, true)),
            Some(Cmd::RotateCw)
        );
    }

    #[test]
    fn zoom_ignores_shift() {
        assert_eq!(
            command_for(Key::Plus, mods(false, true, false)),
            Some(Cmd::ZoomIn)
        );
        assert_eq!(
            command_for(Key::Equals, mods(false, false, false)),
            Some(Cmd::ZoomIn)
        );
        assert_eq!(
            command_for(Key::Minus, mods(false, false, false)),
            Some(Cmd::ZoomOut)
        );
    }

    #[test]
    fn photo_folder_keys() {
        assert_eq!(
            command_for(Key::Insert, mods(false, false, false)),
            Some(Cmd::CopyToFolder(1))
        );
        assert_eq!(
            command_for(Key::Insert, mods(true, false, false)),
            Some(Cmd::MoveToFolder(1))
        );
        assert_eq!(
            command_for(Key::Insert, mods(false, true, false)),
            Some(Cmd::CopyToFolder(2))
        );
        assert_eq!(
            command_for(Key::Num7, mods(false, false, true)),
            Some(Cmd::CopyToFolder(7))
        );
        assert_eq!(
            command_for(Key::Z, mods(true, false, false)),
            Some(Cmd::Undo)
        );
        assert_eq!(
            command_for(Key::Z, mods(false, false, false)),
            Some(Cmd::StretchSmall)
        );
    }

    #[test]
    fn pin_keys() {
        assert_eq!(
            command_for(Key::Backtick, mods(false, false, false)),
            Some(Cmd::PinTop)
        );
        assert_eq!(
            command_for(Key::Backtick, mods(false, true, false)),
            Some(Cmd::PinBottom)
        );
    }

    #[test]
    fn slideshow_keys() {
        assert_eq!(
            command_for(Key::Num3, mods(true, false, false)),
            Some(Cmd::Slideshow(3))
        );
        assert_eq!(
            command_for(Key::Num0, mods(true, false, false)),
            Some(Cmd::StopSlideshow)
        );
        assert_eq!(
            command_for(Key::Num0, mods(false, false, false)),
            Some(Cmd::Original)
        );
    }
}
