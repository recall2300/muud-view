//! Updating from inside the app: only when the user asks (F5 → Windows →
//! "Check for updates"), only to GitHub Releases, nothing else sent.
//!
//! 1. Ask `github.com/<repo>/releases/latest` where it redirects: the tag.
//! 2. Newer than this build? Download that release's `SHA256SUMS.txt` and
//!    `muud-view-<tag>-windows-x64.exe`, and check the hash.
//! 3. Swap: the running exe is renamed to `muud-view.exe.old` (Windows
//!    allows renaming a running exe, not overwriting it), the new one takes
//!    its name, and it is started with `--after-update <pid>`: it waits for
//!    this process to end, then deletes the old file.
//!
//! Copies installed by Scoop or winget are left to them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

pub const REPO: &str = "recall2300/muud-view";

/// A release version, from a tag like `v0.8.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches('v');
        let mut it = s.split('.').map(|p| p.parse::<u32>().ok());
        let v = Version(it.next()??, it.next()??, it.next()??);
        it.next().is_none().then_some(v)
    }

    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version")
    }

    pub fn tag(self) -> String {
        format!("v{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The tag a `/releases/latest` redirect points to.
pub fn tag_from_location(location: &str) -> Option<Version> {
    let (_, tag) = location.trim_end_matches('/').rsplit_once("/tag/")?;
    Version::parse(tag)
}

pub fn latest_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

pub fn exe_name(v: Version) -> String {
    format!("muud-view-{}-windows-x64.exe", v.tag())
}

pub fn download_url(v: Version, file: &str) -> String {
    format!(
        "https://github.com/{REPO}/releases/download/{}/{file}",
        v.tag()
    )
}

/// The hash `SHA256SUMS.txt` gives for `file` ("<hex>  <name>" lines).
pub fn expected_hash(sums: &str, file: &str) -> Option<[u8; 32]> {
    let line = sums
        .lines()
        .find(|l| l.split_whitespace().nth(1) == Some(file))?;
    let hex = line.split_whitespace().next()?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Why this copy cannot replace itself, if it cannot.
pub fn managed_by(exe: &Path) -> Option<&'static str> {
    let p = exe
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('/', "\\");
    if p.contains("\\scoop\\apps\\") {
        Some("INSTALLED WITH SCOOP · RUN: scoop update muud-view")
    } else if p.contains("\\microsoft\\winget\\packages\\") || p.contains("\\winget\\links\\") {
        Some("INSTALLED WITH WINGET · RUN: winget upgrade muud-view")
    } else {
        None
    }
}

/// Where the swap puts things, next to `exe`.
pub fn staging(exe: &Path) -> (PathBuf, PathBuf) {
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    (
        exe.with_file_name(format!("{name}.new")),
        exe.with_file_name(format!("{name}.old")),
    )
}

/// Puts `new_exe` (verified bytes) in place of `exe`. On failure the old
/// exe is put back.
pub fn swap(exe: &Path, bytes: &[u8]) -> Result<(), String> {
    let (new, old) = staging(exe);
    std::fs::write(&new, bytes).map_err(|e| format!("cannot write next to the app ({e})"))?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| {
        let _ = std::fs::remove_file(&new);
        format!("cannot move the running app aside ({e})")
    })?;
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&new);
        return Err(format!("cannot put the new version in place ({e})"));
    }
    Ok(())
}

/// Leftovers of an earlier update (best effort; the old exe may still be
/// running for a moment).
pub fn clean_up(exe: &Path) {
    let (new, old) = staging(exe);
    let _ = std::fs::remove_file(new);
    let _ = std::fs::remove_file(old);
}

// ---- SHA-256 (FIPS 180-4) --------------------------------------------------

pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

// ---- The background work -----------------------------------------------------

/// What the update thread reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    UpToDate(Version),
    Available(Version),
    /// Downloaded so far, of total (when known).
    Progress(u64, Option<u64>),
    /// The new exe is in place; restart to use it.
    Installed(Version),
    Failed(String),
}

/// Asks GitHub for the latest version (on a thread).
pub fn check() -> Receiver<Event> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let ev = match latest() {
            Ok(v) if v > Version::current() => Event::Available(v),
            Ok(v) => Event::UpToDate(v),
            Err(e) => Event::Failed(e),
        };
        let _ = tx.send(ev);
    });
    rx
}

fn latest() -> Result<Version, String> {
    let r = crate::platform::http::get(&latest_url(), false, &mut |_, _| {})?;
    let location = r.location.ok_or("no answer from GitHub")?;
    tag_from_location(&location).ok_or_else(|| format!("unexpected answer: {location}"))
}

/// Downloads, checks and puts `v` in place of `exe` (on a thread).
pub fn install(v: Version, exe: PathBuf) -> Receiver<Event> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let result = (|| -> Result<(), String> {
            if let Some(why) = managed_by(&exe) {
                return Err(why.to_string());
            }
            let sums = crate::platform::http::get(
                &download_url(v, "SHA256SUMS.txt"),
                true,
                &mut |_, _| {},
            )?;
            let name = exe_name(v);
            let want = expected_hash(&String::from_utf8_lossy(&sums.body), &name)
                .ok_or("the release has no checksum for the app")?;
            let mut last = 0u64;
            let file =
                crate::platform::http::get(&download_url(v, &name), true, &mut |n, total| {
                    // A report every ~2%, not every packet.
                    if n - last > total.unwrap_or(u64::MAX / 2) / 50 || Some(n) == total {
                        last = n;
                        let _ = progress_tx.send(Event::Progress(n, total));
                    }
                })?;
            if file.status != 200 {
                return Err(format!("download failed (HTTP {})", file.status));
            }
            if sha256(&file.body) != want {
                return Err("downloaded file does not match its checksum".into());
            }
            swap(&exe, &file.body)
        })();
        let _ = tx.send(match result {
            Ok(()) => Event::Installed(v),
            Err(e) => Event::Failed(e),
        });
    });
    rx
}

/// Starts the new exe (which waits for this process to end) with the file
/// being viewed.
pub fn restart(exe: &Path, current: Option<&Path>) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--after-update")
        .arg(std::process::id().to_string());
    if let Some(p) = current {
        cmd.arg(p);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha256(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn versions_tags_and_sums() {
        assert_eq!(Version::parse("v0.8.0"), Some(Version(0, 8, 0)));
        assert_eq!(Version::parse("1.10.2"), Some(Version(1, 10, 2)));
        assert_eq!(Version::parse("v1.2"), None);
        assert_eq!(Version::parse("v1.2.3.4"), None);
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert_eq!(
            tag_from_location("https://github.com/recall2300/muud-view/releases/tag/v0.7.0"),
            Some(Version(0, 7, 0))
        );
        assert_eq!(
            tag_from_location("https://github.com/recall2300/muud-view/releases"),
            None
        );
        let v = Version(0, 7, 0);
        assert_eq!(exe_name(v), "muud-view-v0.7.0-windows-x64.exe");
        assert_eq!(
            download_url(v, "SHA256SUMS.txt"),
            "https://github.com/recall2300/muud-view/releases/download/v0.7.0/SHA256SUMS.txt"
        );
        let sums = "ea00adafc9dcc38a792c7a2c90583f988fc40e8c2816ba427135c9484dfb95d2  muud-view-v0.7.0-windows-x64.zip\r\n\
                    c2c90c0ca1cf1db1eb2ea74b4d9a8d76a58fe922b4da5910cd6c4cf84b260e3a  muud-view-v0.7.0-windows-x64.exe\r\n";
        let h = expected_hash(sums, &exe_name(v)).unwrap();
        assert_eq!(
            hex(&h),
            "c2c90c0ca1cf1db1eb2ea74b4d9a8d76a58fe922b4da5910cd6c4cf84b260e3a"
        );
        assert!(expected_hash(sums, "other.exe").is_none());
    }

    #[test]
    fn package_managers_keep_their_copies() {
        assert!(managed_by(Path::new(
            r"C:\Users\a\scoop\apps\muud-view\current\muud-view.exe"
        ))
        .unwrap()
        .contains("SCOOP"));
        assert!(managed_by(Path::new(
            r"C:\Users\a\AppData\Local\Microsoft\WinGet\Packages\recall2300.muudview_x\muud-view.exe"
        ))
        .unwrap()
        .contains("WINGET"));
        assert!(managed_by(Path::new(r"D:\Tools\muud-view.exe")).is_none());
    }

    #[test]
    fn swap_puts_the_new_exe_in_place_and_cleans_up() {
        let dir = crate::core::archive::tests::temp_dir("update");
        let exe = dir.join("muud-view.exe");
        std::fs::write(&exe, b"old version").unwrap();
        swap(&exe, b"new version").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
        let (new, old) = staging(&exe);
        assert_eq!(std::fs::read(&old).unwrap(), b"old version");
        assert!(!new.exists());
        clean_up(&exe);
        assert!(!old.exists());
        // Nothing to move aside: the error leaves no half-written file.
        let missing = dir.join("gone.exe");
        assert!(swap(&missing, b"x").is_err());
        assert!(!staging(&missing).0.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
