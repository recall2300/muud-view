//! Settings that persist between runs.

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Background {
    Ink,
    Paper,
    Gray,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelMode {
    Navigate,
    Zoom,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseAction {
    None,
    Fullscreen,
    Maximize,
    Original,
    FitWindow,
}

/// What is drawn over the image when the top bar is hidden.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osd {
    Off,
    Name,
    Full,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortBy {
    Name,
    Modified,
    Size,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ko,
    En,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub background: Background,
    pub smooth: bool,
    pub show_info: bool,
    pub stretch_small: bool,
    /// Keep the top bar visible instead of hiding it while the mouse is away.
    pub pin_top: bool,
    /// Keep the bottom bar visible instead of hiding it while the mouse is away.
    pub pin_bottom: bool,
    /// Photo folders 1–9; `None` means `Pictures\\muud-view\\<n>`.
    pub photo_folders: Vec<Option<std::path::PathBuf>>,
    pub key_overrides: crate::core::keymap::Overrides,
    pub wheel: WheelMode,
    pub middle_click: MouseAction,
    pub double_click: MouseAction,
    /// Mouse back/forward buttons move to the previous/next image.
    pub side_buttons: bool,
    /// Scrolling past the bottom of a tall image moves to the next one.
    pub wheel_edge_next: bool,
    pub cursor_autohide: bool,
    pub osd: Osd,
    /// Reopening a folder starts at the image you last looked at.
    pub resume: bool,
    /// (folder, file name) pairs, newest last.
    pub last_positions: Vec<(String, String)>,
    pub recent_folders: Vec<String>,
    pub favorite_folders: Vec<String>,
    pub sort: SortBy,
    pub sort_desc: bool,
    pub lang: Lang,
    pub dark: bool,
    /// Convert images with an embedded ICC profile to sRGB.
    pub color_manage: bool,
    /// Show where you are in a zoomed-in image (`/`).
    pub minimap: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background: Background::Ink,
            smooth: true,
            show_info: false,
            stretch_small: false,
            pin_top: true,
            pin_bottom: false,
            photo_folders: vec![None; crate::core::photo_folders::SLOTS],
            key_overrides: Vec::new(),
            wheel: WheelMode::Navigate,
            middle_click: MouseAction::Fullscreen,
            double_click: MouseAction::Maximize,
            side_buttons: true,
            wheel_edge_next: true,
            cursor_autohide: true,
            osd: Osd::Name,
            resume: true,
            last_positions: Vec::new(),
            recent_folders: Vec::new(),
            favorite_folders: Vec::new(),
            sort: SortBy::Name,
            sort_desc: false,
            lang: Lang::Ko,
            dark: false,
            color_manage: true,
            minimap: true,
        }
    }
}

impl Settings {
    /// Remembers where we were in `folder`.
    pub fn remember_position(&mut self, folder: &str, file: &str) {
        self.last_positions.retain(|(f, _)| f != folder);
        self.last_positions
            .push((folder.to_owned(), file.to_owned()));
        let n = self.last_positions.len();
        if n > 200 {
            self.last_positions.drain(..n - 200);
        }
    }

    pub fn position_in(&self, folder: &str) -> Option<&str> {
        self.last_positions
            .iter()
            .rev()
            .find(|(f, _)| f == folder)
            .map(|(_, n)| n.as_str())
    }

    pub fn add_recent(&mut self, folder: &str) {
        self.recent_folders.retain(|f| f != folder);
        self.recent_folders.insert(0, folder.to_owned());
        self.recent_folders.truncate(12);
    }

    pub fn toggle_favorite(&mut self, folder: &str) -> bool {
        if let Some(i) = self.favorite_folders.iter().position(|f| f == folder) {
            self.favorite_folders.remove(i);
            false
        } else {
            self.favorite_folders.push(folder.to_owned());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_and_recents() {
        let mut s = Settings::default();
        s.remember_position("C:/a", "1.jpg");
        s.remember_position("C:/b", "2.jpg");
        s.remember_position("C:/a", "3.jpg");
        assert_eq!(s.position_in("C:/a"), Some("3.jpg"));
        assert_eq!(s.last_positions.len(), 2);
        for i in 0..20 {
            s.add_recent(&format!("F{i}"));
        }
        s.add_recent("F5");
        assert_eq!(s.recent_folders[0], "F5");
        assert_eq!(s.recent_folders.len(), 12);
        assert!(s.toggle_favorite("C:/a"));
        assert!(!s.toggle_favorite("C:/a"));
        assert!(s.favorite_folders.is_empty());
    }
}
