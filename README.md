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
| Boot to world | ~140 s |
| Stability | **300 s in world, zero device removals** |
| Frame rate in world | 3–5 fps, 1200–4100 draws/frame |
| Peak working set | ~21 GB |

The picture is correct — beach, water, character, lighting, colour. It renders
the scene the title built. Frame rate is the open problem, and it is a
throughput problem, not a correctness one.

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

A world frame at 3291 draws, 421 ms:

| | |
|---|---|
| On the GPU | 104 ms |
| Preparing | 59 ms — 30 ms of it recording 116 MB of copies |
| Copying back | 37.5 ms — 30.6 MB pulled from card memory |
| CPU-side recording | ~220 ms |

And the number that explains the crawl:

```
textures picked by index: 64324 uploaded (5634 MB), 7907 waiting
```

Seven thousand nine hundred textures queued and never caught up. Every frame is
still uploading. That is the frame-rate problem, and it is next.

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

```powershell
cd E:\GTA6_Frutiger\victory2
$env:CARGO_TARGET_DIR='E:\GTA6_Frutiger\victory2\target'
cargo build --locked --profile diag -p victory --features diagnostics
```

Cold build ≈ 1m27s. Toolchain pinned at `rustc/cargo 1.98.1`. `cargo` prints
`Exited with code 1` on stderr even when it succeeds — check for `Finished`.

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
