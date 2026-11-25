# What DexNom's runtime does that ours does not

Read-only study of `DexNom/VIctory` (553 stars, 545 commits, the furthest
public attempt). **Nothing is grafted.** Their tree is a different fork —
it carries the zslayer `victory-world` restructure of `victory-shader`
(`lower/` split, `own_bits.rs`, `spill.rs`, `intersect.rs`, `packed_mask.rs`,
`guest_address.rs`), so a merge would replace our translator wholesale.
That is the Belobug mistake. What follows is per-fix: what it does, why it
matters here, and whether we took it.

Their base is upstream Marvisak at `4118f33`. From there to their `main`
is **17 commits, 182 files, +56k/−36k**. Almost all of that is the
zslayer restructure, not fixes. The fixes are six commits.

## The six fix commits

| commit | subject |
|---|---|
| `cd94d35` | shaders mark the pages they write through views; copy-backs compare only those |
| `940d32f` | a marked page's copy-back does not save the page for its own copy |
| `76749ce` | voxel visibility shader when a wave64 mask is restored from scalar copies |
| `ddb7332` | 40 shader variants, integer-target blending |
| `5f4c3df` | City texture streaming, Radeon shadow pipelines |
| `921d8d3` | pixel quad sampling, ray buffer views, sparse textures |

## Taken

### 1. A sparse texture is not an absent texture — `921d8d3`

**Their finding.** `mapped_bytes` answered "none of it is there" whenever
the *first and last* page of a texture were unmapped, without looking at
the pages between. A streamed texture routinely arrives with only its
interior mips resident. Both ends away therefore read as *streamed out*,
and the slot was dropped and its host view let go — for as long as the
ends stayed away.

**Our code had the identical shortcut** (`texture.rs:1048`, before this
change) and the identical consumer: `draw::heap_textures` treats
`mapped == 0` as "streamed out" and continues, so the object drawn from
that texture is black.

**Landed.** `mapped_bytes` now only trusts the ends when the whole range
lies in one 4 KiB page. Both ends away costs one `VirtualQuery` walk of
the pages between, which finds the interior run and stops — cheap, and it
is exactly the case that used to cost a full 64 KiB-block scan. The walk
also skips `PAGE_NOACCESS`/`PAGE_GUARD` regions, which it counted as
readable before.

This is our black-hull / black-prop / empty-UI-icon class.

### 2. An integer target may not blend — `ddb7332`

**Their finding.** The console bypasses the blender on integer colour
surfaces even when the draw asks for blending. D3D12 validates blend
enable against every active target format at pipeline creation and
refuses the whole pipeline with `E_INVALIDARG`.

**Landed.** `GraphicsState::apply_integer_blend_bypass`, called from
`create_stream` before the state is handed out. With shared blending it
expands slot zero into every target first, so clearing one target's blend
does not overwrite the others' write masks.

Note this is *not* what refuses our one bad mesh pipeline: its targets are
`R32_FLOAT`, `B4G4R4A4_UNORM`, `R9G9B9E5_SHAREDEXP` — none integer.

## Read, not taken — and why

### 3. Per-range remap pinning — `5f4c3df`

Their `title_memory_reading()` became `title_memory_reading_range(start..end)`:
a texture refresh pins only the span it is about to read, instead of
waiting for *all* title-memory remapping to finish. Windows removes whole
physical views on a partial remap, so their range includes those old
extents.

Worth taking next: our `refresh_timed` still takes the global gate at
`texture.rs:738`. Their measurement is that unrelated remaps no longer
stall an upload. Our per-frame texture phase is 62.8 ms with 0.0 ms spent
waiting on another *texture*, so the remaining stall is this gate.

### 4. Sparse uploads mid-stream — `5f4c3df`

`committed()` gained a partial-mapped path so a texture with only middle
pages resident still uploads what is there. We already do this
(`texture.rs:740-774`) but throttled to one retry per 30 frames. Their
version retries sooner. Left alone for now: item 1 removes the case that
mattered.

### 5. Static sampler chosen across a uniform branch — `ddb7332`

Their `image()` handles a sampler descriptor that arrives as a
`Source::Phi` of two constants: sample both ways, select the texel on the
edge that supplied the console's sampler. Recovered 40 shader variants
that had been failing to translate.

Ours (`lower.rs:9679`) takes `.get(&(site, true))` and passes whatever it
finds — including a `Phi` — straight to `handle(...)` as a descriptor.
That is wrong, but it does not currently bite: a full run has **one**
shader that does not translate (a compute shader) and one refused mesh
pipeline, out of 19,114 containers. Take it when a sampler-shape bug
shows, not before.

### 6. Copy-back page marking — `cd94d35`, `940d32f`

Their shaders mark the guest pages they write through views; the copy-back
compares only marked pages, and a marked page's copy-back does not save
the page for its own copy.

We compare by range (`copystat` prints the top ranges) and pay 23.3 ms
plus 35.9 MB of readback per frame. This is the right fix for that, and
it is a large change: it needs the translator to emit page marks. Queued
behind the texture work.

### 7. Four CPU submission workers — `VICTORY_FILL_WORKERS=4`,
`VICTORY_PARALLEL_PRELUDES=1`

Eligible upload batches across four workers; independent transition lists
recorded on a shared pool, with the final queue, fences and transitions
keeping the game's submission order. Default off in their launcher.

Ours is a single submitting thread: 45 submissions a frame, 1324.8 ms
between them, 469 ms of that in list calls. This is the largest single
perf lever we have and the most invasive. Not now.

## What they have not solved either

- **Dock objects still black** in their City save, with the ray-structure
  builder off. Their README says so plainly. Our black-object class may
  overlap; item 1 is the first time we have a mechanism for it.
- **Interior lighting** still wrong.
- RT frame times still reach seconds.

## Numbers from our last run, for comparison

7031 draws/frame, 1.2 fps, 841 ms/frame: 165 ms preparing, 57.5 ms GPU,
23.3 ms readback, 469 ms recording list calls. 52,368 list calls a frame.
18,536 buffer views reshaped for the host. 12,874 texture slots queued,
4,683 waiting for memory. 54,580 title textures with no host copy.
VRAM 12,729 of 14,940 MB.
