//! Settings that persist between runs.

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Background {
    Ink,
    Paper,
    Gray,
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
        }
    }
}
