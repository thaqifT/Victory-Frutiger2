//! Triggers a RenderDoc capture from inside the process.
//!
//! `renderdoccmd capture` loads the API but not the in-application overlay, so
//! the F12/PrintScreen capture keys have no listener and never fire. The API
//! itself is still live, and `TriggerCapture` is a single call, so the reliable
//! way in is to ask from here.
//!
//! `VICTORY_RDOC_FRAME=<n>`: capture the next frame after `n` is presented.
//! `VICTORY_RDOC_PATH=<template>`: where RenderDoc writes the `.rdc`.
//!
//! Nothing happens when the variable is unset, and nothing happens when
//! `renderdoc.dll` is not in the process, so this is free in a normal run.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
}

/// `eRENDERDOC_API_Version_1_6_0`. Any 1.x API is layout-stable for the slots
/// below, so asking for 1.6 and indexing is safe on a newer runtime.
const API_VERSION_1_6_0: u32 = 10_600;

// Slot numbers in `RENDERDOC_API_1_7_0`, counted from the top of the struct.
const SLOT_SET_CAPTURE_PATH: usize = 11;
const SLOT_GET_NUM_CAPTURES: usize = 13;
const SLOT_TRIGGER_CAPTURE: usize = 15;

type GetApi = unsafe extern "system" fn(u32, *mut *mut c_void) -> i32;
type SetPath = unsafe extern "system" fn(*const u8);
type GetNum = unsafe extern "system" fn(*mut u32, *mut u64, *mut u64) -> u32;
type Trigger = unsafe extern "system" fn();

static DONE: AtomicBool = AtomicBool::new(false);

fn slot(api: *const c_void, index: usize) -> *const c_void {
    // SAFETY: `api` points at the versioned API struct, whose members are all
    // function pointers; `index` is one of the constants above, well inside it.
    unsafe { *(api as *const *const c_void).add(index) }
}

/// Captures frame `frame` with RenderDoc if `VICTORY_RDOC_FRAME` asks for it.
pub(crate) fn maybe_capture(frame: u64) {
    let Ok(want) = std::env::var("VICTORY_RDOC_FRAME") else {
        return;
    };
    let Ok(want) = want.trim().parse::<u64>() else {
        return;
    };
    if frame < want || DONE.swap(true, Ordering::Relaxed) {
        return;
    }

    // SAFETY: plain Win32 lookups with NUL-terminated names that outlive the
    // calls; every returned pointer is checked before use.
    let module = unsafe {
        let name = c"renderdoc.dll";
        let mut handle = GetModuleHandleA(name.as_ptr().cast());
        if handle.is_null() {
            if let Ok(path) = std::env::var("VICTORY_RDOC_DLL") {
                let mut bytes = path.into_bytes();
                bytes.push(0);
                handle = LoadLibraryA(bytes.as_ptr());
            }
        }
        handle
    };
    if module.is_null() {
        tracing::warn!(frame, "no renderdoc.dll in this process: cannot capture");
        return;
    }

    // SAFETY: `RENDERDOC_GetAPI` is a plain exported function.
    let get_api = unsafe { GetProcAddress(module, c"RENDERDOC_GetAPI".as_ptr().cast()) };
    if get_api.is_null() {
        tracing::warn!("renderdoc.dll has no RENDERDOC_GetAPI");
        return;
    }
    let get_api: GetApi = unsafe { std::mem::transmute(get_api) };

    let mut api: *mut c_void = std::ptr::null_mut();
    // SAFETY: the signature and the version constant match the header.
    let ok = unsafe { get_api(API_VERSION_1_6_0, &mut api) };
    if ok == 0 || api.is_null() {
        tracing::warn!("renderdoc refused the 1.6 API");
        return;
    }

    if let Ok(path) = std::env::var("VICTORY_RDOC_PATH") {
        let set = slot(api, SLOT_SET_CAPTURE_PATH);
        if !set.is_null() {
            let mut bytes = path.into_bytes();
            bytes.push(0);
            // SAFETY: slot 11 is `SetCaptureFilePathTemplate(const char*)`.
            let set: SetPath = unsafe { std::mem::transmute(set) };
            unsafe { set(bytes.as_ptr()) };
        }
    }

    let before = {
        let get = slot(api, SLOT_GET_NUM_CAPTURES);
        if get.is_null() {
            0
        } else {
            // SAFETY: slot 13 is `GetNumCaptures(uint32*, uint64*, uint64*)`.
            let get: GetNum = unsafe { std::mem::transmute(get) };
            let mut n = 0u32;
            let mut last = 0u64;
            let mut last_disk = 0u64;
            unsafe { get(&mut n, &mut last, &mut last_disk) };
            n
        }
    };

    let trigger = slot(api, SLOT_TRIGGER_CAPTURE);
    if trigger.is_null() {
        tracing::warn!("renderdoc has no TriggerCapture");
        return;
    }
    // SAFETY: slot 15 is `TriggerCapture()`.
    let trigger: Trigger = unsafe { std::mem::transmute(trigger) };
    unsafe { trigger() };
    tracing::info!(
        frame,
        captures_before = before,
        "asked renderdoc to capture the next frame"
    );
}
