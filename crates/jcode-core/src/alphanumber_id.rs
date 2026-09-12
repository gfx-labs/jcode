//! Alphanumber session identities: a letter (A-Z without I, L, O) plus a digit
//! 1-9, e.g. `A1`, `K7`. This lives in its own module so it stays out of the
//! way of upstream changes to the memorable animal-name allocator in `id.rs`.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;

/// Letters used for alphanumber names. `I`, `L`, and `O` are skipped because they
/// are easily confused with `1`, `1`, and `0`.
pub const ALPHANUMBER_LETTERS: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V',
    'W', 'X', 'Y', 'Z',
];

/// Digits used for alphanumber names. `0` is skipped for the same reason `O` is.
pub const ALPHANUMBER_DIGITS: &[char] = &['1', '2', '3', '4', '5', '6', '7', '8', '9'];

/// Number of distinct alphanumber names.
pub const ALPHANUMBER_NAME_COUNT: usize = ALPHANUMBER_LETTERS.len() * ALPHANUMBER_DIGITS.len();

/// The alphanumber name at `index` in the fixed A1..Z9 ordering. Wraps modulo the
/// total so any cursor value is valid.
pub fn alphanumber_name_at(index: usize) -> String {
    let index = index % ALPHANUMBER_NAME_COUNT;
    let letter = ALPHANUMBER_LETTERS[index / ALPHANUMBER_DIGITS.len()];
    let digit = ALPHANUMBER_DIGITS[index % ALPHANUMBER_DIGITS.len()];
    format!("{letter}{digit}")
}

fn alphanumber_name_cursor() -> &'static AtomicUsize {
    static CURSOR: OnceLock<AtomicUsize> = OnceLock::new();
    CURSOR.get_or_init(|| AtomicUsize::new((rand::random::<u64>() as usize) % ALPHANUMBER_NAME_COUNT))
}

/// Generate an alphanumber session identity that avoids names already held by
/// active sessions. Mirrors `id::new_memorable_session_id_avoiding`: a
/// process-wide cursor keeps concurrent creators apart, `used_names` keeps
/// identities unique across server reloads, and allocation wraps to reuse
/// names rather than fail when every name is taken.
///
/// Returns `(full_id, short_name)` where `full_id` follows the same
/// `session_<name>_<ts>_<rand>` layout as animal names so
/// `id::extract_session_name` keeps working unchanged.
pub fn new_alphanumber_session_id_avoiding(used_names: &HashSet<String>) -> (String, String) {
    let ts = Utc::now().timestamp_millis();
    let rand: u64 = rand::random();

    let cursor = alphanumber_name_cursor();
    let short_name = (0..ALPHANUMBER_NAME_COUNT)
        .find_map(|_| {
            let name = alphanumber_name_at(cursor.fetch_add(1, Ordering::Relaxed));
            (!used_names.contains(&name)).then_some(name)
        })
        .unwrap_or_else(|| alphanumber_name_at(cursor.fetch_add(1, Ordering::Relaxed)));

    let full_id = format!("session_{short_name}_{ts}_{rand:016x}");
    (full_id, short_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphabet_skips_confusable_characters() {
        for banned in ['I', 'L', 'O'] {
            assert!(!ALPHANUMBER_LETTERS.contains(&banned), "{banned} must be excluded");
        }
        assert!(!ALPHANUMBER_DIGITS.contains(&'0'));
        assert_eq!(ALPHANUMBER_NAME_COUNT, 23 * 9);
    }

    #[test]
    fn names_are_letter_then_digit_with_no_separator() {
        assert_eq!(alphanumber_name_at(0), "A1");
        assert_eq!(alphanumber_name_at(8), "A9");
        assert_eq!(alphanumber_name_at(9), "B1");
        assert_eq!(alphanumber_name_at(ALPHANUMBER_NAME_COUNT - 1), "Z9");
        assert_eq!(alphanumber_name_at(ALPHANUMBER_NAME_COUNT), "A1");
        for index in 0..ALPHANUMBER_NAME_COUNT {
            let name = alphanumber_name_at(index);
            assert_eq!(name.len(), 2, "{name}");
            assert!(!name.contains('_') && !name.contains('-'), "{name}");
        }
    }

    #[test]
    fn all_names_are_distinct() {
        let names: HashSet<String> = (0..ALPHANUMBER_NAME_COUNT).map(alphanumber_name_at).collect();
        assert_eq!(names.len(), ALPHANUMBER_NAME_COUNT);
    }

    #[test]
    fn allocation_avoids_active_names_and_round_trips_through_session_id() {
        let mut used: HashSet<String> = HashSet::new();
        for _ in 0..ALPHANUMBER_NAME_COUNT {
            let (id, short) = new_alphanumber_session_id_avoiding(&used);
            assert!(!used.contains(&short), "reused {short} while free names remained");
            assert_eq!(crate::id::extract_session_name(&id), Some(short.as_str()));
            used.insert(short);
        }
        assert_eq!(used.len(), ALPHANUMBER_NAME_COUNT);
        // Exhausted: allocation wraps instead of failing.
        let (_, short) = new_alphanumber_session_id_avoiding(&used);
        assert!(used.contains(&short));
    }
}
