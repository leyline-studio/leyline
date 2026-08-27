//! The capture-date convention of `docs/catalog.md` §9.
//!
//! Its own test binary, and it pins `TZ` itself. Both are deliberate: the
//! zone is process-global, so a test that changed it could disturb another
//! running beside it — and a test that merely read the machine's zone would
//! pass trivially on a CI box set to UTC, which is exactly how this bug
//! survived long enough to break RAW+JPEG pairing on every non-UTC machine.

/// Serializes the tests of this binary against one another. `TZ` is process
/// global, and the harness runs tests on parallel threads: without this, a
/// test reads the zone another one has just set, and the failure looks like a
/// wrong offset rather than a race.
static ZONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Takes the zone for the duration of a test. Held until the guard is dropped,
/// so no other test can change `TZ` in between.
fn hold_zone() -> std::sync::MutexGuard<'static, ()> {
    ZONE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sets the zone for the calls that follow. `wall_clock_from_local` re-reads
/// it on every call, so no further ceremony is needed.
///
/// Only ever called while the [`ZONE`] guard is held.
fn pin_zone(zone: &str) {
    // SAFETY: the caller holds `ZONE`, so nothing else in this binary reads or
    // writes the environment concurrently.
    unsafe { std::env::set_var("TZ", zone) };
}

/// The camera's clock said 2022-04-17 11:51:12. That is the only fact the
/// file holds — a 60D writes no `OffsetTimeOriginal` — so §9 stores it as if
/// it were UTC, and the value must not depend on who imports it.
#[test]
fn a_capture_date_does_not_depend_on_the_importing_machine() {
    let _zone = hold_zone();
    const WALL_CLOCK_AS_UTC: i64 = 1_650_196_272; // 2022-04-17 11:51:12 UTC

    // What LibRaw returns for that shot, machine by machine.
    for (zone, libraw_says) in [
        ("UTC", 1_650_196_272),
        ("Europe/Paris", 1_650_189_072),     // summer time, UTC+2
        ("Asia/Tokyo", 1_650_163_872),       // UTC+9
        ("America/New_York", 1_650_210_672), // UTC-4
    ] {
        pin_zone(zone);
        assert_eq!(
            leyline_raw::wall_clock_from_local(libraw_says),
            WALL_CLOCK_AS_UTC,
            "the capture date came out wrong under TZ={zone}"
        );
    }
}

/// The correction goes through the zone's rules, so it follows daylight
/// saving on the day of the shot rather than applying a fixed offset.
#[test]
fn the_correction_follows_daylight_saving_on_the_day_of_the_shot() {
    let _zone = hold_zone();
    pin_zone("Europe/Paris");
    // Same wall clock, six months apart: UTC+1 in January, UTC+2 in April.
    assert_eq!(
        leyline_raw::wall_clock_from_local(1_642_416_672),
        1_642_420_272,
        "winter: expected a one-hour correction"
    );
    assert_eq!(
        leyline_raw::wall_clock_from_local(1_650_189_072),
        1_650_196_272,
        "summer: expected a two-hour correction"
    );
}

/// A file that records no date says so, and stays saying so.
#[test]
fn a_missing_date_stays_missing() {
    let _zone = hold_zone();
    pin_zone("Europe/Paris");
    assert_eq!(leyline_raw::wall_clock_from_local(0), 0);
    assert_eq!(leyline_raw::wall_clock_from_local(-1), 0);
}
