//! OS integration: file dialogs, Explorer, Recycle Bin.

use std::path::{Path, PathBuf};

use crate::folder::IMAGE_EXTENSIONS;

#[cfg(windows)]
pub fn pick_file(start: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new()
        .set_title("Open image")
        .add_filter("Images", IMAGE_EXTENSIONS)
        .add_filter("All files", &["*"]);
    if let Some(dir) = start {
        d = d.set_directory(dir);
    }
    d.pick_file()
}

#[cfg(windows)]
pub fn pick_folder(start: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title("Open folder");
    if let Some(dir) = start {
        d = d.set_directory(dir);
    }
    d.pick_folder()
}

// Windows is the target platform. Elsewhere, files are opened from the
// command line or by drag & drop.
#[cfg(not(windows))]
pub fn pick_file(_start: Option<&Path>) -> Option<PathBuf> {
    let _ = IMAGE_EXTENSIONS;
    None
}

#[cfg(not(windows))]
pub fn pick_folder(_start: Option<&Path>) -> Option<PathBuf> {
    None
}

pub fn dialogs_available() -> bool {
    cfg!(windows)
}

/// Opens Explorer with the file selected.
pub fn show_in_explorer(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer.exe parses its own command line; the path must be quoted as one argument.
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open").arg(dir).spawn()?;
        Ok(())
    }
}

pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

pub fn delete_permanently(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| e.to_string())
}
