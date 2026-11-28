//! Which title memory the title reserved write-combining, and whether what
//! shaders wrote to it is fetched back (`fxeless` #868).
//!
//! The title asks `XMemVirtualAlloc` for its graphics memory as write-combining
//! (`0x400` in the console's protection). The runtime drops that bit -- the
//! host cannot map a view write-combining -- but the console's semantics are
//! what the title was written against, and it is the semantics that decide
//! whether the bytes ever need to come back.
//!
//! Reading title memory costs what an uncached line costs: the frame account
//! of `gd14` puts `13.6 ms putting them back` for 8.5 MB, which is 645 MB/s,
//! and 64 B per ~100 ns line is 640 MB/s. So every byte not read is worth its
//! price, and the question is only which bytes are read by anybody.
//!
//! `VICTORY_SKIP_WC_BACK=1` answers it by experiment: what shaders wrote to a
//! piece of write-combining title memory is not put back. Nothing else
//! changes -- the CPU's own writes still go up, the compare still runs (so the
//! frame's `pieces that differ` stays honest), and the snapshot is left alone.
//! Where the title's CPU writes part of a 64 KiB piece the card also wrote,
//! the next upload sends the card's stale bytes there: that is the artifact
//! the experiment trades for the time.
//!
//! `VICTORY_SKIP_WC_COMPARE=1` takes the other half: such a piece is not hashed
//! or compared on the card either, since nothing of it is fetched. Faster, and
//! further from a faithful port; a device removal was reported once with it on
//! an RTX 5080, so it stays opt-in and apart.
//!
//! Off by default. Both switches are named in `VICTORY_AB`, so either can be
//! A/B'd inside one run.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The console's write-combining bit in a protection.
const WRITE_COMBINING: u32 = 0x400;

/// Whether shader writes to write-combining title memory are not copied back.
/// Off by default; `VICTORY_SKIP_WC_BACK=1` turns it on.
///
/// It is worth real time. Measured in `wc2`, world frames only, by the switch
/// state each frame line carries: `copying back` 21.36 ms / 23.41 MB with it
/// off, 5.08 ms / 1.58 MB with it on -- 16.3 ms of every frame, and 93 % of
/// the bytes. `wc4` (off) against `wc3` (on) shows it again on the heavy
/// frames: 36.5 ms / 36.5 MB against 4.9 ms / 1.0 MB.
///
/// It is off because it corrupts. `wc3` (on) logged **529** `a black frame
/// after a frame that was not` warnings, alternating in pairs from frame 2068
/// on: `levels=[130, 1, 129, 1, 130, 1, ...]`, same resource, `state=0x4`. It
/// is not a scene being dark -- the probe reads the frame that is presented.
/// `wc4`, the same run with `VICTORY_SKIP_WC_BACK=0` and nothing else changed,
/// reached frame 2932 (past the whole of `wc3`'s window) with **3** warnings,
/// the same three every run logs in its first frames. Every other log on this
/// machine has 0 to 14.
///
/// The mechanism is the one the module header names: title memory where a
/// shader wrote is left stale, and when the title's CPU reads a buffer it
/// fills with compute and hands back to the card, it reads a frame behind.
/// A frame that alternates black and bright is what one of two buffers being
/// stale looks like.
pub(crate) fn skip_back() -> bool {
    // Diagnostics: named in `VICTORY_AB`, it changes while the title runs.
    #[cfg(feature = "diagnostics")]
    {
        use std::sync::OnceLock;
        static BIT: OnceLock<Option<u32>> = OnceLock::new();
        if let Some(bit) = *BIT.get_or_init(|| crate::ab::bit("VICTORY_SKIP_WC_BACK")) {
            return crate::ab::on(bit);
        }
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_SKIP_WC_BACK").is_some_and(|v| v != "0"))
}

/// Whether such a piece is also left out of the card's compare
/// (`VICTORY_SKIP_WC_COMPARE=1`), which implies [`skip_back`].
pub(crate) fn skip_compare() -> bool {
    #[cfg(feature = "diagnostics")]
    {
        use std::sync::OnceLock;
        static BIT: OnceLock<Option<u32>> = OnceLock::new();
        if let Some(bit) = *BIT.get_or_init(|| crate::ab::bit("VICTORY_SKIP_WC_COMPARE")) {
            return crate::ab::on(bit);
        }
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_SKIP_WC_COMPARE").is_some_and(|v| v == "1"))
}

/// Whether a skip is to happen now, which is what the switches decide. The
/// reservations themselves are recorded whatever this says: the title asks for
/// its 3.8 GB once, at start-up, long before an `VICTORY_AB` phase could turn
/// anything on -- a map built only while a switch was on stayed empty for a
/// whole run (`wc1`).
fn on() -> bool {
    skip_back() || skip_compare()
}

/// A reservation of `XMemVirtualAlloc` as the title asked for it.
#[derive(Clone, Copy)]
struct Reservation {
    len: usize,
    protect: u32,
}

static RESERVATIONS: Mutex<BTreeMap<usize, Reservation>> = Mutex::new(BTreeMap::new());

/// The runtime reserved `[at, at + len)` for the title, which asked for
/// `protect` (the console's, before the runtime cleaned it). Only reservations
/// with a bit this file cares about are kept: the map is looked up per piece.
pub fn title_memory_reserved(at: usize, len: usize, protect: u32) {
    if at == 0 || len == 0 {
        return;
    }
    announce();
    let mut reservations = RESERVATIONS.lock().unwrap_or_else(|e| e.into_inner());
    // The widest entry for a base wins: a later commit of part of a
    // placeholder must not narrow what the reservation covered.
    match reservations.get(&at) {
        Some(had) if had.len >= len => {}
        _ => {
            reservations.insert(at, Reservation { len, protect });
        }
    }
}

/// The runtime freed the reservation at `at` (`XMemVirtualFree`), or part of
/// it from `at` on.
pub fn title_memory_released(at: usize) {
    if at == 0 {
        return;
    }
    let mut reservations = RESERVATIONS.lock().unwrap_or_else(|e| e.into_inner());
    // A partial free leaves the rest reachable under the same base, so only
    // whole reservations go; a stale entry can only make a piece look
    // write-combining that no longer is, and freeing a base the title reuses
    // is covered by the insert above.
    reservations.remove(&at);
}

/// Whether every one of `ranges` lies wholly inside a single reservation the
/// title asked for write-combining.
///
/// A card copy -- especially an aligned loose window -- can span several
/// reservations, some write-combining and some not, so a range that crosses a
/// boundary is not write-combining however its start looks.
fn all_wc_in(reservations: &BTreeMap<usize, Reservation>, ranges: &[(usize, usize)]) -> bool {
    !ranges.is_empty()
        && ranges.iter().all(|&(from, to)| {
            // Seeking from the exclusive end also catches a new reservation
            // beginning inside the range; one ending exactly at `to` fits.
            from < to
                && reservations.range(..to).next_back().is_some_and(|(&base, r)| {
                    base <= from
                        && r.protect & WRITE_COMBINING != 0
                        && base.checked_add(r.len).is_some_and(|end| to <= end)
                })
        })
}

/// Whether `[at, at + n)` is write-combining title memory.
pub(crate) fn is_wc(at: usize, n: usize) -> bool {
    if !on() || n == 0 {
        return false;
    }
    let reservations = RESERVATIONS.lock().unwrap_or_else(|e| e.into_inner());
    all_wc_in(&reservations, &[(at, at.saturating_add(n))])
}

/// Pieces left on the card instead of being fetched back: count and bytes.
static SKIPPED: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// Whether the piece of a copy at `at`, `n` bytes long, is to be left on the
/// card rather than fetched back into title memory. Counted, so the log says
/// how much of the copy-back the experiment took away.
pub(crate) fn skip_piece(at: usize, n: usize) -> bool {
    if !skip_back() || n == 0 || !is_wc(at, n) {
        return false;
    }
    let pieces = SKIPPED[0].fetch_add(1, Ordering::Relaxed) + 1;
    let bytes = SKIPPED[1].fetch_add(n as u64, Ordering::Relaxed) + n as u64;
    if pieces.is_power_of_two() {
        tracing::info!(
            pieces,
            mb = bytes >> 20,
            "VICTORY_SKIP_WC_BACK: write-combining pieces left on the card so far (an experiment: the title's CPU reads of them are stale)"
        );
    }
    true
}

/// The experiment's running total, for the frame line: pieces and bytes.
#[cfg(feature = "diagnostics")]
pub(crate) fn skipped() -> (u64, u64) {
    (SKIPPED[0].load(Ordering::Relaxed), SKIPPED[1].load(Ordering::Relaxed))
}

/// How many reservations are known, for the log line at start-up.
#[cfg(feature = "diagnostics")]
pub(crate) fn known() -> usize {
    RESERVATIONS.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// What the experiment is set to, said once, so a log can be read without
/// knowing which switches were passed. Only the further half says anything:
/// leaving write-combining pieces on the card is off by default, and a run
/// that wants them left there says `VICTORY_SKIP_WC_BACK=1`.
pub(crate) fn announce() {
    if !skip_compare() {
        return;
    }
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        tracing::warn!(
            "VICTORY_SKIP_WC_COMPARE=1: write-combining pieces are neither compared on the card nor fetched back \
             (a device removal was reported once with it on an RTX 5080)"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::{Reservation, all_wc_in};
    use std::collections::BTreeMap;

    const MIB: usize = 1 << 20;

    fn reservation(len: usize, protect: u32) -> Reservation {
        Reservation { len, protect }
    }

    /// A piece in the ordinary half of a card window still comes back: the
    /// test is the range's own reservation, not the copy's.
    #[test]
    fn a_piece_in_the_ordinary_half_of_a_window_still_comes_back() {
        let window = 8 * MIB;
        let middle = window + 2 * MIB;
        let map = BTreeMap::from([(window, reservation(2 * MIB, 0x2404)), (middle, reservation(2 * MIB, 0x2004))]);
        let wc = (middle - 65536, middle);
        let ordinary = (middle, middle + 65536);
        assert!(all_wc_in(&map, &[wc]));
        assert!(!all_wc_in(&map, &[ordinary]));
        assert!(!all_wc_in(&map, &[wc, ordinary]));
        assert!(!all_wc_in(&map, &[(wc.0, ordinary.1)]));
    }

    /// A range must fit one known reservation, and an unknown range is never
    /// write-combining however plausible it looks.
    #[test]
    fn a_range_must_fit_one_known_reservation() {
        let map = BTreeMap::from([
            (0x10000, reservation(0x10000, 0x2404)),
            (0x20000, reservation(0x10000, 0x2404)),
            (0x30000, reservation(0x10000, 0x2004)),
            (usize::MAX - 8, reservation(16, 0x2404)),
        ]);
        assert!(all_wc_in(&map, &[(0x10000, 0x20000), (0x20000, 0x30000)]));
        // Crossing the boundary between two reservations is not one range.
        assert!(!all_wc_in(&map, &[(0x1ffff, 0x20001)]));
        // The third reservation is not write-combining.
        assert!(!all_wc_in(&map, &[(0x30000, 0x30001)]));
        // Empty, unknown, and overflowing.
        assert!(!all_wc_in(&map, &[(0x10000, 0x10000)]));
        assert!(!all_wc_in(&map, &[]));
        assert!(!all_wc_in(&map, &[(0x40000, 0x40001)]));
        assert!(!all_wc_in(&map, &[(usize::MAX - 8, usize::MAX)]));
    }

    /// The end of a reservation is exclusive: a range that starts where the
    /// next one begins belongs to the next one.
    #[test]
    fn the_end_of_a_reservation_is_exclusive() {
        let map = BTreeMap::from([(0x10000, reservation(0x10000, 0x2404)), (0x20000, reservation(0x10000, 0x2004))]);
        assert!(all_wc_in(&map, &[(0x10000, 0x1_0000 + 65536)]));
        assert!(!all_wc_in(&map, &[(0x10000, 0x1_0000 + 65537)]));
    }
}
