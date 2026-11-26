# Ray tracing in this fork: what the title does and what has been built

Status, 2026-11-26: **nothing is lit by ray tracing yet.** All 53 builder
programs the title creates now have host pipelines and the builder runs end to
end, but a run with it on dies inside the title at world entry. Everything here
is off by default.

## What the title does

The title's indirect light (probes, bounce, reflections) and the
shadows of its lamps and sun are ray traced. Without them, anything the sun
does not hit directly is black: interiors, the shadowed side of a face, rooms
lit only by lamps.

The title does not use a ray-tracing API:

- Traversal is ordinary compute-shader code: a per-thread stack in shared
  memory, and loads through raw 40-bit addresses of title memory.
- The static world's acceleration structures ship pre-built in the title's
  data, in the console GPU's native node format (64-byte nodes: box nodes with
  four children and 16-bit float bounds, 128-byte box nodes with 32-bit float
  bounds, triangle nodes, instance nodes). They are unpacked on the GPU.
- A set of builder programs (from the title's `XboxBvhBuilder.dll`, 52 compute
  programs) builds and refits the dynamic structures on the GPU every frame.
- Only the per-node test is hardware: the instructions
  `image_bvh64_intersect_ray` and `image_bvh_intersect_ray`. One call tests a
  ray against a box node's four children (returning the hit children, nearest
  first) or against a triangle node (returning the intersection's numerator,
  denominator and barycentric numerators).

So mapping this onto the host's own ray tracing (DXR) is not practical: the
layer would have to know the geometry behind structures the title never
describes to it, and the traversal loops live in 64 of the title's own
programs. This fork runs the title's builder and traversal as translated
shaders and emulates the one instruction.

## What this fork adds

1. **The builder programs translate** (53 of 53 that the title creates in the
   scenes tested; run 2026-11-26 prints `the ray-tracing structure builder
   runs: all of its programs have host pipelines pipelines=53`). They pass
   buffer addresses in a spilled parameter table rather than in descriptors.
   Added: the spilled user-data table, root unordered-access views (translator
   and host binding), byte and 16-bit buffer accesses, and several control-flow
   shapes.
2. **Title memory by raw address from shaders.** A directory with one entry per
   64 KiB of title address space leads a shader to the layer's host copy of
   that memory; loads, stores, atomic add/sub and float min/max go through it.
   Before a dispatch the host makes ready the memory its root views and root
   constants name.
3. **The ray test, emulated in DXIL.** Four slab tests and a sorting network
   for box nodes; a watertight ray/triangle test for triangle nodes; both forms
   of the instruction. A plain Rust reference lives in
   `crates/victory-shader/src/bvh.rs`; tests run the emitted DXIL on the GPU
   over 311 ray/node cases and compare with the reference bit for bit.
4. **The trace and shading programs translate** (59 of 64 trace programs, and
   the irradiance-cache shading program). This needed lane masks treated as
   plain numbers on some paths, and exact forms for programs written for
   64-lane waves running on a GPU with 32-lane waves.
5. **A safety rule on the host side.** The builder's stages run only while
   every builder pipeline the title has created has a host pipeline; otherwise
   none run (`VICTORY_BVH_BUILDER=1` asks for it; `VICTORY_BVH_QUARANTINE=1`
   also keeps the builder's stores out of title memory, for a first test). A
   partly working builder wrote garbage bounds into title memory and crashed
   the title.

## Known gaps

- **The builder runs but the title dies.** With `VICTORY_BVH_BUILDER=1` all 53
  stages run and the title hits `unhandled exception 0xc0000005` at world
  entry, reading `0xffffffffffffffff`. Four experiments rule out the obvious
  causes:
  - `VICTORY_RELAY=0` (copy back synchronously instead of relaying through the
    card's own dirty ranges): still dies.
  - `VICTORY_WRITE_SKIP=0` (declare every bound view written instead of only
    the unordered-access ones): still dies.
  - `VICTORY_BVH_QUARANTINE=1` (keep every builder store out of title memory):
    survives, but the world is destroyed — the title reads structure sizes and
    counters back and finds whatever was there before, so the meshes explode
    into staircase patterns (`cap/shot-0036.png`).
  - No `title memory by address: shaders touched pages that had no copy` line
    fires in any run, so the pages the builder writes were ready.
  So at least one stage stores to a wrong address (or a wrong value), or a
  store that should reach title memory is not copied back over the range the
  title reads. Not yet localised.
- Five shadow-mask trace programs do not translate yet.
- An access to a title page that has no host copy is lost (a read gives zero, a
  store is dropped).
- Not publicly documented, and assumed: the instruction's rounding, the order
  of two boxes entered at the same distance, and the result for a ray exactly
  through an edge.
- Cost on the GPU is unmeasured.

## Where to look

- `crates/victory-shader/src/bvh.rs`: the reference for the node test.
- `crates/victory-shader/src/emit/lower.rs`: the lowering of the two
  instructions and of memory by address (`Lower::memory`).
- `crates/victory-shader/src/resource.rs`: address tracking for descriptors
  and raw addresses.
- `crates/victory-gpu/src/pipeline.rs`, `draw.rs`: the builder rule and the
  host side of root views and title memory.
