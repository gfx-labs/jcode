//! The user's cooked tty settings from before the first jcode process entered
//! raw mode.
//!
//! Crossterm remembers the pre-raw termios in process memory only. After an
//! exec handoff (reload/rebuild/update) the new process calls
//! `enable_raw_mode()` on a tty that is already raw, so crossterm records the
//! raw settings as "original" and restores raw mode on exit. The shell is then
//! left with `-isig` (Ctrl+C does nothing) and `-opost` (newlines without
//! carriage returns, so output staircases). Carry the real original across
//! exec and restore it explicitly.

#[cfg(not(unix))]
pub use fallback::*;
#[cfg(unix)]
pub use unix::*;

#[cfg(unix)]
mod unix {
    use std::sync::Mutex;

    static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

    fn as_bytes(termios: &libc::termios) -> &[u8] {
        // SAFETY: termios is a plain C struct with no pointers.
        unsafe {
            std::slice::from_raw_parts(
                (termios as *const libc::termios).cast::<u8>(),
                std::mem::size_of::<libc::termios>(),
            )
        }
    }

    pub fn encode(termios: &libc::termios) -> String {
        as_bytes(termios)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub fn decode(value: &str) -> Option<libc::termios> {
        let size = std::mem::size_of::<libc::termios>();
        if value.len() != size * 2 || !value.is_ascii() {
            return None;
        }
        let bytes: Vec<u8> = (0..size)
            .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok())
            .collect::<Option<_>>()?;
        // SAFETY: every bit pattern is a valid termios, and the length matches.
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (&mut termios as *mut libc::termios).cast::<u8>(),
                size,
            );
        }
        Some(termios)
    }

    /// Capture the current tty settings. Call before entering raw mode.
    pub fn capture_current() {
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut termios) } == 0 {
            set(termios);
        }
    }

    pub fn set(termios: libc::termios) {
        if let Ok(mut saved) = SAVED.lock() {
            *saved = Some(termios);
        }
    }

    pub fn get() -> Option<libc::termios> {
        SAVED.lock().ok().and_then(|saved| *saved)
    }

    pub fn encoded() -> Option<String> {
        get().map(|termios| encode(&termios))
    }

    /// Derive usable cooked settings from a raw termios, for a handoff from an
    /// older jcode that did not export the original. Mirrors the parts of
    /// `stty sane` that `cfmakeraw` clears.
    pub fn sane_from(mut termios: libc::termios) -> libc::termios {
        termios.c_iflag |= libc::ICRNL;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            termios.c_iflag |= libc::IUTF8;
        }
        termios.c_iflag &= !(libc::IGNBRK | libc::INLCR | libc::IGNCR | libc::ISTRIP);
        termios.c_oflag |= libc::OPOST | libc::ONLCR;
        termios.c_lflag |=
            libc::ISIG | libc::ICANON | libc::IEXTEN | libc::ECHO | libc::ECHOE | libc::ECHOK;
        termios.c_lflag &= !libc::ECHONL;
        termios.c_cc[libc::VMIN] = 1;
        termios.c_cc[libc::VTIME] = 0;
        termios
    }

    pub fn is_raw(termios: &libc::termios) -> bool {
        termios.c_lflag & (libc::ICANON | libc::ISIG) == 0
    }

    /// Adopt the cooked settings handed over by the previous process, or
    /// synthesize sane ones when the tty was inherited already raw.
    pub fn adopt_inherited(encoded: Option<&str>) {
        if let Some(termios) = encoded.and_then(decode) {
            set(termios);
            return;
        }
        let mut current: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut current) } != 0 {
            return;
        }
        set(if is_raw(&current) {
            sane_from(current)
        } else {
            current
        });
    }

    /// Reapply the saved cooked settings, if any. Safe from a dying process.
    pub fn restore() {
        if let Some(termios) = get() {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios);
            }
        }
    }
}

#[cfg(not(unix))]
mod fallback {
    pub fn capture_current() {}
    pub fn adopt_inherited(_encoded: Option<&str>) {}
    pub fn restore() {}
    pub fn encoded() -> Option<String> {
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn cooked() -> libc::termios {
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        termios.c_iflag = libc::ICRNL | libc::IUTF8;
        termios.c_oflag = libc::OPOST | libc::ONLCR;
        termios.c_lflag = libc::ISIG | libc::ICANON | libc::IEXTEN | libc::ECHO | libc::ECHOE;
        termios.c_cc[libc::VINTR] = 3;
        termios
    }

    #[test]
    fn encode_decode_roundtrips_every_byte() {
        let original = cooked();
        let decoded = decode(&encode(&original)).expect("valid encoding");
        assert_eq!(encode(&decoded), encode(&original));
        assert_eq!(decoded.c_cc[libc::VINTR], 3);
    }

    #[test]
    fn decode_rejects_malformed_values() {
        assert!(decode("").is_none());
        assert!(decode("zz").is_none());
        let valid = encode(&cooked());
        assert!(decode(&valid[..valid.len() - 2]).is_none());
        assert!(decode(&valid.replacen('0', "g", 1)).is_none());
    }

    #[test]
    fn raw_termios_is_detected_and_made_sane() {
        let mut raw = cooked();
        unsafe { libc::cfmakeraw(&mut raw) };
        assert!(is_raw(&raw));
        assert!(!is_raw(&cooked()));

        // A raw tty inherited from an older jcode must come back with signals
        // (Ctrl+C), line editing, and CR/LF output translation.
        let sane = sane_from(raw);
        assert!(!is_raw(&sane));
        assert_ne!(sane.c_lflag & libc::ISIG, 0);
        assert_ne!(sane.c_lflag & libc::ECHO, 0);
        assert_ne!(sane.c_oflag & libc::OPOST, 0);
        assert_ne!(sane.c_oflag & libc::ONLCR, 0);
        assert_ne!(sane.c_iflag & libc::ICRNL, 0);
    }
}
