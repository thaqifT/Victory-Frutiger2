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

## Three clean boots, and the refresh filter that lost

The slack holds. Three runs, none of them faulting:

| run | seconds | menu | world from | peak working set |
|---|---|---|---|---|
| `gd6` | 110 | 30 fps / 12 draws from 8 s | 69 s, 402 -> 4,539 draws | 20,996 MB |
| `gd7` | 200 | 30 fps / 13 draws from 7 s | 67 s, 251 -> 5,527 draws | 21,477 MB |
| `gd8` | 191 | 30 fps / 13 draws from 8 s | 68 s, 309 -> 7,760 draws | 22,268 MB |

All three: 0 first-chance exceptions, 0 unhandled, 0 BacktraceBreakpad reports,
0 `ERROR` lines, 0 panics, 0 device removals. `gd8` ended on its own at 191 s
with the log's last lines being pipeline binds, not a fault.

`gd7`'s matched band (95-180 s) is the new baseline for the world:

    138-146 ms/frame = 41-42 preparing (copies 18-19 ms / 73-79 MB,
    textures 0.6-1.3 ms) + 71-73 on the GPU + 19-20 copying back (25-27 MB)

### `VICTORY_REFRESH_FILTER` -- measured, and off

A world frame's 59 flushes hand over ~5,141 refreshes and 4,981 of them find
nothing to copy (`gd6`: 5,858 spans, 4,981 known empty). The idea was to drop
the empty ones on the submitting thread for every flush instead of waking eight
threads to find out.

It loses. `gd7` (off) against `gd8` (on), same machine, same switches:

| | refreshes | lists' steps |
|---|---|---|
| `gd7`, filter off | 16.9-18.2 ms | 22.0-23.8 ms |
| `gd8`, filter on | 34.4 ms | 53.6 ms |

`Shadow::nothing_new` is not the cheap check it looks like: it takes three
mutexes (`pending`, `copied`, `ranged`) and then walks the journal's written
pages. Eight threads pay ~3 us a span for that; one thread after another pays
~7 us, and there are thousands of spans. The switch stays, defaulting off, with
these numbers in its doc comment.

### What the copy instrument says (`VICTORY_COPY_SPLIT`, `gd8`)

    14,706 calls, 49.67 ms, 191.7 MB, 3,378 ns a call, 3.9 GB/s
      <=64KiB   2,349 calls  12.75 ms   61.5 MB
      <=1MiB      512 calls  36.56 ms  124.0 MB

Two buckets hold 96 % of the time, and both move large pieces: the cost is
bandwidth, not the per-call overhead `VICTORY_FILLS_PLAIN` was written for
(1,564 calls of 64 B and under cost 0.01 ms together). `safecopy::copy` is
already one `rep movsb`, so the 3.9 GB/s is the copy itself, not a kernel call.

The same run's `copies per frame` line is where the next lever is:

    8,175 refreshes 9,519 MB, moves 15.74 ms
    457 watches 23.73 ms, 9,315 MB checked, 9,255 MB unchanged

9.3 GB of ranges are checked every frame and 99.4 % of it is unchanged, so the
*walk over the journal's written pages* costs more than the copies it protects.
That is what `nothing_new` walks, and it is why the filter above lost.

### The library's slack was never mapped (`b37d58a8`, `gd12`)

`898c769e` fixed the boot crash by mapping `Mapped::SLACK` (0x4000) bytes of
zeroes after `host-pipelines.bin`, so that the two bytes `amdxc64.dll` reads
past the end of the blob land in mapped memory. It did not do that. It passed
`len + SLACK` as the mapping object's size, and Windows refuses a view larger
than the file:

    file   671,742 bytes
    slack 0       -> ok    (view 671,742)
    slack 0x1000  -> CreateFileMappingW failed 8
    slack 0x4000  -> CreateFileMappingW failed 8

`8` is `ERROR_NOT_ENOUGH_MEMORY`, and it comes back from `CreateFileMappingW`
itself, not from `MapViewOfFile`. So `Mapped::map` returned `None`, `Blob::of`
fell through to `std::fs::read`, and **the library has been held in a heap
buffer in every run since**. The log line says so and was never read:

    host pipeline library loaded bytes=671742 mapped=false

The crash stopped because the driver's two bytes past the end landed in the
heap allocation's rounding instead of an unmapped page. The fix worked by
accident, and the accident was "stop mapping".

What it is now: `Blob::read_padded` reads the file into a buffer of
`len + 0x4000` and truncates the length, so the slack is in the same allocation
and `Blob::bytes` does not show it. `Mapped::map` takes no slack parameter --
a view ends exactly at the file's last byte and cannot carry any. Mapping is
off by default (`VICTORY_LIBRARY_MAP=1` to turn it on); saves still go through
a mapping, which is what `Mapped::create` was always for.

The two tests `898c769e` left failing now pass, and they were failing on `HEAD`
too -- they are the record that the mapping was gone:

    the_library_file_is_mapped_and_replaced ... ok
    a_replaced_library_file_still_reads_from_the_disk ... ok

### The three locks in `nothing_new_at` are not the cost (`gd12`)

`nothing_new_at` takes `pending`, `copied` and `ranged`, and 5,579 spans a
frame pay for all three. Made them cheap: `copied` became an `RwLock` (read by
every refresh, written by few), `ranged` became six atomics (it is always
touched under `copied`'s lock anyway, so they are free), and `pending` grew a
`pending_len` counter so the checks that would take its lock only to find it
empty skip it.

Measured, same switches, same ~5,500 spans:

| | checks median | checks mean |
|---|---|---|
| `gd10` (before) | 4.9 ms | 3.92 ms |
| `gd12` (after) | 5.1 ms | 3.90 ms |

No change. The locks were not the cost; the journal walk inside
`crate::watch::nothing_new` is, and `be8543c8` already took the cache misses out
of it. Reverted. `nothing_new` is ~5 ms of all threads' time in a 175 ms frame
-- even deleting it entirely is 3 % -- so it is not where the frame goes.

Where the frame does go (`gd10`, world band, medians): 45.3 ms preparing,
82.4 ms on the GPU, 24.1 ms copying back. `VICTORY_RENDER_SCALE=0.5` in `gd11`
cut the pixels 44 % and the GPU time only 82.4 -> 71.1 ms, so the GPU is not
fill-bound; it is geometry, compute or barriers. The host runs real mesh
shaders (`host mesh shader support mesh=true`, tier 1, shader model 6.6 on the
RX 6800), so amplification is not being emulated either.
### Where a frame goes, and the barrier count (`gd12`, `gd13`)

The line that splits the preparing says it plainly (world band, `gd12`):

    20.9 in the lists' steps (refreshes 15.3 in 47 flushes of 5516, 4699 of them
    known to copy nothing, 4756 on several threads, 1.7 one at a time in 319
    before shader writes; copies and fills 1.3), 5.8 asking the write watch,
    2.7 snapshots, 5.8 for copies in the card's memory

84 % of the refreshes are already known to copy nothing before they start, and
the ones left cost 18.7 us each -- the copy, not the check. `nothing_new` at
~5 ms of all threads' time is 3 % of a 175 ms frame, which is why `gd12`'s lock
work measured nothing.

`gd13` is the run with the library held instead of mapped: 0 first-chance,
0 unhandled, 0 device removed, 0 panicked, best 31 fps, peak 20,515 MB. Its
world band medians are 126.0 ms with 36.8 preparing, 69.4 on the GPU and 15.3
copying back, against `gd12`'s 178.2 / 56.4 / 87.0 / 26.9. **That is not a
2x win from the library fix** -- the scenes differ (1,911 draws a frame against
5,500) and cross-run frame times cannot be compared here at all. What `gd13`
says is that the library fix is stable, which is what it was for.

### The barriers, and how to measure them

    waits for every earlier shader write, per frame: 763 for the title's
    barrier groups, 0 for its barriers on buffers; 136 indirect commands
    bounded (a dispatch of ours between two such waits each)

1,082 of them in a heavier second. Both call sites -- `list.rs`'s `sync` (the
title's pipeline syncs) and `barrier_groups` (its enhanced barriers) -- throw
the arguments away and record one global `UAV_BARRIER` (`kind: 2`,
`first: null`), which waits for every earlier shader write in the pipeline.
Upstream is the same: `fxeless/victory-fork` main's `barrier_groups` has the
same `_groups` and the same comment, "one barrier on every shader write". So
this is not a port; nobody has done it.

Two of every three are ours: 136 indirect clamps x 2 waits each = 272 of the
763, from the pass that bounds an indirect command (`VICTORY_BARRIER_TEST` bit
1 drops exactly those).

`VICTORY_BARRIER_TEST` now counts the syncs apart from the groups
(`WaitsForSync`), gained bit 2 (`sync`) and `all`, and can be named in
`VICTORY_AB` -- both sides, one run, one scene. That last part matters: the
draw count swings 400 to 7,900 inside a run, so two runs never compare.

fxeless #867 tracks a per-list `uav_dirty` flag (set by UAV-writing
dispatches, storing draws, copies; cleared by a global UAV barrier) and skips
the barrier when nothing has written since. Its measured wins came from
pipeline waits and texture staging, not from that item.
### The barriers cost nothing (`gd14`)

`gd14` ran the whole barrier experiment as a `VICTORY_AB` switch, so both sides
were measured in one run and one scene -- the only valid comparison here (the
draw count swings 400 to 7,900 inside a run). 25 s phases, one switch, so odd
phases dropped every wait for earlier shader writes and even phases kept them:

| phase | waits | frames | mean ms | median ms | draws |
|---|---|---|---|---|---|
| 3 | dropped | 105 | 228.8 | 209.4 | 3412 |
| 4 | kept | 128 | 187.9 | 162.6 | 1522 |
| 5 | dropped | 133 | 180.5 | 147.0 | 1419 |
| 6 | kept | 126 | 191.7 | 182.6 | 1586 |
| 7 | dropped | 145 | 169.5 | 114.9 | 1314 |

Per draw, which is the only way to compare phases with different scenes:
dropped 0.0671, 0.1272, 0.1290 ms; kept 0.1234, 0.1209 ms. The dropped phases
are not faster -- the middle two pairs differ by 3 % one way and 24 % the
other, with p90s from 177 to 300 ms, which is scene noise. **763-1,082 global
UAV barriers a frame cost nothing measurable on this driver and this card.**

So `fxeless` #867's per-list `uav_dirty` flag is not worth porting. Its own
measured wins came from pipeline waits and texture staging, not that item, and
this measurement says the same from the other side.

Two more things `gd14` settled:

- **The title never calls `sync`.** `WaitsForSync` is 0 in every line of the
  run, so all 763 are `barrier_groups` (its enhanced barriers). `VICTORY_BARRIER_TEST`
  bit 2 is dead weight; `list.rs`'s `sync` path is exercised only if the title
  ever starts calling it.
- The experiment flips `exp=0` / `exp=7` exactly as the phases say, so the
  switch really did reach both call sites.

0 first-chance, 0 unhandled, 0 device removed, 0 panicked, best 31 fps, peak
21,199 MB.

### The freezes are pipeline waits, and one frame's worth is enough (`#1181`, `#1234`)

The worst frames of the two stable runs, from `looks\<tag>\fps.txt`:
1,399.7 ms (`gd13`), 1,223.1, 1,034.0, 760.2, 625.8 ms (`gd14`). Those are
not fill: they are the title binding a pipeline the worker has not made, and
the wait budget (`VICTORY_PIPELINE_WAIT_MS`, 500 ms a frame, shared across
threads) being spent on it -- in **every** frame that draws with it.

`fxeless` #1234 fixes the shape of that: a pipeline is waited for in one frame
only (`Pending::waited_frame`, claimed with `compare_exchange` and only while
the frame still has budget). A compile the worker has not finished in that
frame is skipped in later frames instead of taking the whole budget again.
`VICTORY_PIPELINE_WAIT_EVERY_FRAME=1` restores the old behaviour, and the
switch is `crate::ab`-able, so it can be A/B'd in one run.

Note the frame line already reports the cost of *not* waiting: `dropped 1342
without a host pipeline (0 being made)`. Those draws do not happen -- the
frame is cheaper and the scene is missing pieces. That is the trade the old
behaviour made the other way round.
### Write-combining reads are the copy-back, and no code beats the hardware (`gd14`)

The frame account line splits the two directions exactly:

    what shaders wrote: 2.8 ms finding 438 pieces that differ, 13.6 ms putting them back

13.6 ms for the pieces put back. The frame line puts the same band at 24.4 ms
for 8.5 MB. 8.5 MB over 13.6 ms is **645 MB/s**, and that is not a slow loop:
it is what reading write-combining memory costs. A WC read is one uncached
fetch per 64-byte line, ~100 ns each, and 64 B / 100 ns is 640 MB/s. The copy
back reads title memory (`read_into`) before it writes, so that the title's own
concurrent writes to the same buffer are not clobbered -- it reconstructs what
the memory held (`was = now ^ changed`) and writes only the dwords the card
changed. The read is the price of not clobbering, and at 645 MB/s it is already
at the hardware's limit.

The other direction is the same shape: `refreshes 25.0 in 48 flushes of 5140,
4266 of them known to copy nothing` -- 874 of them do, and they read title
memory into the shadow at the same per-line cost.

**So 38.6 ms of a 248 ms frame (16 %) is reading write-combining memory, and
no tighter loop, wider copy or non-temporal load changes it.** The only lever
is to read fewer bytes, which needs to know which title memory the title never
reads back. That is exactly what `fxeless` #868's census is for, and why its
switch (`VICTORY_SKIP_WC_COMPARE`) is "leave write-combining memory on the
card" and not "copy it faster".

The rest of the account, for the record (248 ms frame, 2395 draws):

    after a present 22.5 ms until the first list call and 1.3 ms until the
    first submission; 0.0 ms from the last submission to the present call;
    lists reset in 14.8 ms; the process ran 471 ms user + 159 ms kernel on all
    threads

and the card:

    the card's clock per frame: nothing of ours queued 202.2 ms (the card about
    28% busy by this count), 188.5 ms of that while the submitting thread was
    working (the card idle behind us), the rest while the title had not
    submitted; the submitting thread stood waiting for the card 80.3 ms

28 % busy. The card is idle most of the frame, and 188.5 ms of that idle time
is while our own side is still working. That is the frame-overlap prize, and it
is the one worth the most: it needs no new rendering, only submissions that
leave while the next one is prepared.
### The pipeline wait does bite, once (`gd15`)

`gd14`'s 39 recording lines peaked at 0.1 ms waiting for pipelines, which made
#1234 look like a no-op. `gd15` (the same A/B with the switch named) shows one
window at **250.3 ms** -- and the per-second curve names the frame it belongs
to: `t=178`, **1521.9 ms/frame, 0 draws**. Nothing drawn, a quarter of a second
in pipeline waits, and the frame three times its own budget.

So the shape is right: a pipeline that a worker has not made costs the whole
frame's budget (500 ms, shared) and then does it again next frame. It is rare --
one window in 39 -- but it is one of the 1+ second frames, and it is the one
class of them that is entirely ours. `VICTORY_PIPELINE_WAIT_EVERY_FRAME=1` puts
it back for comparison.

The A/B phases cannot settle it either way: phase 3 (every-frame waits, 2,787
draws, mean 205.4) against phase 4 (one-frame, 1,507 draws, mean 184.1) is
0.0737 against 0.1222 ms a draw, but the phases drew different scenes. That is
the same trap as `gd14`'s barriers; the per-window counter is the honest
measure and it is quoted above.

`gd15` itself: 0 first-chance, 0 unhandled, 0 device removed, 0 panicked, best
31 fps, peak 21,299 MB.

### Write-combining memory: what the title asks for

`XMemVirtualAlloc` already logs what the title asked for, before the runtime
cleans it. From `gd14`, the reservations that carry the console's
write-combining bit (`0x400`):

| protection | kind | flags | MB | at |
|---|---|---|---|---|
| `0x2404` | `0x203000` | `0x283` | 2,199 | `0xd49400000` |
| `0x2404` | `0x42000` | `0x23` | 1,621 | `0xfa5600000` |
| `0x42404` | `0x203000` | `0x283` | 0 | `0xdd2b00000` |

3.8 GB. `0x2404` is `PAGE_READWRITE` (`0x4`) plus the GPU-read bit (`0x2000`)
plus write-combining (`0x400`); `0x42404` adds `0x40000`. The runtime drops
`0x400` -- Windows cannot map a view write-combining -- but the console's
semantics are what the title was written against, and they are what decide
whether a byte ever has to come back to the CPU.

That is the whole premise of `fxeless` #868, and it is now in the tree as
`crate::wc`: reservations are recorded with the protection the title asked for,
a piece is skipped only when it lies **wholly inside one** such reservation (a
card copy's aligned window can span several, some of them ordinary memory), and
`VICTORY_SKIP_WC_BACK=1` leaves those pieces on the card instead of reading
them back. `VICTORY_SKIP_WC_COMPARE=1` also leaves them out of the card's
compare. Both are off by default, both are `VICTORY_AB`-able, and the frame
line reports pieces and megabytes left behind per frame.

The measured cost it is aimed at, from `gd14`: 13.6 ms to put 8.5 MB back,
645 MB/s, against the 640 MB/s an uncached line costs at 64 B per ~100 ns.
### Upstream triage: what was looked at and why it was not taken

Three of `fxceless/victory-fork`'s 946 open PRs were read against this tree.

- **#1450, "cap the time a frame waits for compute pipelines made at their
  dispatch."** A real crash fix -- with `VICTORY_PIPELINE_COMPUTE_WAIT` on, an
  empty cache built 457 compute programs in one frame at about a second each,
  the frame did not end for 30 s, and the title's own hang watchdog stored to
  address 0. **Not applicable here:** this tree has no
  `VICTORY_PIPELINE_COMPUTE_WAIT` and no compute-at-dispatch path at all --
  `ready()` always goes through `wait(p, frame)` under the frame's budget
  (`pipeline/make.rs:416`). The crash shape does not exist. What this tree does
  have is the bounded version of the same wait, which `gd15` measured at 250.3
  ms in one window.
- **#1192, "reduce CPU work when preparing descriptor tables."** 217 lines
  across two files, a restructure of the static table layout, the slot set and
  the dynamic view expressions. Its own body says "**no gameplay FPS gain has
  been measured**". High regression risk on the path every draw goes through,
  no number on the other side. Not taken.
- **#1479, "count the pieces that differ on the relay and card merges."**
  Diagnostics only, and this tree already counts them: the frame account prints
  `what shaders wrote: 2.8 ms finding 438 pieces that differ, 13.6 ms putting
  them back`, and `card_back` counts `CARD_STATS[3..6]` for pieces, bytes,
  fetches and the wait. Not taken.

#868 is the one that carried a measurement, and it is now in the tree as
`crate::wc`.
### A shader write is not landed when the title writes a dword beside it

`host::refresh_tests::landings_title_writes_and_light_refreshes_at_once` fails
on this tree, deterministically, in the first few rounds. It was taken as
"pre-existing and flaky" in earlier sessions. It is neither flaky nor benign.

**Repro, 60 ms:**

```powershell
$env:VICTORY_TEST_ROUNDS='5'
cargo test --manifest-path E:\GTA6_Frutiger\victory2\Cargo.toml --locked `
  -p victory-gpu --features diagnostics `
  landings_title_writes_and_light_refreshes_at_once
# not landed: expected at least 6 at offset 0xc50c (class 3), found 4
```

`VICTORY_TEST_ROUNDS=1` and `=2` **pass**. `=5` and above fail. `=120` fails in
the same place or in the second copy's class. So it is not a race that needs
load -- it is state that two rounds build.

**What the test sets up.** Two writer threads bump dwords at offset `% 32 == 4`
and `== 8`. A flight's shaders write dwords at `% 32 == 12` (copy 0) and
`== 20` (copy 1). The two sets are disjoint on purpose, and the test's own
comment says so: whatever lands can never be overwritten by the title.

**What is observed.** Copy 0's round-1 flight (value 4) lands. Its round-2
flight (value 6) does not, and title memory still holds 4. The failure is at a
`% 32 == 12` offset, which is a shader dword, so the title never wrote it.

**Where it goes.** `Shadow::land_card` (`shadow.rs:4873`) builds a skip list
from `card.dirty` and `card.sent` before landing, and lands only the parts of
each piece outside it:

```rust
let mut skip: Vec<(usize, usize)> =
    dirty.iter().copied().filter(|&(a, b)| a < to && b > from).collect();
sent.near(fetched_after, from, to, &mut skip);
```

`dirty` is "the title's bytes that are to go up"; skipping them is right, since
the title's newer bytes must win over an older fetch. So the question is only
how a shader's dword at `% 32 == 12` ever gets into that list.

**What was ruled out.** The first guess was `card_changed_runs`
(`shadow.rs:4507`) joining runs that touch (`from <= last.1 && to >= last.0`).
It is not that. The join is a union of written bytes and cannot invent one:
`(4, 12)` and `(12, 16)` merge to `(4, 16)`, which holds nothing nobody wrote.
A writer at `% 32 == 8` cannot grow a run into the shader's `12`.

The second guess was a stale `VICTORY_SEND_CHANGED` note. Also not it:
`send_changed()` is on by default now, and the test's own `exact` flag reads the
same switch, so `dword_runs(.., gap 0)` is what records the ranges.

`land_echo()` (`shadow.rs:2086`) is `toggled!`, so it too is on by default,
which means `take_landed` is the path for a page that is `known`, `landing` and
not under `upcheck`.

**Two live suspects, neither confirmed.**

1. `Shadow::copy` (`shadow.rs:4424`) sends a whole 4 KiB page into `dirty`
   when `whole = !known && card.before.is_none()`. That page then holds every
   byte of it, including shader dwords that have not been landed yet, and
   `land_card` skips all of them. It also pushes the title's stale bytes up.
   It needs a page that is not yet `known`, which is a cold page -- and the
   failure is on a page the test has written to for several rounds.
2. `sent.near(fetched_after, ...)` filters by the fetch's send number, but
   `dirty` does not: a range that has not gone up yet is skipped as if it had.

**Why it does not crash the game.** The consequence is a stale dword in title
memory where the title and a shader write the same 32 bytes. It is corruption,
not a fault, and it is invisible unless something reads the stale word.

**What would settle it, cheaply.** Run the test with the two suspects turned
off, one at a time, and see which one makes it pass:

```powershell
$env:VICTORY_SEND_CHANGED='0'   # the whole-page road only
$env:VICTORY_LAND_ECHO='0'      # no take_landed
```

Neither was run here. The test is a synthetic harness that mutates the card's
side by hand, so a pass under either switch would say which mechanism the
harness is exercising before it says anything about the game.

### Where a world frame's 144 ms goes, after the write-combining skip

`wc2` and `wc3`, world frames only, with `VICTORY_SKIP_WC_BACK` on. The frame
line splits into four terms and one of them is not on the line at all:

| term | ms | where it is |
|---|---|---|
| preparing | 40.3 | `PREPARING`, on the submitting thread |
| waiting for the GPU | 80.2 | `SUBMISSION_US[1]` |
| copying back | 4.8 | `COPY_BACK`, now mostly gone |
| the title's own recording | ~18.6 | the rest of the wall time |

Preparing itself, from `preparing per frame, the submitting thread`:

- **22.1 ms in refreshes** -- 71 flushes of 9,966 ranges, **8,929 of them
  known to copy nothing**.
- 7.4 ms asking the write watch (0.5 of them finding what to ask).
- 9.4 ms of texture refreshes on all threads.

**The refresh term is the one worth taking next.** 22.1 ms over 9,966 ranges is
2.2 microseconds a range for ranges that do nothing. `Shadow::nothing_new`
(`shadow.rs:4086`) is what answers them, and it is not cheap: `pending`,
`copied` and `ranged` locks, then `watch::nothing_new` (`watch.rs:1629`), which
walks `published(start, end)` and the journal's notes page by page -- 16 pages
for each 64 KiB piece. Nothing has measured a cheaper way. `VICTORY_REFRESH_FILTER`
loses because `nothing_new` done one after another on the submitting thread
pays ~7 us a span where eight threads pay ~3, and `VICTORY_REFRESH_WIDE` is a
wash for the same reason.

**The larger term is the 80.2 ms GPU wait running strictly after the 40.3 ms of
preparing.** Overlap them and the frame is ~104 ms instead of 144. That is the
relay, and it is blocked on the device removal recorded in the section above.
### The relay: the card stops with nothing in flight

`VICTORY_RELAY=1` with `VICTORY_D3D12_DRED=full` (`rel2`). It lasted 66 s this
time and then:

```
19:39:44.151447Z  WARN  no host list for a prelude
  error=The GPU device instance has been suspended. (0x887A0005)
19:39:44.152513Z ERROR  device-removed data; breadcrumbs (done/count, list) []
19:39:44.152595Z ERROR  no host list for a state prelude; device removed ...
  The GPU will not respond to more commands ... (0x887A0007)
```

Two things to read out of that.

**`breadcrumbs []`.** Device-removed data was on with breadcrumbs and markers
(`crash tracing on (device-removed data) breadcrumbs=true markers=true`), and it
found **no breadcrumb node at all**. DRED only reports lists that were
submitted, so an empty list means nothing of ours was in flight when the card
went. The removal is therefore not a bad command: no command was running.

**0x887A0005, then 0x887A0007.** The first thing to fail is a submission
(`The GPU device instance has been suspended`), which is what a driver reset
looks like from the submitting side. `0x887A0007` (`DXGI_ERROR_DEVICE_RESET`)
follows on the next submission, the state prelude with no host list.

Together: the GPU stopped making progress, the driver reset it, and we found out
on the next submit. That is a **wedge**, not a fault -- a wait that can never be
satisfied, or a queue waiting on a signal that was never posted. It is the
branch the relay was being run to decide between, and it is decided.

**The wrapper eats `-More` commas.** `run2.ps1` splits `-More` on `;`, not `,`
(line 115). `-More 'VICTORY_RELAY=1,VICTORY_D3D12_DRED=full'` therefore set one
variable, `VICTORY_RELAY=1,VICTORY_D3D12_DRED=full`, and left DRED at the
table's `light`. Every earlier run that passed two switches comma-separated did
the same; `wc2`'s A/B worked only because `crate::ab` splits its own value on
commas. **Use `;` between switches.**
### The frame budget, as the lines actually print it

The section above was written from one heavy frame of `wc3` and its refresh
figure (**22.1 ms**) is not what a steady frame says. Corrected here, from the
steady lines in `log-wc3.err.txt` (19:33:49 and 19:33:50), so nobody quotes the
wrong number later.

```
6.1 fps, 163.9 ms/frame (31.0 preparing: copies 12.3 ms 31.4 MB,
  textures 0.8 ms 42.9 MB; 77.4 on the GPU; 4.9 copying back 1.0 MB)
```

| term | ms | share |
|---|---|---|
| preparing | 31.0 | 19 % |
| on the GPU | 77.4 | **47 %** |
| copying back | 4.9 | 3 % |
| the title's own recording | ~50.6 | 31 % |

**Preparing, split as the next line splits it:**

```
preparing per frame, the submitting thread: 7.2 ms asking the write watch
  (0.6 of them finding what to ask), 0.0 putting the title's bytes under
  flights' writes, 17.8 in the lists' steps (refreshes 11.4 in 65 flushes
  of 9166, 8641 of them known to copy nothing, 7805 on several threads,
  0.9 one at a time in 243 before shader writes; copies and fills 1.7),
  2.9 snapshots, 2.7 for copies in the card's memory, ...
  the checks behind those refreshes 9410 spans, 6.8 ms on all threads,
  9836.8 MB found clean
```

So the refresh filter is **6.8 ms of thread time**, not 22.1: 9,410 spans at
0.72 us each, and 7,805 of them run on the eight threads at once. The refresh
*term* on the submitting thread is 11.4 ms and includes the copies the other
525 spans do. **The refresh lever is worth about 7 ms, not 20.** It is already
most of the way to its floor; the earlier note sent the next session after it
with the wrong prize attached.

**The write watch is the bigger of our own terms, and it is nearly all waste:**

```
copies per frame: ... 541 watches 11.29 ms, 12832.8 MB checked,
  12802.2 MB unchanged
```

11.29 ms to walk **12.8 GB** of page table and find 0.2 % of it written. That
is 8 % of the frame for a rounding error of signal. It is already past
`ask_ahead`, `ask_once` and the guard bits; what is left is 541 askings of
~24 MB each, one per span that actually copies.

**The 77.4 ms on the GPU is the frame, and there is no lever on it that has
been found.** `VICTORY_GPU_TIMING` (needs `VICTORY_D3D12_DRED=full`) names six
compute dispatches at 25.9 ms between them; they are single dispatches in the
title's own shaders. The only remaining structural win is overlapping the
77.4 with the 31.0 -- the relay -- and that is the wedge in the section above.
### The write-combining skip strobes the screen: `wc3` against `wc4`

`313f0e9f` shipped `VICTORY_SKIP_WC_BACK` on by default on the strength of a
time measurement alone. `wc4` is the controlled run that says that was wrong.

**The warning.** `wc3` (skip on) logged **529** `a black frame after a frame
that was not`, alternating in pairs from frame 2068 on:

```
frame=2108 resource=0x2177b250d70 state=0x4 draws=1069 copied=true
  black_frames=24 levels=[130, 1, 129, 1, 130, 1, 129, 1, 129, 2, 128, 1]
```

Same resource every time, `state=0x4`, `copied=true`, gap median 2. The probe
(`capture::probe` -> `capture::black`) reads the texture that is being
presented, so this is not a dark scene: the presented frame itself alternates.

**The controlled run.** `wc4` is `wc3` with one thing changed --
`-More 'VICTORY_SKIP_WC_BACK=0'` -- and the same wrapper, tag shape and
`-Seconds 260 -Press 28,40,60,80,100,120`:

| run | skip | max frame reached | black-frame warnings |
|---|---|---|---|
| `wc4` | off | 2932 | **3** |
| `wc3` | on | 3152 | **529** |

`wc4` reached frame 2932, past the whole of `wc3`'s alternation window
(2068-3152), and logged only the three warnings every run logs in its first
frames. Across the ~150 logs on this machine the count is 0 to 14. And
`looks\wc4\150.png` renders correctly -- a cutscene frame, subtitles legible.

**What it costs to be correct.** `copying back` on the frame line:

| run | skip | copying back |
|---|---|---|
| `wc2`, world frames only | off | 21.36 ms / 23.41 MB |
| `wc2`, world frames only | on | 5.08 ms / 1.58 MB |
| `wc4`, a heavy frame | off | 36.5 ms / 36.5 MB |
| `wc3`, a heavy frame | on | 4.9 ms / 1.0 MB |

So 16 to 32 ms a frame is real, and it is still the wrong trade.

**Why.** `wc.rs`'s own module header names the artifact: "Where the title's CPU
writes part of a 64 KiB piece the card also wrote, the next upload sends the
card's stale bytes there". Leaving the piece on the card makes that happen for
*every* piece a shader wrote, not just the ones the title later touches. When
the title reads a buffer it fills with compute and hands back to the card, it
reads a frame behind. A frame that alternates black and bright is one of two
buffers being stale.

**The way back** is `7b606552`: default off, `VICTORY_SKIP_WC_BACK=1` still buys
the time for anyone who wants it. A real fix would need to know that the
title's CPU never reads a piece, and there is no such signal today.

**What this cost the session** is worth recording too. `wc2`'s A/B looked clean
because `wc2` never reached the scene -- max 5502 draws against `wc3`'s 8277 --
and because the warning count for the whole run was 6, which reads like the
startup noise every log has. The lesson is not "measure"; it is **count the
warnings, not only the milliseconds, and make sure the run reaches the scene**.
### Two ports from fxeless, and the one that proves itself

`fxeless/victory-fork` had two open pull requests on 2026-11-27 that touch
correctness rather than speed. Both are in.

**#1496, a placed page remembers its asking.** `memory::place()` uploads a page
of title memory at a submission and leaves `held.taken` at 0. The next
`bring()` reads that as "never taken" (`held.taken == 0` in the `stale`
expression) and schedules a whole-page upload over whatever the builder has
already written on the card. Stamping `taken` from `watch::writes` right after
the upload makes the first refresh see the page as current; a later title write
still reaches the copy because its asking is higher than the stamp.
`VICTORY_PLACE_TAKEN=0` keeps the old behaviour.

The test is upstream's, adapted to our shapes: an unchanged placed page uploads
nothing, and one title dword still lands. It is in `memory::tests` as
`a_placed_page_is_not_taken_again_until_the_title_writes`.

**#1495, the scalar-code table.** `ir::unspill` keeps `let mut used = [false;
128]` of scalar codes the program uses, and indexes it with `Reg::S(c)` where
`c` is a `u8`. A program naming a scalar code of 128 or more panics — measured
here as `index out of bounds: the len is 128 but the index is 215` at
`ir.rs:336` — and that shader never translates at all. `free[]` beside it was
already 256, so the table was the only one short.

**This is the one that proves itself.** With the old size the new test panics
and the suite goes red; with 256 it passes and `victory-shader` is 304 passed,
0 failed, 4 ignored. It is the first port in this repo that can be shown to fix
something without a frame of video.

**`wc5` verifies the shipped write-combining default.** No
`VICTORY_SKIP_WC_BACK` in the environment at all, so the default is what runs:

| run | skip | max frame reached | black-frame warnings |
|---|---|---|---|
| `wc4` | `=0` passed in | 2932 | 3 |
| `wc5` | default (off) | 2864 | 3 |
| `wc3` | default (was on) | 3152 | **529** |

The frame lines carry `write-combining skip off`. 98 warnings and errors over
the run, all of them the usual texture-index and unmirrored-view notes.

### The relay is closed: both stages wedge

`rel3` was the cheap third data point the last session left open —
`VICTORY_RELAY=2;VICTORY_D3D12_DRED=full`, 180 s asked for. It died at 65 s:

```
ERROR victory_gpu::host: device-removed data; breadcrumbs (done/count, list) []
ERROR victory_gpu::host: no host list for a state prelude; device removed:
      ... (0x887A0007)
```

Same shape as `rel2` (`=1`): breadcrumbs **empty**, then `0x887A0007`. DRED
only reports lists that were submitted, so an empty list means nothing of ours
was in flight when the card went. That is a wedge, not a bad command, and it is
the same wedge in both stages.

So the relay is not "blocked on a bad command" any more — it is **closed**.
Stage 1 waits for everything in the air, stage 2 waits exactly, and both hang
the card. The 40 ms of overlap it promised (144 → ~104 ms) is not reachable by
turning it on.

What would be left to try, if a later session wants it: `VICTORY_LATE_WAIT=3`
alone, without the relay, to see whether the late path itself wedges the card
or only the landing thread does. Nothing else in the relay's stack is
unmeasured.

### Heap pacing, ported and off (`Vice-Tay` #357)

The title's descriptors are rewritten for frame F+1 as soon as its present of F
returns, and our side brings them into the host heaps while the card may still
be drawing F — so F's draws can read F+1's views. That is the stretched
triangle ("wedge") artifact of `Vice-Tay` #90.

`host/heap_pace.rs` is the port: a submission of ours is prepared only when the
card has run the newest frame we sent (`sent()` takes the fence behind the
present's copy), with a 250 ms give-up so a frame that never ends cannot stop
us, and `descriptor::sync_early()` returns `None` so the heaps' rounds of their
own stop.

**It is off by default here** (`VICTORY_HEAP_SYNC_PACE=1` turns it on), which
is the opposite of upstream. Their own numbers are 17–19 fps down to 12–14 on
an RTX 2080 Ti, and this port's whole business is that the card and our
preparing already run at the same time — which is exactly the overlap this
build is short of. It is worth having as a switch for a frame that shows a
wedge, not as a default.

### The late path is what wedges the card, not the relay

`hp2` ran `VICTORY_LATE_WAIT=1;VICTORY_HEAP_SYNC_PACE=1` — the late path on its
own, without the relay, with the heaps paced. It died at 126 s:

```
ERROR device-removed data; breadcrumbs (done/count, list) []
ERROR host GPU device removed after 4 submissions sent ahead (VICTORY_LATE_WAIT=1)
      of 27 lists; breadcrumbs (done/count, list) []
```

**Four submissions sent ahead.** Same empty breadcrumbs as `rel2` and `rel3`.
So the wedge is not the relay's landing thread and not its waits — it is the
sent-ahead path itself, which the relay needs and which `VICTORY_LATE_WAIT`
turns on. That closes the last cheap question in the relay's stack: there is
nothing left to bisect, because the thing underneath it is already broken.

**Heap pacing held nothing.** `hp2` also ran `VICTORY_HEAP_SYNC_PACE=1` with the
late path on, and across 108 pacing lines in the world the count was:

```
heap pacing (VICTORY_HEAP_SYNC_PACE=1): 30.7 submissions a frame,
  0.00 held a frame, 0.00 ms a frame held, 0 gave up after 250 ms
```

Every one of them zero — max 0.00 held a frame over the whole run. So on this
title the card has already finished the newest frame we sent by the time the
next submission begins: our preparing and the card's drawing are already not
overlapping, which is the same thing the frame budget says from the other side
(preparing 31 ms, then 77 ms on the GPU, one after the other). The wedge
mechanism heap pacing exists to prevent cannot be happening here, and the
switch buys nothing. It stays in the tree, off, for a frame that shows a
stretched triangle.

That is worth stating plainly because it changes what the next session should
do: **there is no overlap to win back on this title's submission path.** The
frame is 31 ms of preparing plus 77 ms of card, in series, and the only levers
left are on the terms themselves.

### `land_card`: both suspects are ruled out, by running them

The two-switch experiment the earlier note left unrun has now been run, three
times, on `host::refresh_tests::landings_title_writes_and_light_refreshes_at_once`:

| switches | result |
|---|---|
| none (baseline) | FAILED |
| `VICTORY_SEND_CHANGED=0` | FAILED |
| `VICTORY_LAND_ECHO=0` | FAILED |

So neither the whole-page road nor `take_landed` is what puts the shader's dword
into the skip list. The failure is unchanged in all three:

```
not landed: expected at least 6 at offset 0xc50c (class 3), found 4
```

`0xc50c % 32 == 12`, and class 3's lane is `4 * 3 == 12`, so it is the shader's
own dword — the title never wrote it. Title memory still holds the older `4`.

**A lead the experiment leaves standing.** `bring()` only resends a page when it
has never been taken (`if never { shadow.resend(base, base + PAGE) }`), so the
whole-page road cannot explain a page the test has written to for several
rounds — which is the same reason suspect 1 was weak. What is left is
`sent.near(fetched_after, from, to, &mut skip)`: `dirty` is filtered by the
fetch's send number, and if a range in it has not actually gone up yet, it is
skipped as though it had. That is suspect 2, and it is the only one still
standing.

Nothing here is a fault in the game: the consequence is a stale dword where the
title and a shader write the same 32 bytes, invisible unless something reads it.

### The baseline at pure defaults (`base1`)

Every comparison this session has been against a different scene, and the draw
count swings 400 to 8,300 inside one run. `base1` is the reference the next runs
measure against: no `-More` at all, the wrapper's own table only, 400 s,
`-Press 28,40,60,80,100,120`.

| | world (>= 200 draws) |
|---|---|
| frames | 60 |
| median | **206.6 ms, 4.84 fps** |
| draws, median | 2,908 |
| preparing | 51.6 ms |
| copies | 25.1 ms |
| on the GPU | **98.1 ms** |
| copying back | 24.1 ms |
| ms per 1,000 draws | 71.0 |

Peak working set 21,370 MB. Best fps of the run 33 (the menu's cap). No device
removal, no panic, no `ERROR` line.

Preparing, as its own line splits it:

```text
preparing per frame, the submitting thread: 7.4 ms asking the write watch
  (0.6 of them finding what to ask), 0.0 putting the title's bytes under
  flights' writes, 26.3 in the lists' steps (refreshes 19.1 in 64 flushes of
  9426, 8465 of them known to copy nothing, 8355 on several threads, 1.2 one
  at a time in 315 before shader writes; copies and fills 1.9), 3.1 snapshots,
  6.6 for copies in the card's memory; the checks behind those refreshes
  9740 spans, 8.4 ms on all threads, 9605.8 MB found clean
```

and the copies:

```text
copies per frame: 46.0 submissions; 11486 title copies 6.2 MB; 22429
  refreshes 18100.2 MB; 589 watches 24.24 ms, 13449.2 MB checked,
  13381.6 MB unchanged
```

**Two things this settles.**

1. **The write watch is bigger here than the handoff's frame said** -- 24.24 ms
   of a 206.6 ms frame (12 %), not 11.29 of 137. It scales with the number of
   copies, and this scene makes more of them: 589 askings of about 23 MB each,
   13.4 GB walked to find 0.5 % written.
2. **The card is idle half the frame, and not idle behind the title.** The
   card's own clock line:

   ```text
   nothing of ours queued 77.8 ms (the card about 54% busy by this count),
   67.1 ms of that while the submitting thread was working (the card idle
   behind us), the rest while the title had not submitted; the submitting
   thread stood waiting for the card 93.6 ms
   ```

   67.1 ms of the card's idle time is inside our own preparing. That is the
   same statement `hp2` made from the other side (heap pacing held 0.00
   submissions a frame): our preparing and the card's drawing are in series,
   and the 40-75 ms of overlap the relay promised is still the biggest single
   item on the table -- and still unreachable, because the sent-ahead path
   wedges the card.

The copy-back is 24.1 ms here against the 4.9 ms the handoff's table quotes,
because that table is `wc3`, which ran the write-combining skip on. At the
shipped default every byte a shader wrote comes back.

## Where the 98 ms on the card goes (`tim1`)

The baseline left one question open: 98.1 ms a frame on the card against a
card that is only 54 % busy. The handoff's item 1 asked whether that time is
ours -- a translation fault in the hot compute dispatches -- or the title's
own work. `tim1` ran 300 s with `VICTORY_GPU_TIMING=1`, `VICTORY_D3D12_DRED=full`
and `VICTORY_SHADER_SAVE`, the title reached the world at 68 s and then closed
itself at 175 s with no panic, no device removal and no breadcrumb.

The timing run prints a block per flush: the card's time per command, in
milliseconds a frame, over the window the flush covers. Summing the blocks in
the world phase reproduces the 98 ms the frame line reports, so the
attribution is complete rather than a sample. Four commands carry 14 ms of it:

```text
5.4 ms  x1.00  dispatch [compute 39f0e9aeb156a61c]
3.5 ms  x1.00  indirect [compute 6697dff15287e544]
2.6 ms  x1.00  indirect [compute bc6ca78d5425c179]
2.5 ms  x75    alias copy
```

and the remaining ~84 ms is spread over the title's 2,908 draws at 30 us each,
with no single draw above 0.55 ms. That distribution is the answer: there is
no hot command of ours to fix, because our copies are 2.5 ms of the 98.

**The six hottest computes translate tighter than the console.** `tools\shader-audit.ps1`
beside `victory shader-info --disasm` and `dxc -dumpbin`:

```text
Shader           Console  Ours Ratio Barriers Phi Loads Calls  Brs Threads Wave Scratch
9938c251a313fe28    1379  3308   2.4        0  65    12  1090   73 8,8,1
418f3fdf020b9eda    2142  5125  2.39        0 326    69  1147  294 32,1,1
deec5fb86d9cc5f3    1116  2598  2.33        0 167    67   627  155 32,1,1
6697dff15287e544    6352 14517  2.29        0 603   271  4936  317 8,4,1   32
8b9483919d8d5e99    1332  3041  2.28        0  89    18   960   82 4,4,4
915ed81ea580a276    3614  8255  2.28        0 366    93  2429  252 8,4,1   32
cf7a9d328289d2a8    1617  3552   2.2        0  60    11  1163   69 8,8,1
34fb51b8a0312e82    1416  3042  2.15        0  88    35   935   98 8,4,1
5cf00dc39042edaf    6177 12682  2.05        0 321   248  3600  394 4,8,1   32
39f0e9aeb156a61c   74594 15027   0.2        0 546   492  2028 4063 32,1,1  32
```

Zero group barriers in any of them, so nothing of ours is serialising the
card. The wave size matches the console's on every one that declares it. The
one that is five times smaller than the console is the one that costs 5.4 ms:
`39f0e9aeb156a61c` is 74,594 instructions of straight line on GFX10 -- a fully
unrolled loop, 333,760 bytes of native Cs, dispatched 920x1x1 for 29,440
threads over a 448,000-element buffer -- and our DXIL keeps the loop instead,
4,063 branches and 15,027 instructions. We emit a fifth of the console's code
and the card still takes 5.4 ms, which is what 15 elements a thread through
that much arithmetic costs. It is the title's shader doing the title's work.

**What this rules out and what it leaves.** No barrier was added, no loop was
unrolled into more code than the console had, no load was duplicated into a
redundant one -- the three faults item 1 was looking for are absent from every
shader that matters. The 98 ms is the title's own rendering load at 2,908
draws, and the lever that would hide it is overlap, which is closed.

The preparing side is where our time still is: 51.6 ms, of which 26.3 is the
lists' steps (19.1 of them refreshes across 64 flushes of 9,426 entries, 8,465
of those entries known to copy nothing) and 7.4 is asking the write watch.
9,605.8 MB is walked each frame to find 0.5 % written. That is item 2, and it
is now the largest thing on the frame that is ours.

## The frame is preparing, then the card, then the copy back -- added up

Four runs at shipped defaults (`base1`, `landfix`, `split1`, `sendchg`; the
last with `VICTORY_SEND_CHANGED=1`), reading the frame line against the card's
clock. The three terms the line gives add up to the frame, in every run:

```text
Run      draws  ms/frame  preparing  GPU   copy back  the three   the rest
base1     3915     168.1       42.8   90.8       20.6      154.2       13.9
landfix   1321     134.5       39.8   70.0       18.2      128.0        6.5
split1    1385     130.7       37.2   71.2       16.7      125.1        5.6
sendchg   1093     153.6       45.8   82.1       20.1      148.0        5.6
```

The rest is the present and the title's own frame bookkeeping. What is left
out of the three is a handful of milliseconds, so **preparing is not hidden
under the card at all**: a millisecond taken off the lists' steps is a
millisecond off the frame. The card's clock says the same from the other side:

```text
base1    nothing of ours queued 77.8 ms (the card 54 % busy), 67.1 ms of it
         while the submitting thread was working (the card idle behind us);
         the submitting thread stood waiting for the card 93.6 ms
landfix  nothing of ours queued 58.0 ms (the card 56 % busy), 53.2 ms of it
         while the submitting thread was working; waited for the card 74.6 ms
```

The card is idle 44-46 % of every frame, and half of the frame's idle time is
spent while we prepare. The handoff's item 1 asked where the 77-98 ms on the
card goes; the answer stands (the title's own load, `tim1`), but it is not the
wall on its own: at these draw counts 37-46 ms of the frame is preparation, and
it is serial with the card rather than under it.

### What the copies are, by size

`VICTORY_COPY_SPLIT=1` splits [`copystat::moved`] into size buckets. At
defaults, per frame:

```text
<=64B    544 calls  0.01 ms   0.0 MB
<=256B  1031 calls  0.01 ms   0.1 MB
<=1KiB  3380 calls  0.02 ms   1.9 MB
<=4KiB   509 calls  0.23 ms   1.8 MB
<=64KiB  879 calls  5.36 ms  27.2 MB
<=1MiB   203 calls 12.50 ms  46.9 MB
        6545 calls 18.13 ms  78.0 MB   2,770 ns a call, 4.3 GB/s
```

The small copies are free: 4,955 calls under 1 KiB cost 0.04 ms between them,
so the per-call overhead the `VICTORY_JOIN_COPIES` note worried about is not
there any more. All of the time is bulk movement -- 74 MB in 1,082 calls -- and
it runs at 3.8-5.1 GB/s, which is what reading cold title memory and writing it
into the copy costs. There is no overhead left to remove here; the only lever
is to move fewer bytes.

`VICTORY_SEND_CHANGED=1` (a closed lever, measured again and still left off)
says how much of those bytes is padding: 54.6 MB compared, 28.2 MB of it found
as it was, 6.8 MB going up in 116,833 runs. It does halve the movement -- the
same 78.0 MB, but `moves` fell from 19.39 to 9.95 ms, 7.8 GB/s against 4.3 --
and it is still a loss, which is why it stays off: the comparing is not free,
and the frame's own `copies` term went *up*, 18.0 to 26.4 ms, with preparing
39.8 to 45.8. Reading 54.6 MB to find out that 28.2 MB of it is already there
costs more than moving the 28.2 MB twice. The bytes are the cost either way;
there is nothing to be won by looking at them first.

### Where that leaves item 2

At 1,321 draws: preparing 39.8 ms, of which copies 18.0, the lists' steps 21.1
(refreshes 16.5 across 55 flushes of 5,770 entries, 4,980 of them known to copy
nothing), asking the write watch 5.5, card copies 7.3, snapshots 2.6. On all
threads beside that, the guards: 15.3 ms waiting for them, 7.5 ms changing
protections, 736 blocks made read-only. And 8,990 MB is walked by the write
watch each frame to find 21 MB written.

## A bit keyed by address outlives the memory it was noted for

`watch::tests::what_is_taken_again_is_what_was_written` failed about one run
in four under the whole suite and never once in thirty runs alone. The panic
was at `track(p, 4 * PAGE, false).expect("watched")`, and `track` answers
`None` when `writes` does.

The test now says why instead of guessing, and one run under the suite gave:

```text
not watched: light true skip_view_ask true, refused_as_view true,
protected [], views [23078131], published false, committed true
```

`VIEWS` is a bit per 64 KiB block, keyed by **address**, and it outlives the
memory it was noted for: the runtime maps a section, the guards note its
blocks, the section goes, and the address comes back as an ordinary
`VirtualAlloc` -- with the bit still set. `refused_as_view` then says "view
memory, the kernel would refuse" and `writes` answers `None` without asking.
That is fail-closed and it self-heals on the guarded path (`guard` finds it no
view and clears the bit), so production loses a range's watching for a frame
or two and copies it whole. The test could not tolerate it at all.

The five tests that need a write-watched range now take one from
`watch::write_watched(pages)`: it allocates, asks `is_view` to confirm a fresh
allocation is no view, and clears the `PROTECTED`/`VIEWS` bits of the blocks
it got -- a true statement, because a fresh allocation is ours alone and no
live view can be sitting on it.

### What the guard machinery costs, when the lines are read together

`log-landfix`, a steady world frame at 1,321 draws:

```text
guards 17.9 ms                                    (all threads, per frame)
waits for the write trackers' locks per frame: ranges held 10 0.96 ms,
  journal read 109 1.24 ms, journal noted in 52 0.40 ms, guards 259 29.68 ms
176 view ranges answered block by block; 736 blocks made read-only;
  235 opened by ourselves; 15.3 ms waiting for the guards,
  7.5 ms changing protections
```

So the guard machinery is **47 ms of all-threads time a frame, and 30 of it is
waiting for its own lock** -- 259 waits at 115 us each. The lock is the
`GUARDS` write lock (renderer port #6, `VICTORY_PORT_GUARD`, is off), and it is
held across the `VirtualProtect` work: 736 blocks at ~10 us each is 7.5 ms of
pure holding a frame, and every title thread that writes a guarded block
queues behind it. The port-6 claims machinery exists to make that hold short
and is closed; without it, the wait is the cost, not the walk.

The walk itself is small: 40 MB ranges are 640 blocks, ten words of the
`clean` set, so the per-word scan is ~1,760 words a frame. The 17.9 ms of
"guards" is 736 `cover()` calls (each taking `inplace::VIEWS` to find the
other views of the page) and 736 `VirtualProtect` calls, not a scan over
gigabytes. The 8,990-12,833 MB that `copystat` reports as "checked" is the
range the askings cover, not memory that is walked.

## The whole frame on a timeline, and where the scheduler sits (`tl-fl`)

`VICTORY_TIMELINE=120,8,E:\GTA6_Frutiger\tl-fl.json` at shipped defaults, in
the world: eight frames of 145.3 ms, 81,668 events. Per frame, by cause, the
chain each present came out of:

```text
    50.04 ms  the card running submissions
    15.57 ms  the title's CopyPageMappings
     8.58 ms  woken, not running yet
     7.07 ms  a job for another thread
     5.09 ms  work spread over threads
     4.21 ms  sent, the card not started yet
     4.08 ms  copying back: into title memory
     3.77 ms  streaming: a request
     3.45 ms  outside our spans on 11084
     3.32 ms  a wait nobody seen here ended (WaitForSingleObject)
     3.17 ms  kernel: GetWriteWatch
     2.68 ms  submit: the lists' steps
     2.62 ms  a long list call
     2.50 ms  a copy's refresh
     2.39 ms  resetting a host list
     2.20 ms  submit: sending
     1.80 ms  beside: the heap's views
     1.74 ms  outside our spans on 23028
     1.67 ms  a texture's refresh
     1.49 ms  asking: ahead
     1.36 ms  the title's Present
```

Three things it settles.

**The frame is CPU-bound, and the card is idle most of it.** 50.04 ms of 145.3
is the card running our submissions; the other 95 is threads. That is §"The
frame is preparing, then the card, then the copy back" read from the other
side, and it is the reason tuning the card's work does not move the frame.

**The title's own page mapping is the largest single CPU item, and it is our
lock.** `CopyPageMappings` is 15.57 ms of the title's thread, and §"the guard
machinery" has 30 ms of every frame waiting on the same `GUARDS` write lock
that mapping queues behind. Those are one cost seen twice: the mapping is not
slow in itself, it stands in the queue our `VirtualProtect` work makes.

**20.7 ms of the frame is threads woken and not running** (8.58 woken, 7.07 a
job handed to another thread, 5.09 work spread over threads). This is not
saturation: the thread table sums to about 690 thread-ms a frame against
2,320 available on eight cores with SMT, so the machine is 30 % busy. It is
wake-up latency, and part of it is the instrument: `victory-sample`, the
timeline's own sampling thread, **ran 134.1 ms of every 145.3 ms frame** --
one core held for the whole run. These figures carry their own overhead and
should be read as an upper bound on the scheduler's share.

### The processor pinning does nothing on this machine

`cores.rs` picks the fast processors two ways: by efficiency class, and, where
every class is alike, by the largest last-level cache. A Ryzen 7 5800X
reports one class and one 32 MB L3 shared by all eight cores, so `fast_by_class`
and `fast_by_cache` both answer empty, `ask` returns before it opens a thread,
and `settle` is never reached. `VICTORY_FAST_CORES` and `VICTORY_LANE_CORE` are
no-ops here; both are for the 7900X3D/7950X3D class of part. Worth knowing
before reading a run as evidence for or against them.

### A texture's refresh is 1.67 ms of the chain, not 45

The `texture refreshes per frame, all threads: 45 ms` line is
`REFRESH_WAITS[3]`, summed over **every thread and every texture**, and the
same span on the chain is 1.67 ms. The 25-56 ms is real CPU, but it is spread
over the wide threads and lands beside the frame rather than in it. It is not
the lever the all-threads figure suggests, and the guard work above
(`VICTORY_TEXTURE_GUARD`, `HASH_EVERY = 8`) is already what keeps it there.
