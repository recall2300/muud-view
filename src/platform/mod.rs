//! OS integration: file dialogs, Explorer, Recycle Bin, clipboard.

pub mod clipboard;
pub mod drag;
pub mod integration;
pub mod single_instance;

use std::path::{Path, PathBuf};

use crate::core::folder::IMAGE_EXTENSIONS;

#[cfg(windows)]
pub fn pick_file(start: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new()
        .set_title("Open image")
        .add_filter("Images", IMAGE_EXTENSIONS)
        .add_filter(
            "Archives (zip, cbz)",
            crate::core::archive::ARCHIVE_EXTENSIONS,
        )
        .add_filter("Camera RAW", crate::core::formats::RAW_EXTENSIONS)
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

#[cfg(windows)]
fn wide_z(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Runs a shell verb on a file: "edit" opens the default image editor,
/// "print" the Windows photo printing dialog.
pub fn shell_verb(path: &Path, verb: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb_w = wide_z(std::ffi::OsStr::new(verb));
        let file_w = wide_z(path.as_os_str());
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb_w.as_ptr(),
                file_w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 mean success.
        if r as usize > 32 {
            Ok(())
        } else {
            Err(format!("no program for \"{verb}\" (code {})", r as usize))
        }
    }
    #[cfg(not(windows))]
    {
        if verb == "edit" {
            std::process::Command::new("xdg-open")
                .arg(path)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            Err("not supported on this system".into())
        }
    }
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        shell_verb(Path::new(url), "open")
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Makes `path` (a JPEG/PNG/BMP) the desktop wallpaper.
pub fn set_wallpaper(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPIF_SENDWININICHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
        };
        let mut file_w = wide_z(path.as_os_str());
        // SAFETY: NUL-terminated path buffer, alive for the call.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                file_w.as_mut_ptr().cast(),
                SPIF_UPDATEINIFILE | SPIF_SENDWININICHANGE,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("not supported on this system".into())
    }
}

/// Picks a program (for the external editor setting).
#[cfg(windows)]
pub fn pick_program() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Choose an image editor")
        .add_filter("Programs", &["exe"])
        .pick_file()
}

#[cfg(not(windows))]
pub fn pick_program() -> Option<PathBuf> {
    None
}

pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

pub fn delete_permanently(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

/// The user's Pictures folder (follows OneDrive or other redirection on Windows).
#[cfg(windows)]
pub fn pictures_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath};
    let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: valid GUID pointer and out-pointer; the returned string is
    // freed with CoTaskMemFree as documented, even on failure.
    unsafe {
        let hr = SHGetKnownFolderPath(&FOLDERID_Pictures, 0, std::ptr::null_mut(), &mut raw);
        let path = if hr >= 0 && !raw.is_null() {
            let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
            let wide = std::slice::from_raw_parts(raw, len);
            Some(PathBuf::from(std::ffi::OsString::from_wide(wide)))
        } else {
            None
        };
        CoTaskMemFree(raw as *const _);
        path.or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Pictures")))
    }
}

#[cfg(not(windows))]
pub fn pictures_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_PICTURES_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures")))
}
