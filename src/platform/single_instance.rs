//! One window: a second launch hands its file to the running viewer and exits.
//!
//! Windows uses a per-user named pipe. Elsewhere every launch opens its own
//! window.

// Used by the Windows implementation; tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// Pipe name, unique per user so different accounts never talk to each other.
pub fn pipe_name(user: &str) -> String {
    let safe: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(r"\\.\pipe\muud-view-{safe}")
}

/// What a second instance sends: the path as UTF-8 (empty = just focus).
pub fn encode(path: Option<&std::path::Path>) -> Vec<u8> {
    path.map(|p| p.to_string_lossy().into_owned().into_bytes())
        .unwrap_or_default()
}

pub fn decode(bytes: &[u8]) -> Option<PathBuf> {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

pub enum Role {
    /// We are the first window; requests from later launches arrive here.
    Primary(Option<Receiver<Option<PathBuf>>>),
    /// Another window took the request; this process should exit.
    Handed,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::io::Write;
    use std::sync::mpsc::{channel, Sender};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    fn name() -> String {
        pipe_name(&std::env::var("USERNAME").unwrap_or_else(|_| "user".into()))
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn create(first: bool) -> Option<isize> {
        let flags = PIPE_ACCESS_INBOUND
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        let n = wide(&name());
        // SAFETY: valid NUL-terminated name; default security; handle checked below.
        let h = unsafe {
            CreateNamedPipeW(
                n.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                std::ptr::null(),
            )
        };
        (h != INVALID_HANDLE_VALUE).then_some(h as isize)
    }

    /// Tries to hand `path` to a running window.
    fn send(path: Option<&std::path::Path>) -> bool {
        let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(name()) else {
            return false;
        };
        // Let the running window come to the front.
        // SAFETY: plain call; ASFW_ANY.
        unsafe { AllowSetForegroundWindow(u32::MAX) };
        f.write_all(&encode(path)).is_ok()
    }

    fn serve(first: isize, tx: Sender<Option<PathBuf>>, wake: Box<dyn Fn() + Send>) {
        let mut handle = Some(first);
        loop {
            let h = match handle.take().or_else(|| create(false)) {
                Some(h) => h as _,
                None => return,
            };
            // SAFETY: `h` is a valid pipe handle owned by this loop.
            unsafe {
                let ok = ConnectNamedPipe(h, std::ptr::null_mut()) != 0
                    || GetLastError() == ERROR_PIPE_CONNECTED;
                if ok {
                    let mut data = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let mut read = 0u32;
                        if ReadFile(
                            h,
                            buf.as_mut_ptr(),
                            buf.len() as u32,
                            &mut read,
                            std::ptr::null_mut(),
                        ) == 0
                            || read == 0
                        {
                            break;
                        }
                        data.extend_from_slice(&buf[..read as usize]);
                    }
                    if tx.send(decode(&data)).is_err() {
                        CloseHandle(h);
                        return;
                    }
                    wake();
                }
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
        }
    }

    pub fn start(path: Option<&std::path::Path>, wake: Box<dyn Fn() + Send>) -> Role {
        for _ in 0..2 {
            if let Some(first) = create(true) {
                let (tx, rx) = channel();
                std::thread::Builder::new()
                    .name("single-instance".into())
                    .spawn(move || serve(first, tx, wake))
                    .ok();
                return Role::Primary(Some(rx));
            }
            if send(path) {
                return Role::Handed;
            }
            // The other window may be just starting or closing; try once more.
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        Role::Primary(None)
    }
}

#[cfg(windows)]
pub use imp::start;

#[cfg(not(windows))]
pub fn start(_path: Option<&std::path::Path>, _wake: Box<dyn Fn() + Send>) -> Role {
    Role::Primary(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_per_user_and_safe() {
        assert_eq!(pipe_name("donguk"), r"\\.\pipe\muud-view-donguk");
        assert_eq!(pipe_name("홍 길동"), r"\\.\pipe\muud-view-____");
    }

    #[test]
    fn round_trip() {
        let p = std::path::Path::new(r"C:\사진\a b.jpg");
        assert_eq!(decode(&encode(Some(p))), Some(p.to_path_buf()));
        assert_eq!(decode(&encode(None)), None);
        assert_eq!(decode(b"  \n"), None);
    }
}
