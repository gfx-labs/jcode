//! Terminal query replies share the event reader, never a second stdin consumer.

use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use super::{filter::Filter, lock_internal_event_reader, InternalEvent};

/// A reply to the image capability queries used by Jcode.
#[derive(Debug, PartialOrd, PartialEq, Hash, Clone, Eq)]
pub enum TerminalResponse {
    /// Whether the Kitty graphics query with image id 31 succeeded.
    Kitty(bool),
    /// Primary device attributes, including attribute 4 for Sixel.
    PrimaryDeviceAttributes(Vec<u16>),
    /// Character cell size in pixels, width followed by height.
    CellSize(u16, u16),
    /// Device status report.
    Status,
}

struct ResponseFilter;

impl Filter for ResponseFilter {
    fn eval(&self, event: &InternalEvent) -> bool {
        matches!(event, InternalEvent::TerminalResponse(_))
    }
}

/// Send a terminal query and collect its replies using the normal event source.
///
/// Call before constructing `EventStream`, not concurrently with event reading.
/// Keyboard, paste, mouse, and resize events remain queued for the next reader.
/// The deadline is absolute, so input cannot keep the probe alive indefinitely.
/// Late replies remain recognized by the byte parser after this call returns.
/// A status reply alone does not end the query: multiplexers can reorder replies.
/// Raw mode is restored only when this call enabled it.
pub fn query_terminal(query: &[u8], timeout: Duration) -> io::Result<Vec<TerminalResponse>> {
    let restore_raw = !crate::terminal::is_raw_mode_enabled()?;
    if restore_raw {
        crate::terminal::enable_raw_mode()?;
    }
    struct RestoreRaw(bool);
    impl Drop for RestoreRaw {
        fn drop(&mut self) {
            if self.0 {
                let _ = crate::terminal::disable_raw_mode();
            }
        }
    }
    let _restore = RestoreRaw(restore_raw);
    let mut reader = lock_internal_event_reader();
    // Discard only stale replies, never events or partial input in the source.
    reader.discard_terminal_responses();
    let started = Instant::now();
    io::stdout().write_all(query)?;
    io::stdout().flush()?;
    let mut replies = Vec::new();
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() || !reader.poll(Some(remaining), &ResponseFilter)? {
            break;
        }
        if let InternalEvent::TerminalResponse(response) = reader.read(&ResponseFilter)? {
            replies.push(response);
        }
        // Complete success can finish early. Unsupported queries use the deadline.
        if replies
            .iter()
            .any(|r| matches!(r, TerminalResponse::Kitty(_)))
            && replies
                .iter()
                .any(|r| matches!(r, TerminalResponse::PrimaryDeviceAttributes(_)))
            && replies
                .iter()
                .any(|r| matches!(r, TerminalResponse::CellSize(_, _)))
            && replies
                .iter()
                .any(|r| matches!(r, TerminalResponse::Status))
        {
            break;
        }
    }
    Ok(replies)
}
