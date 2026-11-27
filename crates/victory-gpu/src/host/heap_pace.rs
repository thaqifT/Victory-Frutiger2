//! Heap pacing (off by default, `VICTORY_HEAP_SYNC_PACE=1` turns it on): a
//! submission of ours is prepared (the title's descriptor records brought into
//! the host heaps, card steps' slots pointed, ...) only when the card has run
//! the last frame our side has sent, and the heaps' rounds of their own between
//! submissions stop.
//!
//! Ported from `TaynanGT/Vice-Tay` #357, where it is on by default. It is off
//! here until it has been measured on this title: the title's own numbers on
//! their card are 17-19 fps down to 12-14, and this port's whole business is
//! that the card and our preparing already run at the same time.
//!
//! Why it exists: the card reads descriptors from the host heaps when it runs,
//! and the title rewrites its records for frame F+1 once its present of F has
//! returned (the console's present lets it take the card to be done with F
//! then). Brought into the host heaps while the card still ran F, F's draws
//! read F+1's views (other places of its ring, other buffers): stretched
//! triangles ("wedges", issue #90). This holds our side, not the title.
//!
//! What a frame's end is: the fence value signalled behind its present's copy
//! to the back buffer ([`sent`]). A held submission gives up after [`MOST_MS`]
//! (counted), so that a frame that never ends cannot stop us.
//!
//! It only engages where the flights are in the air (`crate::host::late`), the
//! same guard upstream uses: without that path a submission is waited for
//! before the next begins, so the card is never still drawing our previous one
//! and there is no overlap left to pace. `hp1` ran the switch with the late
//! path off and its own line said so — `0.0 submissions a frame, 0.00 held`.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// The most a submission is held, in ms.
const MOST_MS: u32 = 250;

/// Whether heap pacing is on (`VICTORY_HEAP_SYNC_PACE=1`; off unless asked).
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_HEAP_SYNC_PACE").is_some_and(|v| v == "1");
        if on {
            tracing::info!(
                "heap pacing (VICTORY_HEAP_SYNC_PACE=1): a submission is prepared when the card has run the frame our side sent before it"
            );
        }
        on
    })
}

/// The fence value behind the newest frame our side has sent (0: none yet).
static NEWEST_SENT: AtomicU64 = AtomicU64::new(0);

/// Since the last line: submissions begun, held, microseconds held, held ones
/// that gave up.
static STATS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// Our side sent a frame's present; `fence` is signalled behind its copy.
pub(crate) fn sent(fence: u64) {
    if on() && fence != 0 {
        NEWEST_SENT.fetch_max(fence, Relaxed);
    }
}

/// A submission of ours begins: held until the card has run the newest frame
/// our side has sent.
pub(crate) fn before_submission() {
    if !on() {
        return;
    }
    let Some(host) = super::host() else {
        return;
    };
    STATS[0].fetch_add(1, Relaxed);
    let fence = NEWEST_SENT.load(Relaxed);
    // SAFETY: a plain query.
    if fence == 0 || unsafe { host.fence.GetCompletedValue() } >= fence {
        return;
    }
    let started = std::time::Instant::now();
    thread_local! {
        // SAFETY: a plain event of this thread's.
        static EVENT: Option<HANDLE> = unsafe { CreateEventW(None, false, false, None) }.ok();
    }
    let _held = crate::timeline::span(
        "a submission held: the card runs the frame before (VICTORY_HEAP_SYNC_PACE)",
    );
    EVENT.with(|event| {
        let Some(event) = *event else {
            return;
        };
        // SAFETY: the host's fence and this thread's event.
        unsafe {
            if host.fence.SetEventOnCompletion(fence, event).is_err()
                || WaitForSingleObject(event, MOST_MS).0 != 0
            {
                STATS[3].fetch_add(1, Relaxed);
            }
        }
    });
    STATS[1].fetch_add(1, Relaxed);
    STATS[2].fetch_add(started.elapsed().as_micros() as u64, Relaxed);
}

/// For the frame-rate line.
pub(crate) fn line(frames: f64) {
    if !on() {
        return;
    }
    let s: Vec<u64> = STATS.iter().map(|s| s.swap(0, Relaxed)).collect();
    tracing::info!(
        "heap pacing (VICTORY_HEAP_SYNC_PACE=1): {:.1} submissions a frame, {:.2} held a frame, {:.2} ms a frame held, {} gave up after {MOST_MS} ms",
        s[0] as f64 / frames,
        s[1] as f64 / frames,
        s[2] as f64 / 1e3 / frames,
        s[3],
    );
}
