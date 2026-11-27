# Frutiger2

GTA VI — the Xbox Series X build, `1.0.2.1` — running natively on Windows on an
AMD RX 6800. Not streamed, not emulated in a VM, not a screenshot. The title's
own code, executed, with its renderer translated to Direct3D 12 as it runs.

Frutiger2 is our branch of the VIctory line, tuned from the metal up for
**RDNA2**. Everything upstream aims at RDNA4 and a 24 GB card. This is what
happens when the same title meets a 6800 and 32 GB of system RAM, and the
answers are different.

| | |
|---|---|
| GPU | Radeon RX 6800, 16 GB, RDNA2 |
| CPU | Ryzen 7 5800X |
| RAM | 31.9 GiB |
| Display | 1920x1080 |
| Driver | `32.0.21045.29003` |
| Boot to world | ~67 s |
| Stability | **240 s in world, zero device removals, every run** |
| Frame rate in world | 4–10 fps, 900–6500 draws/frame |
| Mean frame, release build | **198 ms** (100–200 s window) |
| Peak working set | ~21 GB |

The picture is correct — beach, water, character, lighting, colour. It renders
the scene the title built. Frame rate is the open problem, and it is a
throughput problem, not a correctness one.

**`--profile release` is the play build.** It is 15–26 % faster than the
diagnostics build on the same script and the same scene class, on *more* draws
in both windows measured. Diagnostics stays for cause breakdowns; release is
what runs when the question is "is it faster".

## The finding

Every run used to die. 48 s, 77 s, 125 s — always the same way. No TDR, no entry
in `LiveKernelReports`, nothing in the Windows event log. The driver resets the
device itself, mid-compile, and the first thing our code hears about it is a
`DEVICE_SUSPENDED` from a call that has nothing to do with the reset. The last
completed pipeline compiles before death took 1392 ms, 2735 ms, 700 ms. A
healthy D3D12 compile is 10–100 ms. Those numbers are a driver already in
trouble, not the cause of it.

**It was concurrency.** Seven host layers run by default, and not one of them is
touched by `-Safe`:

```
VICTORY_ASYNC_SUBMIT   VICTORY_THIN         VICTORY_RELAY
VICTORY_RELAY_COPY     VICTORY_MAP_THREADS  VICTORY_JOIN_COPIES
VICTORY_UPLOAD_AHEAD
```

All seven off: 282 s, no reset, past every previous death by more than 2×.
Then narrowed — **only the relay's send-ahead off**, everything else at full
speed: **300 s, zero device removals, time limit hit.**

So the kill is one line:

```powershell
-More 'VICTORY_RELAY=0;VICTORY_RELAY_COPY=0'
```

`VICTORY_THIN=1`, `VICTORY_ASYNC_SUBMIT=1`, `VICTORY_JOIN_PARTS=8` and
`VICTORY_UPLOAD_AHEAD=1` all stay on. They were never the problem, and turning
them off costs frame rate for nothing.

`VICTORY_PIPELINE_WORKERS` is also innocent — it defaults to two thirds of
logical processors, 10 threads inside AMD's compiler at once, and the title
survives with all ten running. The compile storm is a symptom. It was never the
killer.

## The frame budget

The honest breakdown, from `VICTORY_TIMELINE` on a diag run (204.6 ms/frame):

| | |
|---|---|
| The card running submissions | 49 ms |
| Preparing (the title's own `XMemMapPhysicalPages`) | 18 ms |
| Copying back into title memory | 7 ms |
| Work spread over threads | 6 ms |
| Jobs for another thread | 7 ms |
| A copy's refresh, a texture's refresh | 8 ms |
| Resetting a host list, the lists' steps | 7 ms |
| Outside our spans | 4 ms |

**The frame is serialized, not overlapped**: ~45 ms of GPU wait and ~79 ms of
CPU work, and they do not run at the same time. `max(GPU, CPU)` is what
overlapping them would buy, and it is worth up to 78 ms. It is also
unreachable on this title — both the relay and the late path remove the device,
there is no D3D12 debug layer on this machine (`0x887A002D`), and there is no
GPU capture tool that sees through the translation.

## What is measured and closed

Every lever below was run, measured against a baseline over a 100-second
window, and closed on evidence. They are written down so nobody re-tests them.

| lever | verdict |
|---|---|
| `VICTORY_RENDER_SCALE` below 2/3 | no effect — the title's largest render size clamps at 2/3 |
| `VICTORY_FSR_4K` | **4K-only.** This panel is 1920x1080: a 4K output renders 4x the pixels and downsamples back |
| `VICTORY_UPSCALER` | cosmetic — patches the picker, does not select the FSR path |
| `VICTORY_PORT_DESCRIPTORS` (port 2) | 1.17 ms/frame cost, no saving; the scan it replaces does not exist here |
| `VICTORY_PORT_GUARD` (port 6) | measured, off |
| `VICTORY_PORT_JOURNAL` (port 7) | measured, off |
| `kernel: GetWriteWatch` write-watch | 2.37 ms/frame, 1.1 % — the asking is not the cost; the *refusal* is (see below) |
| the refusal fallback, before `VICTORY_WATCH_HEAP` | 516 guard walks a frame over 8,948.9 MB, 16.36 ms, to find 60 MB changed |
| `VICTORY_SYNC_FINE` | inside the noise: +3.0 % one window, −5.7 % the next |
| card-buffer pair `MIN_KB=64` / `MB=1536` | loses badly, 217.7 ms against 123.7; mapping causes explode |
| `VICTORY_LATE_*` | removes the device within 40–110 s with every knob off |

Two fixes that **did** land and are proven:

- **Page-mapping** (`271c815a`, `07ff49ee`): `CopyPageMappings` 64.07 → 1.17 ms,
  `XMemMapPhysicalPages` 50.24 → 3.88 ms. Aliased views are mapped one view per
  run of slices. The cost is call count, not call size — `UnmapViewOfFile2` is
  0.223 ms and `MapViewOfFile3` 0.116–0.128 ms, both dominated by the
  address-space lock.
- **Release build** over diagnostics: 15–26 %.

## Measuring honestly

Run-to-run variance is larger than most levers. The same scene class, the same
script, four runs: 123.7, 142.0, 144.8, 217.7 ms. Comparing two separate runs
is how you fool yourself.

```powershell
.\fpswin.ps1 -Tag rl1 -Tag2 pm2 -From 100 -To 200
```

reads `looks\<tag>\fps.txt` (the wrapper's per-second curve) and reports the
mean, median and p90 of the ms/frame over a window, so two runs compare over
100 seconds instead of one 30-frame sample.

On a diagnostics build there is also `VICTORY_AB=<seconds>,<switch>,<switch>`:
it sweeps every combination of the named switches inside one run, in one
scene, and logs each phase's mean, median and p90. Two switches at 30 s is
120 s of measurement with no scene change at all.

## Running it

Never `scripts\play.ps1` on its own. The wrapper sets the knob table, sweeps
every `VICTORY_*` variable, samples the title bar, presses the buttons the
title waits for, and records peak working set.

```powershell
cd E:\GTA6_Frutiger
.\run2.ps1 -Tag r0 -Seconds 300 -Shots 120,240,290 `
           -More 'VICTORY_RELAY=0;VICTORY_RELAY_COPY=0'
```

Outputs `log-<tag>.txt`, `log-<tag>.err.txt` and `looks\<tag>\`.

**The title waits for a button and nothing presses it for you.** The account
picker and the loading screens sit still until pad A is pressed. `press.ps1`
sends it:

```powershell
.\press.ps1 -Keys ENTER -Times 2 -Gap 3000
```

`Enter` is pad **A**. `Backspace` is B. `Esc` is Menu.

It has to bring the VIctory window forward first, and that is not cosmetic —
`gameinput.rs:1441` gates the entire keyboard stand-in on `if focused()`, which
requires the foreground window to belong to our process. A background process
is refused `SetForegroundWindow`, so `press.ps1` uses `AttachThreadInput` to
take it.

`run2.ps1 -Press @(30,45,...)` does this automatically at each listed second.
`-NoPress` disables it.

## Building

Two profiles. **Release is the play build** (15–26 % faster); diagnostics is
for cause breakdowns and `VICTORY_AB`.

```powershell
cd E:\GTA6_Frutiger\victory2
$env:CARGO_TARGET_DIR='E:\GTA6_Frutiger\victory2\target'

# play build
cargo build --locked --profile release -p victory

# diagnosis build
cargo build --locked --profile diag -p victory --features diagnostics
```

≈2m cold, either one. Toolchain pinned at `rustc/cargo 1.98.1`. `cargo` prints
`Exited with code 1` on stderr even when it succeeds — check for `Finished`.

`run2.ps1 -Profile release` runs the first, `-Profile diag` (the default) the
second.

## Reading a run

| file | holds |
|---|---|
| `looks\<tag>\fps.txt` | the per-second curve — `X fps Y ms/frame Z draws/frame` |
| `log-<tag>.txt` | the boot banner and the wrapper's own lines |
| `log-<tag>.err.txt` | **everything the runtime says** — tracing goes here, not to `.txt` |
| `looks\<tag>\*.png` | the screenshots `-Shots` took |

## What this branch is

A tree of other people's work with our own on top. `origin` tracks
[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD) at
`a-publier` / `4eb6cfd`; `frutiger` is ours. The RDNA2 work, the stability
finding and the run procedure live here.

The history is kept, and commits taken from elsewhere name their commit and
author. The tree is GPL-3.0-or-later, as VIctory — see [LICENSE](LICENSE).

**This repository contains no game files, code or assets.** You need your own
copy of the title. Nothing here is affiliated with or endorsed by its developer
or publisher.

## Built on

- **[VIctory](https://github.com/Marvisak/VIctory)** — Marvisak and contributors:
  the loader, runtime services, the D3D12 back end, the shader recompiler.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)** — the
  fork this line started from, and the pull requests it carries.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)** — ray tracing fixes,
  cube gradients, sparse textures.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**
  — the ambient light idea, pipeline workers, romainhedouin's work.
- **[myokex/victory-amd](https://github.com/myokex/victory-amd)** — the guard
  over every view of a page (with Spexivus, joroyt, tightfall), Joshua Odom's
  reads into title memory.
- **Geckomacabre**, **cxy0072019-sketch** — texture read-back.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the
  same title behaves under another layer. No code of it is here.

Deeper detail: [FRUTIGER2.md](FRUTIGER2.md), [docs/findings.md](docs/findings.md),
[docs/architecture.md](docs/architecture.md).
