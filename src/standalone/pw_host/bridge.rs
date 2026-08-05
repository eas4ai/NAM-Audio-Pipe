// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! Allocation of the `DspBridge` shared buffer for lock-free communication
//! between capture and playback streams.

use neural_amp_modeler_rs::dsp::pipeline::{BridgeBuffer, BridgeRef, DspBridge, MAX_BRIDGE_BUF};

/// Allocates `DspBridge` with double-buffering via `Box::leak` (`'static` lifetime).
///
/// `Box::leak` is acceptable here because the DspBridge lifetime equals the
/// process lifetime in the standalone PipeWire binary.
///
/// The buffer is aligned to 128 bytes (`repr(align(128))`) to avoid false-sharing
/// between the two RT callbacks.
///
/// Applies `madvise(MADV_DONTFORK)` and `madvise(MADV_DONTDUMP)` separately to
/// avoid Copy-on-Write overhead on forks and to exclude the buffers from core dumps.
pub fn allocate_dsp_bridge() -> BridgeRef {
    let bridge: &'static DspBridge = Box::leak(Box::new(DspBridge {
        buffers: [
            BridgeBuffer {
                buf_l: [0.0f32; MAX_BRIDGE_BUF],
                buf_r: [0.0f32; MAX_BRIDGE_BUF],
                n_samples: 0,
            },
            BridgeBuffer {
                buf_l: [0.0f32; MAX_BRIDGE_BUF],
                buf_r: [0.0f32; MAX_BRIDGE_BUF],
                n_samples: 0,
            },
        ],
        active_read_idx: std::sync::atomic::AtomicUsize::new(0),
        generation: std::sync::atomic::AtomicU64::new(0),
        consumed_gen: std::sync::atomic::AtomicU64::new(0),
        dropped_frames: std::sync::atomic::AtomicU32::new(0),
    }));
    let bridge_ptr = unsafe { BridgeRef::new(bridge as *const DspBridge as *mut DspBridge) };

    // SAFETY: `bridge` points to a valid `DspBridge` allocation produced
    // by `Box::leak` immediately above — the pointer and size are correct.
    // `madvise` advice are NOT bitmask-combinable; each call is independent.
    let bridge_void = bridge as *const DspBridge as *mut libc::c_void;
    let bridge_size = std::mem::size_of::<DspBridge>();

    let ret = unsafe { libc::madvise(bridge_void, bridge_size, libc::MADV_DONTFORK) };
    if ret != 0 {
        log::warn!(
            "madvise(MADV_DONTFORK) returned {} (errno: {}). \
             Buffer may be included in forks.",
            ret,
            std::io::Error::last_os_error()
        );
    }

    let ret = unsafe { libc::madvise(bridge_void, bridge_size, libc::MADV_DONTDUMP) };
    if ret != 0 {
        log::warn!(
            "madvise(MADV_DONTDUMP) returned {} (errno: {}). \
             Buffer may be included in core-dumps.",
            ret,
            std::io::Error::last_os_error()
        );
    }

    bridge_ptr
}
