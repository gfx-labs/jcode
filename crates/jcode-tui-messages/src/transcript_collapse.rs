//! Per-message expand/collapse state for compact transcript rows.
//!
//! When `display.compact_transcript` is on, tool results and thinking traces
//! render as one summary line each. Clicking a row expands it in place to show
//! the full tool output or reasoning text; clicking again folds it back.
//!
//! The expanded set is keyed by the message's `stable_cache_hash()` rather than
//! its transcript index so the state survives inserts above it (compacted
//! history loading, reasoning-trace GC) and reloads that rebuild the transcript
//! from the same stored messages. It lives in a process-wide table (like the
//! mermaid inline-expand levels) so the pure render functions, which only
//! receive the `DisplayMessage`, can consult it without threading app state
//! through every renderer. Every change bumps an epoch that render caches fold
//! into their keys.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static EXPANDED: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
static EPOCH: AtomicU64 = AtomicU64::new(0);

fn expanded() -> &'static Mutex<HashSet<u64>> {
    EXPANDED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Whether the message with this stable hash is currently expanded.
pub fn transcript_message_expanded(hash: u64) -> bool {
    expanded()
        .lock()
        .map(|set| set.contains(&hash))
        .unwrap_or(false)
}

/// Flip the expanded state for `hash`. Returns the new state.
pub fn toggle_transcript_message_expanded(hash: u64) -> bool {
    let now_expanded = match expanded().lock() {
        Ok(mut set) => {
            if set.remove(&hash) {
                false
            } else {
                set.insert(hash);
                true
            }
        }
        Err(_) => return false,
    };
    EPOCH.fetch_add(1, Ordering::Relaxed);
    now_expanded
}

/// Explicitly set the expanded state for `hash`. Returns true when it changed.
pub fn set_transcript_message_expanded(hash: u64, value: bool) -> bool {
    let changed = match expanded().lock() {
        Ok(mut set) => {
            if value {
                set.insert(hash)
            } else {
                set.remove(&hash)
            }
        }
        Err(_) => false,
    };
    if changed {
        EPOCH.fetch_add(1, Ordering::Relaxed);
    }
    changed
}

/// Forget every expanded row (used when the transcript is discarded).
pub fn clear_transcript_expanded() {
    let cleared = expanded()
        .lock()
        .map(|mut set| {
            let was_empty = set.is_empty();
            set.clear();
            !was_empty
        })
        .unwrap_or(false);
    if cleared {
        EPOCH.fetch_add(1, Ordering::Relaxed);
    }
}

/// Monotonic counter bumped on every expand/collapse change. Render caches
/// keyed by message content must fold this in, since the content itself does
/// not change when a row is toggled.
pub fn transcript_expand_epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// Bump the epoch without changing any row state. Used when a setting that
/// changes how every compact row renders (compact transcript on/off, reasoning
/// display mode) flips, so prepared-body caches keyed on message hashes
/// rebuild.
pub fn bump_transcript_expand_epoch() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Trailing badge appended to a collapsed compact row.
pub const TRANSCRIPT_EXPAND_BADGE: &str = "▸ expand";
/// Trailing badge appended to an expanded compact row.
pub const TRANSCRIPT_COLLAPSE_BADGE: &str = "▾ collapse";

/// Measured wall-clock seconds the model spent thinking for a committed
/// message, keyed by that message's stable hash. Recorded by the streaming
/// path when the message commits so the folded `thinking` row can show a real
/// duration rather than the whole-turn time. Bounded so a long session cannot
/// grow it without limit.
static THINKING_SECS: OnceLock<Mutex<std::collections::HashMap<u64, f32>>> = OnceLock::new();
const THINKING_SECS_LIMIT: usize = 4096;

fn thinking_secs() -> &'static Mutex<std::collections::HashMap<u64, f32>> {
    THINKING_SECS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Record how long the model thought for the message with this hash.
pub fn record_thinking_secs(hash: u64, secs: f32) {
    if secs <= 0.0 {
        return;
    }
    if let Ok(mut map) = thinking_secs().lock() {
        if map.len() >= THINKING_SECS_LIMIT && !map.contains_key(&hash) {
            map.clear();
        }
        map.insert(hash, secs);
    }
}

/// Thinking duration recorded for the message with this hash, if any.
pub fn thinking_secs_for(hash: u64) -> Option<f32> {
    thinking_secs()
        .lock()
        .ok()
        .and_then(|map| map.get(&hash).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_roundtrip_bumps_epoch() {
        let hash = 0xdead_beef_0001;
        assert!(!transcript_message_expanded(hash));
        let before = transcript_expand_epoch();
        assert!(toggle_transcript_message_expanded(hash));
        assert!(transcript_message_expanded(hash));
        assert!(transcript_expand_epoch() > before);
        assert!(!toggle_transcript_message_expanded(hash));
        assert!(!transcript_message_expanded(hash));
    }

    #[test]
    fn set_reports_change() {
        let hash = 0xdead_beef_0002;
        assert!(set_transcript_message_expanded(hash, true));
        assert!(!set_transcript_message_expanded(hash, true));
        assert!(set_transcript_message_expanded(hash, false));
        assert!(!set_transcript_message_expanded(hash, false));
    }
}
