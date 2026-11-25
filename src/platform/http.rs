//! HTTPS GET for the in-app update, through Windows' own WinHTTP: system
//! TLS, and the proxy settings of the PC (company networks), no extra
//! library. Used only when the user asks to check for or install an update.

#![cfg_attr(not(windows), allow(dead_code))]

pub struct Response {
    pub status: u32,
    /// `Location` header (when redirects are not followed).
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// (host, path) of an `https://` URL.
pub fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    (!host.is_empty()).then_some((host, path))
}

/// Largest body accepted (the exe is ~20 MB).
const MAX_BODY: u64 = 256 * 1024 * 1024;

/// GETs `url`. `progress(received, total)` while the body arrives.
#[cfg(windows)]
pub fn get(
    url: &str,
    follow_redirects: bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Response, String> {
    use windows_sys::Win32::Networking::WinHttp::*;

    struct Handle(*mut core::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle WinHTTP gave us, closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let last_error = |what: &str| {
        format!(
            "{what} failed (network error {})",
            std::io::Error::last_os_error()
        )
    };

    let (host, path) = split_url(url).ok_or("not an https address")?;
    let agent = wide(&format!("muud-view/{}", env!("CARGO_PKG_VERSION")));
    let (host_w, path_w, get_w) = (wide(host), wide(path), wide("GET"));
    // SAFETY: WinHTTP calls with NUL-terminated strings and handles that
    // stay open until the `Handle`s drop, in reverse order.
    unsafe {
        let session = Handle(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(last_error("connecting"));
        }
        let connect = Handle(WinHttpConnect(
            session.0,
            host_w.as_ptr(),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ));
        if connect.0.is_null() {
            return Err(last_error("connecting"));
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            get_w.as_ptr(),
            path_w.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(last_error("request"));
        }
        if !follow_redirects {
            let off: u32 = WINHTTP_DISABLE_REDIRECTS;
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_DISABLE_FEATURE,
                &off as *const u32 as *const _,
                4,
            );
        }
        if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 {
            return Err(last_error("request"));
        }
        if WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
            return Err(last_error("response"));
        }
        let number = |info: u32| -> Option<u64> {
            let mut v: u32 = 0;
            let mut len: u32 = 4;
            let ok = WinHttpQueryHeaders(
                request.0,
                info | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                &mut v as *mut u32 as *mut _,
                &mut len,
                std::ptr::null_mut(),
            );
            (ok != 0).then_some(v as u64)
        };
        let status = number(WINHTTP_QUERY_STATUS_CODE).unwrap_or(0) as u32;
        let total = number(WINHTTP_QUERY_CONTENT_LENGTH);
        let location = {
            let mut buf = vec![0u16; 2048];
            let mut len = (buf.len() * 2) as u32;
            let ok = WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_LOCATION,
                std::ptr::null(),
                buf.as_mut_ptr() as *mut _,
                &mut len,
                std::ptr::null_mut(),
            );
            (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize / 2]))
        };
        let mut body = Vec::with_capacity(total.unwrap_or(0).min(MAX_BODY) as usize);
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(request.0, &mut avail) == 0 {
                return Err(last_error("download"));
            }
            if avail == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + avail as usize, 0);
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                body[start..].as_mut_ptr() as *mut _,
                avail,
                &mut read,
            ) == 0
            {
                return Err(last_error("download"));
            }
            body.truncate(start + read as usize);
            if body.len() as u64 > MAX_BODY {
                return Err("download too large".into());
            }
            progress(body.len() as u64, total);
        }
        Ok(Response {
            status,
            location,
            body,
        })
    }
}

#[cfg(not(windows))]
pub fn get(
    _url: &str,
    _follow_redirects: bool,
    _progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Response, String> {
    Err("updating from inside the app works on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_split() {
        assert_eq!(
            split_url("https://github.com/a/b/releases/latest"),
            Some(("github.com", "/a/b/releases/latest"))
        );
        assert_eq!(split_url("https://example.org"), Some(("example.org", "/")));
        assert_eq!(split_url("http://github.com/"), None);
        assert_eq!(split_url("https:///x"), None);
    }

    /// The real thing, on the Windows CI runner: the latest-release
    /// redirect, and a release file behind GitHub's download redirect.
    #[cfg(windows)]
    #[test]
    fn github_releases_are_reachable() {
        use crate::core::update::*;
        let r = get(&latest_url(), false, &mut |_, _| {}).unwrap();
        assert!((300..400).contains(&r.status), "{}", r.status);
        let v = tag_from_location(r.location.as_deref().unwrap()).unwrap();
        assert!(v >= Version(0, 7, 0));
        let mut calls = 0;
        let sums = get(
            &download_url(Version(0, 7, 0), "SHA256SUMS.txt"),
            true,
            &mut |_, _| calls += 1,
        )
        .unwrap();
        assert_eq!(sums.status, 200);
        assert!(calls > 0);
        let text = String::from_utf8_lossy(&sums.body);
        assert!(
            expected_hash(&text, &exe_name(Version(0, 7, 0))).is_some(),
            "{text}"
        );
    }
}
