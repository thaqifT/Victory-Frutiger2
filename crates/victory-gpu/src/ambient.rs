//! A stand-in for the indirect light the title traces rays for.
//!
//! The title's indirect light is ray traced. With the structure builder off
//! (`crate::pipeline::builder_runs`) every ray misses, and the program that
//! resolves the diffuse light of its screen probes
//! (`CS_ScreenProbes_TemporalResolve_*`) leaves the sky's light outdoors and
//! nothing indoors: a wall, or a face, that no lamp or sun reaches directly is
//! black.
//!
//! `VICTORY_AMBIENT=<level>` puts a constant in its place, written into the
//! image that program has just written: every texel takes the greater of the
//! two, so the sky the title found outdoors stays and the black indoors is
//! lifted. That the image is the one to write, and where it sits in the
//! program's tables, is beloooo's find (VIctory-Belobug-AMD, commits 06031237,
//! 4f45ab88, cd9f78ce, GPL-3.0-or-later), which writes a constant there too.
//!
//! The image is R9G9B9E5 shared exponent, held as 32-bit words. The stand-in
//! reads a word, lifts it, and writes it back, so nothing else about the
//! program's output changes.

use std::sync::OnceLock;

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

use crate::host::{HostList, host};

/// The program the stand-in follows: the one that resolves the diffuse light
/// of the title's screen probes.
pub(crate) fn is_resolve(container: &[u8]) -> bool {
    const NAME: &[u8] = b"CS_ScreenProbes_TemporalResolve";
    container.windows(NAME.len()).any(|w| w == NAME)
}

/// Where that program writes the diffuse light: root parameter, and the
/// dword of its table.
pub(crate) const LIGHT: (u8, u32) = (1, 176);
/// The scene's depth, read by the same program: root parameter and dword.
/// Only used to check the image found is the one the depth belongs to.
pub(crate) const DEPTH: (u8, u32) = (0, 464);

/// The light the stand-in puts in, or 0 when it is off
/// (`VICTORY_AMBIENT=<level>`).
pub(crate) fn level() -> f32 {
    static LEVEL: OnceLock<f32> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("VICTORY_AMBIENT")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(0.0)
    })
}

/// How far above the stand-in's light what the title's program left may
/// count (`VICTORY_AMBIENT_MOST`, default 4). With no rays its resolve
/// leaves blocks of the probes' memory, which are bright garbage: this is
/// what keeps them from taking the picture.
pub(crate) fn most() -> f32 {
    static MOST: OnceLock<f32> = OnceLock::new();
    *MOST.get_or_init(|| {
        std::env::var("VICTORY_AMBIENT_MOST")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 1.0)
            .unwrap_or(4.0)
    })
}

/// Whether the stand-in runs: asked for, and the title's rays find nothing.
pub(crate) fn on() -> bool {
    level() > 0.0 && !crate::pipeline::builder_runs()
}

const SOURCE: &str = r#"
// A typed UAV cannot be a root descriptor; it comes through a table.
#define RS "RootConstants(num32BitConstants=4, b0), DescriptorTable(UAV(u0))"
cbuffer Set : register(b0) {
    float level; float most; uint width; uint height;
};
// The diffuse light: R9G9B9E5, one word a texel.
RWTexture2D<uint> light : register(u0);
static const float3 LUMA = float3(0.2126, 0.7152, 0.0722);

float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float top = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(top, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (top * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}

[RootSignature(RS)]
[numthreads(8, 8, 1)]
void raise(uint3 at : SV_DispatchThreadID) {
    if (at.x >= width || at.y >= height) return;
    uint2 p = at.xy;
    float3 found = float3(1.0, 0.98, 0.94) * level;
    // What the program left counts as far as it can be true. Without rays
    // it is the probes' uninitialised memory in blocks of tiles, brighter
    // than anything in the scene: unbounded, it is the picture.
    float3 was = unpack(light[p]);
    float top = max(dot(found, LUMA) * most, 1e-6);
    float has = dot(was, LUMA);
    was *= min(1.0, top / max(has, 1e-6));
    light[p] = pack(max(was, found));
}
"#;

struct Pass {
    root: ID3D12RootSignature,
    raise: ID3D12PipelineState,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn pass() -> Option<&'static Pass> {
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    PASS.get_or_init(|| {
        let host = host()?;
        make(&host.device)
            .inspect_err(|e| tracing::warn!(error = %e, "no stand-in for the indirect light"))
            .ok()
    })
    .as_ref()
}

fn make(device: &ID3D12Device) -> windows::core::Result<Pass> {
    let code = crate::host::compile(SOURCE, s!("raise"), s!("cs_5_1"))?;
    // SAFETY: plain creations; the root signature is the shader's own.
    unsafe {
        let root = device.CreateRootSignature(0, &code)?;
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: code.as_ptr().cast(),
                BytecodeLength: code.len(),
            },
            ..Default::default()
        };
        Ok(Pass {
            raise: device.CreateComputePipelineState(&desc)?,
            root,
        })
    }
}

/// What the stand-in reads and writes: the image the title's program just
/// wrote, as a view in the heap the list has set, and its size.
pub(crate) struct Image {
    pub(crate) view: D3D12_GPU_DESCRIPTOR_HANDLE,
    pub(crate) size: [u32; 2],
}

/// After the title's program, in `list` with the program's view heap set.
/// Replaces the list's compute root signature and pipeline; false if it
/// recorded nothing.
pub(crate) fn after(list: &HostList, image: &Image) -> bool {
    let Some(pass) = pass() else { return false };
    let level = level();
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        return false;
    }
    let words = [level.to_bits(), most().to_bits(), width, height];
    // SAFETY: an open list with the heap of the view set; the title's
    // program has just written the image, so it is in the state the view
    // needs.
    unsafe {
        let l = &list.list;
        let barrier = || D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            ..Default::default()
        };
        l.ResourceBarrier(&[barrier()]);
        l.SetComputeRootSignature(&pass.root);
        l.SetComputeRoot32BitConstants(0, 4, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(1, image.view);
        l.SetPipelineState(&pass.raise);
        l.Dispatch(width.div_ceil(8), height.div_ceil(8), 1);
        l.ResourceBarrier(&[barrier()]);
    }
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!(level, size = ?image.size, "a stand-in for the indirect light");
    }
    true
}
