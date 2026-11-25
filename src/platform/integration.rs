//! Windows integration: "Open with" entries, default-app registration,
//! Explorer context menus and a Start menu shortcut. Everything goes under
//! HKEY_CURRENT_USER, so no administrator rights are needed, and
//! `unregister` removes all of it.
//!
//! Windows 10/11 does not let programs change the default app themselves;
//! muud view registers itself as a candidate and then opens the Default
//! apps page, where the user makes the final choice.

// The registry plan is only executed on Windows but is tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

/// The one ProgID of versions before 0.8 (now the generic image type's).
pub const LEGACY_PROG_ID: &str = "muudview.image";
pub const APP_NAME: &str = "muud view";
const APP_KEY: &str = r"Software\muud-view";
const CAPABILITIES: &str = r"Software\muud-view\Capabilities";

pub use super::file_types::{FileType, FILE_TYPES};

/// The ProgID of a file type (`muudview.jpg`, ...). The generic "image"
/// type keeps the ProgID earlier versions used for everything.
pub fn prog_id(t: &FileType) -> String {
    format!("muudview.{}", t.icon)
}

/// Every extension registered for "Open with" and default-app selection.
pub fn extensions() -> impl Iterator<Item = (&'static str, &'static FileType)> {
    FILE_TYPES
        .iter()
        .flat_map(|t| t.extensions.iter().map(move |e| (*e, t)))
}

/// One registry write under HKEY_CURRENT_USER. `name: None` is the default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    pub key: String,
    pub name: Option<String>,
    pub data: String,
}

fn v(key: impl Into<String>, name: Option<&str>, data: impl Into<String>) -> RegValue {
    RegValue {
        key: key.into(),
        name: name.map(str::to_owned),
        data: data.into(),
    }
}

/// Everything `register` writes.
pub fn plan(exe: &str) -> Vec<RegValue> {
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    let classes = r"Software\Classes";
    let mut out = Vec::new();
    // One file type per format, each with its own Explorer icon.
    for t in FILE_TYPES {
        let id = prog_id(t);
        out.push(v(
            format!(r"{classes}\{id}"),
            None,
            format!("{} (muud view)", t.name),
        ));
        out.push(v(
            format!(r"{classes}\{id}\DefaultIcon"),
            None,
            format!("\"{exe}\",-{}", t.icon_id),
        ));
        out.push(v(
            format!(r"{classes}\{id}\shell\open"),
            Some("FriendlyAppName"),
            APP_NAME,
        ));
        out.push(v(
            format!(r"{classes}\{id}\shell\open\command"),
            None,
            open.clone(),
        ));
    }
    out.extend([
        // "Open with" → muud view.
        v(
            format!(r"{classes}\Applications\muud-view.exe"),
            Some("FriendlyAppName"),
            APP_NAME,
        ),
        v(
            format!(r"{classes}\Applications\muud-view.exe\shell\open\command"),
            None,
            open.clone(),
        ),
        // Shown on Settings → Default apps.
        v(CAPABILITIES, Some("ApplicationName"), APP_NAME),
        v(
            CAPABILITIES,
            Some("ApplicationDescription"),
            "Fast, quiet image viewer",
        ),
        v(CAPABILITIES, Some("ApplicationIcon"), icon.clone()),
        v(
            r"Software\RegisteredApplications",
            Some(APP_NAME),
            CAPABILITIES,
        ),
        // Right-click an image: "Open with muud view".
        v(
            format!(r"{classes}\SystemFileAssociations\image\shell\muudview"),
            None,
            "muud view로 열기",
        ),
        v(
            format!(r"{classes}\SystemFileAssociations\image\shell\muudview"),
            Some("Icon"),
            icon.clone(),
        ),
        v(
            format!(r"{classes}\SystemFileAssociations\image\shell\muudview\command"),
            None,
            open.clone(),
        ),
        // Right-click a folder (or inside one): "View folder in muud view".
        v(
            format!(r"{classes}\Directory\shell\muudview"),
            None,
            "muud view로 폴더 보기",
        ),
        v(
            format!(r"{classes}\Directory\shell\muudview"),
            Some("Icon"),
            icon.clone(),
        ),
        v(
            format!(r"{classes}\Directory\shell\muudview\command"),
            None,
            open,
        ),
        v(
            format!(r"{classes}\Directory\Background\shell\muudview"),
            None,
            "muud view로 폴더 보기",
        ),
        v(
            format!(r"{classes}\Directory\Background\shell\muudview"),
            Some("Icon"),
            icon,
        ),
        v(
            format!(r"{classes}\Directory\Background\shell\muudview\command"),
            None,
            format!("\"{exe}\" \"%V\""),
        ),
    ]);
    for (ext, t) in extensions() {
        let id = prog_id(t);
        out.push(v(
            format!(r"{classes}\.{ext}\OpenWithProgids"),
            Some(&id),
            "",
        ));
        out.push(v(
            format!(r"{classes}\Applications\muud-view.exe\SupportedTypes"),
            Some(&format!(".{ext}")),
            "",
        ));
        out.push(v(
            format!(r"{CAPABILITIES}\FileAssociations"),
            Some(&format!(".{ext}")),
            id,
        ));
    }
    out
}

/// Keys `unregister` deletes entirely.
pub fn owned_trees() -> Vec<String> {
    let classes = r"Software\Classes";
    let mut out: Vec<String> = FILE_TYPES
        .iter()
        .map(|t| format!(r"{classes}\{}", prog_id(t)))
        .collect();
    out.extend([
        format!(r"{classes}\Applications\muud-view.exe"),
        format!(r"{classes}\SystemFileAssociations\image\shell\muudview"),
        format!(r"{classes}\Directory\shell\muudview"),
        format!(r"{classes}\Directory\Background\shell\muudview"),
        APP_KEY.to_string(),
    ]);
    out
}

/// Single values `unregister` removes from keys shared with other programs.
pub fn shared_values() -> Vec<(String, String)> {
    let mut out = vec![(
        r"Software\RegisteredApplications".to_string(),
        APP_NAME.to_string(),
    )];
    for (ext, t) in extensions() {
        out.push((
            format!(r"Software\Classes\.{ext}\OpenWithProgids"),
            prog_id(t),
        ));
    }
    out.extend(stale_values());
    out
}

/// What versions before per-format icons wrote that the current layout no
/// longer uses: every extension pointed at one ProgID, `muudview.image`.
/// Removed when registering, so "Open with" does not list muud view twice.
pub fn stale_values() -> Vec<(String, String)> {
    extensions()
        .filter(|(_, t)| prog_id(t) != LEGACY_PROG_ID)
        .map(|(ext, _)| {
            (
                format!(r"Software\Classes\.{ext}\OpenWithProgids"),
                LEGACY_PROG_ID.to_string(),
            )
        })
        .collect()
}

pub const DEFAULT_APPS_URI: &str = "ms-settings:defaultapps?registeredAppUser=muud%20view";

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set(value: &RegValue) -> Result<(), String> {
        let key = wide(&value.key);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: NUL-terminated strings; the key handle is closed below.
        unsafe {
            let rc = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                std::ptr::null(),
                &mut h,
                std::ptr::null_mut(),
            );
            if rc != 0 {
                return Err(format!("cannot create {} ({rc})", value.key));
            }
            let name = value.name.as_deref().map(wide);
            let data = wide(&value.data);
            let rc = RegSetValueExW(
                h,
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * 2) as u32,
            );
            RegCloseKey(h);
            if rc != 0 {
                return Err(format!("cannot write {} ({rc})", value.key));
            }
        }
        Ok(())
    }

    fn notify_shell() {
        // SAFETY: documented way to refresh file associations.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
    }

    pub fn register() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // SAFETY: NUL-terminated strings; missing values are fine.
        unsafe {
            for (key, name) in stale_values() {
                let (k, n) = (wide(&key), wide(&name));
                RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr());
            }
        }
        for value in plan(&exe.display().to_string()) {
            set(&value)?;
        }
        notify_shell();
        Ok(())
    }

    pub fn unregister() -> Result<(), String> {
        // SAFETY: NUL-terminated strings; missing keys are fine.
        unsafe {
            for key in owned_trees() {
                let k = wide(&key);
                RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr());
            }
            for (key, name) in shared_values() {
                let (k, n) = (wide(&key), wide(&name));
                RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr());
            }
        }
        let _ = std::fs::remove_file(start_menu_shortcut());
        notify_shell();
        Ok(())
    }

    fn start_menu_shortcut() -> std::path::PathBuf {
        let appdata = std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        appdata.join(r"Microsoft\Windows\Start Menu\Programs\muud view.lnk")
    }

    /// Creates a Start menu shortcut through the Windows Script Host.
    pub fn add_start_menu_shortcut() -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let lnk = start_menu_shortcut();
        let q = |s: &str| s.replace('\'', "''");
        let script = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Description='muud view';$s.Save()",
            q(&lnk.display().to_string()),
            q(&exe.display().to_string()),
            q(&exe.parent().map(|p| p.display().to_string()).unwrap_or_default()),
        );
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() && lnk.exists() {
            Ok(())
        } else {
            Err("PowerShell could not create the shortcut".into())
        }
    }

    pub fn open_default_apps() -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd.exe")
            .args(["/C", "start", "", DEFAULT_APPS_URI])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn is_registered() -> bool {
        use windows_sys::Win32::System::Registry::{RegOpenKeyExW, KEY_READ};
        let k = wide(CAPABILITIES);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: read-only probe; handle closed if opened.
        unsafe {
            let ok = RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_READ, &mut h) == 0;
            if ok {
                RegCloseKey(h);
            }
            ok
        }
    }
}

#[cfg(windows)]
pub use imp::{add_start_menu_shortcut, is_registered, open_default_apps, register, unregister};

#[cfg(not(windows))]
mod imp {
    const MSG: &str = "Windows only";
    pub fn register() -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn unregister() -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn add_start_menu_shortcut() -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn open_default_apps() -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn is_registered() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub use imp::{add_start_menu_shortcut, is_registered, open_default_apps, register, unregister};

/// Portable mode: a file named `portable` (or `portable.txt`) next to the
/// exe keeps settings and logs in a `muud-view-data` folder beside it.
pub fn portable_data_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    ["portable", "portable.txt"]
        .iter()
        .any(|n| dir.join(n).exists())
        .then(|| dir.join("muud-view-data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_covers_every_extension_and_menu() {
        let p = plan(r"C:\Tools\muud-view.exe");
        for (ext, t) in extensions() {
            let id = prog_id(t);
            assert!(p.iter().any(
                |v| v.key == format!(r"Software\Classes\.{ext}\OpenWithProgids")
                    && v.name.as_deref() == Some(id.as_str())
            ));
            assert!(p
                .iter()
                .any(|v| v.key.ends_with(r"Capabilities\FileAssociations")
                    && v.name.as_deref() == Some(&format!(".{ext}"))
                    && v.data == id));
        }
        let cmd = p
            .iter()
            .find(|v| v.key == r"Software\Classes\muudview.jpg\shell\open\command")
            .unwrap();
        assert_eq!(cmd.data, r#""C:\Tools\muud-view.exe" "%1""#);
        // Each type shows its own icon from the exe.
        let icon = p
            .iter()
            .find(|v| v.key == r"Software\Classes\muudview.avif\DefaultIcon")
            .unwrap();
        assert_eq!(icon.data, r#""C:\Tools\muud-view.exe",-105"#);
        assert!(p.iter().any(|v| v
            .key
            .contains(r"Directory\Background\shell\muudview\command")
            && v.data.ends_with("\"%V\"")));
    }

    #[test]
    fn unregister_covers_what_register_writes() {
        let trees = owned_trees();
        let shared = shared_values();
        for value in plan(r"C:\x\muud-view.exe") {
            let removed_by_tree = trees
                .iter()
                .any(|t| value.key == *t || value.key.starts_with(&format!(r"{t}\")));
            let removed_as_value = shared
                .iter()
                .any(|(k, n)| *k == value.key && Some(n.as_str()) == value.name.as_deref());
            assert!(
                removed_by_tree || removed_as_value,
                "left behind: {value:?}"
            );
        }
    }

    #[test]
    fn file_types_are_consistent() {
        let mut ids = std::collections::HashSet::new();
        let mut exts = std::collections::HashSet::new();
        for t in FILE_TYPES {
            assert!(ids.insert(t.icon_id), "icon id {} twice", t.icon_id);
            let ico = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/icons")
                .join(format!("{}.ico", t.icon));
            assert!(ico.exists(), "{}", ico.display());
            for e in t.extensions {
                assert!(exts.insert(*e), ".{e} in two types");
                assert!(
                    crate::core::folder::is_image(std::path::Path::new(&format!("a.{e}")))
                        || *e == "cbz",
                    ".{e} registered but not opened"
                );
            }
        }
        // The generic type keeps the old ProgID; old entries elsewhere go.
        assert!(FILE_TYPES.iter().any(|t| prog_id(t) == LEGACY_PROG_ID));
        let stale = stale_values();
        assert!(stale
            .iter()
            .any(|(k, _)| k.ends_with(r".jpg\OpenWithProgids")));
        assert!(!stale
            .iter()
            .any(|(k, _)| k.ends_with(r".bmp\OpenWithProgids")));
    }

    #[test]
    fn writes_only_under_current_user_software() {
        for value in plan(r"C:\x\muud-view.exe") {
            assert!(value.key.starts_with(r"Software\"), "{}", value.key);
        }
    }
}
