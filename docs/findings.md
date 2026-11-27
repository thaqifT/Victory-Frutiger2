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