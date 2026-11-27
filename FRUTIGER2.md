# Frutiger2

VIctory, tuned for an RX 6800 / RDNA2, running the GTA VI Xbox Series X build
`1.0.2.1` natively on Windows. Fork of
[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD),
branch `a-publier`, HEAD `4eb6cfd`.

This branch is where the RDNA2 work lives. `origin` tracks Belobug; `frutiger`
is ours.

## State, 2026-11-27

The game boots, reaches the world, renders correctly, and **stays up**. The
device-reset problem that killed every run is understood and has a working
configuration. Frame rate is the open problem.

| | |
|---|---|
| Boot to world | ~140 s, with Enter pressed through the account picker |
| Stability | 300 s in world, no device removal |
| Frame rate in world | 3–5 fps, 1200–4100 draws/frame |
| Peak working set | ~21 GB |

## Hardware

Ryzen 7 5800X, Radeon RX 6800 16 GB (RDNA2), 31.9 GiB RAM, 1920x1080,
driver `32.0.21045.29003`.

Belobug tunes for RDNA4. That gap is real and is why this fork exists.

## Build

```powershell
cd E:\GTA6_Frutiger\victory2
$env:CARGO_TARGET_DIR='E:\GTA6_Frutiger\victory2\target'
cargo build --locked --profile diag -p victory --features diagnostics
```

Cold build ≈ 1m27s, toolchain pinned at `rustc/cargo 1.98.1`. `cargo` prints
`Exited with code 1` on stderr even when it succeeds — check for `Finished`.

## Run

Never `scripts\play.ps1` directly. The wrapper measures while it runs and
presses the buttons the title waits for:

```powershell
cd E:\GTA6_Frutiger
.\run2.ps1 -Tag <name> -Seconds 300 -Shots 120,240,290 `
           -More 'VICTORY_RELAY=0;VICTORY_RELAY_COPY=0'
```

`run2.ps1` sets Belobug's 17-knob table, sweeps every `VICTORY_*` variable
first, samples the title bar every 5 s, sends Enter at `-Press` seconds, takes
screenshots, and records peak working set. Outputs `log-<tag>.txt`,
`log-<tag>.err.txt` (the tracing one) and `looks\<tag>\`.

`press.ps1` is what sends the keys. It must bring the VIctory window to the
foreground: `gameinput.rs:1441` gates the keyboard stand-in on `if focused()`,
which needs the foreground window to belong to our process.

## What was wrong

**The device was being reset by a concurrency bug, not a timeout.** No TDR is
ever recorded; `LiveKernelReports` is empty. The driver resets the device
itself, mid-compile, and the first thing our code notices is a
`DEVICE_SUSPENDED` from an unrelated call.

Seven host concurrency layers are on by default and **none of them are
disabled by `-Safe`**:

```
VICTORY_ASYNC_SUBMIT  VICTORY_THIN        VICTORY_RELAY
VICTORY_RELAY_COPY    VICTORY_MAP_THREADS VICTORY_JOIN_COPIES
VICTORY_UPLOAD_AHEAD
```

With all seven off the game ran 282 s without a device removal, past every
previous death at 48 / 77 / 125 s. With only the relay's send-ahead off it
ran 300 s.

`VICTORY_PIPELINE_WORKERS` defaults to two thirds of logical processors — 10
threads inside AMD's compiler at once — and is *not* part of the fix; the game
survives with all 10 running.

## Corrections to the earlier notes

- The pipeline library is **not** broken. `bare1` loaded 61 pipelines from it.
  A run that loads nothing is a run whose knobs changed, which changes the
  translated DXIL, which changes the library name.
- `pipeline library: made` / `loaded` go through `host::trail`, a breadcrumb
  ring flushed only on device removal. A clean run prints neither. The
  "95 pipelines compiled per run" figure was a crash dump, not a total.
- The compile storm is a symptom, not the cause.

## Open

Frame rate. The world frame is ~100 ms GPU, ~60 ms preparing, ~40 ms copying
back, and ~220 ms of CPU-side recording. Texture upload never catches up:
`64324 uploaded (5634 MB), 7907 waiting`. Stability is solved; speed is not.

Do not touch the lighting or colour path. It is correct and stays that way.
