//! The safety net for the panics the lints cannot see (ADR 0152 §5): an
//! index out of bounds, an overflow in a debug build, a panic inside a
//! dependency.
//!
//! Two pieces, used together. The hook makes every panic *say* something a
//! user can carry into a bug report; the isolation keeps one photograph's
//! panic from taking a whole batch down with it. Both rely on the profiles
//! keeping `panic = "unwind"`.

use std::any::Any;
use std::backtrace::Backtrace;
use std::panic::{self, AssertUnwindSafe};

/// Replaces the default panic hook with one that always prints the
/// backtrace, and says that what follows is a bug in Leyline.
///
/// The default hook prints the backtrace only when `RUST_BACKTRACE` is set,
/// which no user launching Studio from a menu has done: the one panic they
/// hit would then reach a report with nothing to locate it. Called first
/// thing by each client's `main`.
pub fn install_panic_hook() {
    panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("<unnamed>");
        eprintln!(
            "internal error — this is a bug in Leyline, please report it.\n\
             thread '{thread}' {info}\n{}",
            Backtrace::force_capture()
        );
    }));
}

/// Runs one photograph's work so that a panic inside it becomes that item's
/// failure reason instead of the batch's end (ADR 0152 §5).
///
/// The hook has already printed the message and the backtrace by the time
/// this returns; the reason only has to name what went wrong for the
/// report's line. `AssertUnwindSafe` is sound for the callers: the state a
/// panic can leave half-written is a catalog behind a poison-tolerant lock
/// and a SQLite transaction that rolls back on drop — the same state an
/// ordinary `Err` returned midway leaves.
pub(crate) fn isolate<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(work)).map_err(|payload| {
        format!(
            "internal error (a bug in Leyline): {}",
            panic_message(&*payload)
        )
    })
}

/// The text a panic was raised with: `panic!` with a literal carries a
/// `&str`, with a format string a `String`; anything else says nothing.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("panic without a message")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_the_items_reason() {
        let outcome: Result<(), String> = isolate(|| {
            #[allow(clippy::panic, reason = "the panic is what is under test")]
            {
                panic!("index 7 out of range");
            }
        });
        assert_eq!(
            outcome,
            Err("internal error (a bug in Leyline): index 7 out of range".to_owned())
        );
    }

    #[test]
    fn work_that_does_not_panic_passes_through() {
        assert_eq!(isolate(|| 41 + 1), Ok(42));
    }
}
