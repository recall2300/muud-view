//! Dragging the current file out of the window — into a mail, a messenger,
//! Explorer or another program.
//!
//! On Windows the shell builds the data object for the file (the same one
//! Explorer uses) and runs the drag with its own drag image and cursor, so
//! every drop target that accepts files from Explorer accepts ours.

use std::path::Path;

/// Runs a drag of `path` until it is dropped or cancelled. Returns true if
/// it was dropped somewhere.
#[cfg(windows)]
pub fn drag_file(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::core::{IUnknown_Vtbl, GUID};
    use windows_sys::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY};
    use windows_sys::Win32::UI::Shell::{
        ILClone, ILCreateFromPathW, ILFindLastID, ILFree, ILRemoveLastID, SHCreateDataObject,
        SHDoDragDrop,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // IDataObject {0000010e-0000-0000-C000-000000000046}
    const IID_IDATAOBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: plain Win32 calls; every PIDL and COM pointer obtained here is
    // freed/released before returning, and null results are checked.
    unsafe {
        let full = ILCreateFromPathW(wide.as_ptr());
        if full.is_null() {
            return false;
        }
        let parent = ILClone(full);
        if parent.is_null() {
            ILFree(full);
            return false;
        }
        ILRemoveLastID(parent);
        let child = ILFindLastID(full) as *const _;
        let mut obj: *mut core::ffi::c_void = null_mut();
        let hr = SHCreateDataObject(parent, 1, &child, null_mut(), &IID_IDATAOBJECT, &mut obj);
        let mut dropped = false;
        if hr >= 0 && !obj.is_null() {
            let mut effect: DROPEFFECT = 0;
            // No drop source of our own: the shell supplies the default one.
            let hr = SHDoDragDrop(
                GetForegroundWindow(),
                obj,
                null_mut(),
                DROPEFFECT_COPY,
                &mut effect,
            );
            dropped = hr >= 0 && effect != 0;
            let vtbl = *(obj as *const *const IUnknown_Vtbl);
            ((*vtbl).Release)(obj);
        }
        ILFree(parent);
        ILFree(full);
        dropped
    }
}

/// Other systems: not supported yet (the app targets Windows).
#[cfg(not(windows))]
pub fn drag_file(_path: &Path) -> bool {
    false
}
