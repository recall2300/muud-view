//! Keeps a crash in a decoder (ours or a library's, on a broken or hostile
//! file) from taking the app down: the panic becomes an ordinary error for
//! that one file. The panic hook in [`super::log`] still writes it, with its
//! location, to the log.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Runs `f`; a panic inside it comes back as `Err("... crashed: <message>")`.
pub fn catch<T>(what: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => Err(format!("{what} crashed: {}", message(payload.as_ref()))),
    }
}

/// Like [`catch`], for I/O that returns `std::io::Result`.
pub fn catch_io<T>(what: &str, f: impl FnOnce() -> std::io::Result<T>) -> Result<T, String> {
    catch(what, || f().map_err(|e| e.to_string()))
}

fn message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown error".into()
    }
}

/// In tests, a file with this name makes the decoder panic, to check that
/// everything around it survives.
#[cfg(test)]
pub const PANIC_FILE: &str = "__panic__.png";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_an_error() {
        let r: Result<(), String> = catch("decoder", || panic!("bad huffman table"));
        assert_eq!(r.unwrap_err(), "decoder crashed: bad huffman table");
        let r: Result<(), String> = catch("decoder", || panic!("index {} out of range", 7));
        assert_eq!(r.unwrap_err(), "decoder crashed: index 7 out of range");
        assert_eq!(catch("decoder", || Ok(3)), Ok(3));
        assert_eq!(
            catch::<()>("decoder", || Err("plain".into())),
            Err("plain".into())
        );
    }
}
