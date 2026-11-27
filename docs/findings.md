# What was learned on the RX 7900 XTX (2026-11-23 to 2026-11-25)

One PC: i9-12900K (8 fast cores, 8 slow ones, 24 threads), 64 GB, Radeon RX 7900 XTX
24 GB, Windows 11, 1440p, Series X profile. Figures are of diagnostics builds with a
screenshot a second. Scenes are not the same from run to run, so read differences
under about 5 % as nothing. What a switch does is in the comment where it is read.

## Where a frame goes

Median of whole runs, same part of the prologue: 280 ms, then 124, 103, 75, now 46-65.
On the water (first boat mission) 46-53 ms.

The title needs 33 ms. Of a 55 ms frame on the water:

- **The thread that prepares submissions: 48 ms.** It is the wall. Steps of the lists
  18 (refreshes of host copies 10), steps done on the card 7, waiting for descriptor
  records 6, asking the write watch 5, copies in the card's memory 4, the title's bytes
  under shader writes 4.
- **The card: about 41 ms busy**, the next wall.
- **List calls: 36,000 a frame, 230 ms of work** spread over eight threads. In a street
  chase 67,000 calls and 430 ms.
- Descriptor records copied as plain bytes: 400,000 slots looked at, 40,000 mirrored a
  frame, most of them empty.
- Landing what shaders wrote: 16 MB a frame put as dwords, on a thread of its own.

## The one idea behind most of it

On the console the CPU and the graphics processor share memory. Here they do not, so
everything one writes and the other reads must be noticed and moved. Every large cost
is one of four: **noticing** writes of the title (write watch, guards), **noticing**
writes of shaders (hashes on the card, compares on the CPU), **moving** them, and
**waiting** until an order is safe. Gains came from making each of them narrower
(dwords, not pages), wider (threads, AVX2) or later (on the card, behind the frame),
in that order of size:

| What | Frame before | after |
|---|---|---|
| Descriptor records of draws' tables left to the draws | 280 ms | about 124 |
| Our hash passes over card copies, 64 threads a piece | card 79 ms | 66 |
| Preparing on a thread of its own, refreshes and records spread | 103 | 75 |
| List calls written down and translated by eight threads (`VICTORY_THIN`) | 75 | 68 |
| Submissions joined, card steps, textures sent ahead | 68 | about 63 |
| Title bytes under shader writes compared with AVX2 on six threads | 14 ms of it | 4-7 |
| Stores into guarded memory looked at first, fewer guards reset | 69 ms of threads' time | about 6 |

(Each figure is from the run in which it was measured; they overlap and do not add up.)

## Faults found, and what they were

- **Boats sank, the sea plane went under.** The ocean's height and displacement maps are
  textures that compute passes write and the title's physics sample on the CPU. They were
  never brought back to title memory. Fixed by Geckomacabre's read-back as merged in
  fxceless/victory-fork f5c03f3b; on by default here (`VICTORY_TEXTURE_BACK=0`: off).
  With it the boat stays up through a whole run; without it sank in every one.
- **A device loss about every 16 minutes**: a page fault inside a texture made in the same
  tenth of a second. A slot showed it before its upload had run.
  `VICTORY_SHOW_AFTER=1`; none in the sessions since (likely fixed, not proven).
- **A rare lost write** (6 of 200 runs of a stress test): a view was mapped between our
  look at its guards and our lock, and views were opened one after another. None in 603
  runs after.
- **Blocky patches**: indirect dispatches cut at 65,535 groups.
- **Square holes and dashed edges in shadows**: pixel shaders that read their quad's
  neighbours with wave reads; on AMD helper lanes take no part in them.
- **Cube reflections too blurred, wrong materials**: DXIL's `Log` is base 2, and
  normalized buffer codes were one step off.
- **Every ray missed**: the dispatch flag that says counts are threads, not groups.

## Measured and not worth it

- Unordered early pipelines for everything (AMD's compiler crashed).
- Series S profile, smaller textures, a second drawing queue, `ASYNC_SUBMIT` off.
- Mapping pages on sixteen threads: no more pages mapped, frames longer.
- Scalar buffer loads four dwords a call (`VICTORY_SHADER_OPT=127`): no gain on this card.
- Wave sizes, fused multiply-adds, fast reciprocals, raw picked buffers, leaving out
  barriers: nothing to 0.1 ms each. Exact RDNA float rules (`VICTORY_RDNA_FLOAT`): 0.8 ms.
- Running the console's shader code natively on the card: the Windows driver takes no raw
  code, the two generations differ, and a Vulkan host layer ends at the same compiler. What
  is left to gain from shader code was guessed at under 1 ms.
- An on-screen panel of numbers (`VICTORY_PANEL`): it helps finding where a frame goes but
  costs some, so it is off and F3 shows it.

## What is next

1. The preparing thread: split what one submission does over two threads in a row; ask
   for the next submission while this one is sent; view the card's own buffers where fills
   copy them now.
2. The card: the passes that run for nothing without rays. Small buffers' copies in the
   card's own memory are done -- the floor is 1 MiB on 16 GB cards and `MIN_KB=64` was
   measured neutral (see "The card-buffer floor" below).
3. Stalls: 270-660 s of a session were full of frames over 200 ms, almost all while new
   pipelines were made. `victory warm`, and a quarter of the workers for the slow ones.
4. Outdoor flicker: suspected to be one bad indirect record that uses up its list's
   budget, so that every later draw of it is empty. The log's frame line counts them;
   `VICTORY_INDIRECT_REST=0` is the experiment.
5. Memory, 40-46 GB private: lists' allocators 9 GB, windows 7, uploads 4
   (`VICTORY_MEMORY_MAP=1` lists it).

## The other repositories, compared

Read up to 2026-11-25: Marvisak/VIctory, fxceless/victory-fork, DexNom/VIctory,
Belobug/VIctory-Belobug-AMD, FQY55kai/ZJOU-VICTORY. What was taken is in the history,
each commit naming its source. In short:

- Marvisak's commits hardly apply any more; nearly all of what they fix is here, often
  another way. Taken: nearest normalized codes, d16 views, shader model flags.
- fxceless: the base. Newest taken: texture read-back, small buffers on the card, hashes
  over a buffer's own bytes.
- DexNom: thread counts of ray dispatches, cube gradients, joined sparse ranges.
- Belobug: pipeline workers for screen passes, the ambient stand-in's idea, pipeline and
  constant buffer bookkeeping (romainhedouin).
- ZJOU: render targets not set again; some shader corrections are being looked at.

## The card-buffer floor, and what it cost

A title buffer's copy has two ways to be watched: on the card, in a compute pass of ours
that compares the declared ranges by 64 KiB piece, or on the CPU, by snapshotting and
comparing 80 MB a frame. Which one a copy takes is a size threshold,
`VICTORY_CARD_BUFFERS_MIN_MB`. On a 16 GB card it was 4 MiB, and that floor was
leaving every copy between 1 and 4 MiB on the CPU.

At 1 MiB, on an RX 6800 (`cb1` -> `cb3`, 210 s, one variable, 0 errors, 0 removals):

| counter | 4 MiB floor | 1 MiB floor |
|---|---|---|
| CPU compare, title buffers' copies | 80.3 MB / 396 ranges | 30.2 MB / 342 ranges |
| of that, in copies >= 1 MiB | 46.5 MB | 0.0 MB |
| copies, preparing | 22.8 ms / 99.5 MB | 19.2 ms / 65.9 MB |
| copy-back | 43.4 ms / 38.6 MB | 24.7 ms / 21.2 MB |
| card buffers alive | 1,001-1,004 MB | 1,087-1,095 MB |

18.7 ms off the frame for 91 MB more card memory out of a 2,048 MB cap, so the tier
now defaults to `(1, 2048)`. Going further down is not the move: `MIN_KB=64` puts 3,485
copies and 1,129 MB on the card and 15,661 pieces through the compare pass, and the
frame does not move (141.7 ms against 144.1 at matched draws).

The 43.4 ms of copy-back was never the compare or the fetch wait. The compare is about
6 ms spread over the copy threads and the card fetch wait is 4.0 ms; the rest is the
read of title memory, the merge and the put back, which `card_back` already joins into
one copy list, one signal, one wait. `VICTORY_TIMELINE` cannot measure them here: the
recording stalls every hooked call, 2,758 ms a frame against 197.

## The card's time, named by command

`VICTORY_GPU_TIMING=1` alone names nothing: `HostList::note` returns at its
`if !dred_on()` gate, so every note is empty and the by-list line reads
`their noted commands 0.0, the rest 121.4`. `VICTORY_D3D12_DRED=full` is what
fills them (`g2`, 210 s, 0 errors, 0 removals, peak 22,989 MB -- DRED full
costs frame rate, so a timing run is not an fps run).

At ~1,450 draws:

```text
the card's time by list (timing run), per frame: 124.6 ms in 298 lists
  the title's 107.9 ms in 212 (their noted commands 78.7, the rest 29.3)
  preludes 11.5 ms in 71 (refreshes of copies)
  compares of copies in the card's memory 5.2 ms in 15
  the least a list took 60 us (17.9 ms a frame at that for every list)
```

Averaged over the run's 1,136 by-command lines:

| ms/frame | per frame | what |
|---|---|---|
| 5.60 | x1.0 | `compute 8bbcbda6008eaa37` |
| 4.96 | x1.0 | `compute 919bafe017d59512` |
| 4.76 | x1.0 | `compute 39f0e9aeb156a61c` |
| 4.35 | x0.9 | `compute 915ed81ea580a276` |
| 3.14 | x1.0 | `compute 776d835da5ffa646` |
| 3.13 | x1.0 | `compute 6697dff15287e544` |
| 3.00 | x140.6 | `vertex ea1100d0f8564245` |
| 2.26 | x1.0 | `compute bc6ca78d5425c179` |
| 2.13 | x71.3 | `alias copy` |
| 2.10 | x1.0 | `compute bba90b2f0ff2eada` |

Six compute passes, each dispatched once a frame, are 25.9 ms of the ~125.
They are not many small things that could be batched; they are single fat
dispatches at 4-6 ms each, the longest 16.6 ms. No rewrite of ours reaches
into the title's own shaders, so this is a floor, not a lever.

By kind over all 1,136 lines: `alias` 27 at 2.13 ms, `indirect` 398 at 0.90,
`dispatch` 532 at 0.64, `draw` 131 at 0.61, `mesh` 13 at 0.40, `indexed` 33 at
0.08, `copy` 2 at 0.01.

The **29.3 ms "the rest"** of the title's lists -- barriers, what of ours is
recorded among the title's commands, and the sending -- is the bigger item and
is ours. `VICTORY_JOIN_BARRIERS=1` was the first way in and it does not
survive the boot: the title's own Breakpad handler fires three seconds in
while its first pipelines are still being made. Closed, off.

## Where the preparing goes, split

`rf2` (210 s, 0 errors, 0 removals, peak 20,425 MB -- the lowest of the
session) instruments `Shadow::nothing_new` and settles what the refreshes
cost. Over 103 preparing lines of 5,000-11,000 spans a frame:

| per frame | |
|---|---|
| refreshes | 23.4 ms |
| of which the checks | 7.6 ms |
| of which the copies | 1.4 ms |
| calls of `nothing_new` | ~6,224 |
| bytes found clean | 5,912 MB |

So the copies are almost nothing and the *check* is 7.6 ms -- 1.2 us a call,
which is the page-record walk (`j.notes.each` over ~230 pages) and not the
locks: the journal's own read waits are 134-156 waits and 1.2-3.8 ms a frame,
measured separately. The remaining ~5.8 ms is the span build and the
`wide_each` dispatch.

`VICTORY_WIDE_SPIN=500` was measured against this (`rf1`) and is a wash:
refreshes 20.2 -> 19.1 ms in the matched 95-180 s window, for four cores
spinning. Stays 0.

## The boot crash: the guard was watching the wrong address

`amdxc64.dll+0x176cd5` faults ~50% of boots while it compiles the pipeline for
one vertex and one pixel program. The fault reads `0x19905a14000` -- two bytes
past the end of a mapped blob, i.e. the compiler walking a shader it was handed.

We already had a guard for exactly this: `pipeline::make::compiler_crashed`,
installed as a vectored handler, which notes the fault, sleeps the worker for
good, and lets the watcher leave that one pipeline out (`warm-bad.txt`) and
start another worker. It is on by default (`VICTORY_COMPILER_GUARD`).

It never fired. Its first test was

    record.ExceptionInformation[1] >= 0x10000  ->  CONTINUE_SEARCH

`ExceptionInformation[1]` is the faulting address, and the guard only took
faults *below* 64 KiB -- "an access of the first pages in `amdxc64.dll`", the
null-deref shape it was written for on an RX 9070 XT. Our fault is at
`0x19905a14000`. The guard stepped aside, the access violation went unhandled,
and the process died before any pipeline could be left out. That is why no
`warm-bad.txt` was ever written, and why the crash looked unguardable.

The address was never the thing worth judging. The thread already says it: the
handler runs only when `COMPILING` is set, which is only set on a pipeline
worker inside a compile, and `WORKER_SLOT` must name that worker. The address
is now recorded and reported, not tested.

Effect: a boot that hits this should survive with one pipeline missing instead
of dying. The log line to look for is

    the driver's compiler crashed making a pipeline: it is left out, the worker is replaced

with `rip` and `address` on it, and the key appended to `pipelines\warm-bad.txt`
so the next boot skips it outright.

The malformed shader itself is still there. This buys the boot, not the pixels:
whatever that pipeline drew goes without a host pipeline, as the title's own
missing pipelines already do.

### Correction: the guard was not the fix, and the fault is not the shader

Two runs after the guard change say so.

`gd2` (guard on): the guard fired, exactly as intended --

    the driver's compiler crashed making a pipeline: it is left out, the worker is replaced
    key="c3d97bacccf970b61be9a46d23db5118" shaders=[vertex dfafaf385b2f94e8, pixel 06295fb1d1aefb78]

-- and the process lived. It then sat at 2.0 fps and **0 draws/frame** for the whole
run. A frozen boot is not a boot.

`gd3` (next boot, key now in `warm-bad.txt`): the pipeline was skipped before any
compile --

    WARN ... not made: the driver's compiler crashed on it before key="c3d97..."

-- and **the same fault fired anyway**, on a thread that is not a pipeline worker:

    first-chance 0xc0000005 at amdxc64.dll+0x176cd5 (read 0x1cf83a04000)
    r13 0x1cf83a03ffe  r14 0x1cf83a03ffe  r15 0x11f3

Same `r15`, same `...3ffe`, same call stack. So the crash is not the pipeline being
made, and leaving that pipeline out does not prevent it.

The two shaders are fine. `VICTORY_SHADER_SAVE` dumped them (`gd4`), 3035 B of vertex
and 1983 B of pixel, and both pass Microsoft's own validator:

    DXIL  2 emitted of 2 vertex and pixel programs; 2 validated, 0 pipelines

### What it actually is: a mapped file two bytes short of a page

The host pipeline library is not read into memory, it is **mapped**
(`pipeline/library.rs`, `Mapped`), and D3D12 keeps reading the bytes it was handed
for as long as the library lives. The mapping is the file's size rounded up to a
page, so a view that ends on a page has nothing mapped past it.

    host-pipelines.bin   671,742 bytes
    164 * 4096           671,744
    671742 = 671744 - 2

Two bytes short of a page. `amdxc64` reads two bytes past the end of the blob it is
walking -- `r13 = 0x...3ffe`, read at `0x...4000` -- and on a mapping that ends there
the read lands on an unmapped page. The size of that file drifts as pipelines are
saved, which is why the fault came and went between boots instead of always firing.

The fix is slack: map four pages of zeroes past the library's own bytes
(`Mapped::SLACK`, read mappings only -- a writable mapping larger than the file would
grow it). The compiler's two bytes now land in mapped zeroes.

The guard stays: it is what named the shader pair and what keeps a crash on a worker
from ending the process. But it is a net under the real floor, not the floor.
