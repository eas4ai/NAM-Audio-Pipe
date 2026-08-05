// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! PipeWire host execution — dual-stream topology setup, DSP bridge allocation,
//! CPU affinity locking, main control loop, and graceful shutdown.

use super::handlers;
use super::output_pw::AppState;
use crate::recording::buffer::{MAX_BLOCK_SIZE, RingPayload};
use crate::standalone::rt_setup;
use neural_amp_modeler_rs::common::spsc::{
    GcItem, GcOverflowBuffer, ParamPayload, RtStatusFlags, SHUTDOWN,
};
use neural_amp_modeler_rs::dsp::resampler::NamResampler;
use neural_amp_modeler_rs::models::StaticModel;

use rtrb::Consumer;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::PipewireHostConfig;
use super::bridge;
use super::capture;
use super::identity;
use super::playback;

/// Initializes the PipeWire dual-stream topology (Capture + Playback).
///
/// Architecture: Apps → [Capture: Audio/Sink] → process(DSP) → DspBridge → [Playback: Stream/Output] → Hardware.
/// The monitor port of `Audio/Sink` copies the buffer *before* `process()` — therefore, the only
/// way to deliver the processed audio to hardware is via a second playback stream
/// that reads from `DspBridge` post-DSP.
///
/// ## SPSC channel parameters
///
/// - `consumer`: Consumer of the CLI→DSP parameter channel (gain, model, etc.).
/// - `gc_producer`: Producer of the GC channel for drop-delegation of obsolete models.
/// - `resampler_consumer`: Dedicated channel for receiving pre-built resamplers
///   from the main thread — **zero allocations in the RT callback**.
/// - `resampler_producer`: Producer of the resampler channel — the main thread
///   builds `NamResampler::new().expect("construction should succeed for test-sized buffers")` here (allocation outside RT) and sends to the callback.
/// - `rt_status`: Atomic flags for silent RT→Main communication.
#[expect(
    clippy::too_many_arguments,
    reason = "FFI design or complex DSP kernel signature required by construction"
)]
pub fn run_pipewire_host(
    consumer: Consumer<ParamPayload>,
    gc_producer: rtrb::Producer<GcItem>,
    gc_overflow: Arc<GcOverflowBuffer>,
    resampler_consumer: Consumer<Box<NamResampler>>,
    mut resampler_producer: rtrb::Producer<Box<NamResampler>>,
    cabsim_consumer: Consumer<Option<neural_amp_modeler_rs::dsp::cabsim::adapter::CabSimAdapter>>,
    mut cabsim_producer: rtrb::Producer<
        Option<neural_amp_modeler_rs::dsp::cabsim::adapter::CabSimAdapter>,
    >,
    rt_status: Arc<RtStatusFlags>,
    config: PipewireHostConfig,
    mut gc_consumer: Consumer<GcItem>,
    slimmable_consumer: Consumer<Option<Box<StaticModel>>>,
    os_consumer: Consumer<Box<neural_amp_modeler_rs::dsp::oversample::OsEnginePair>>,
    recording_producer: Option<rtrb::Producer<RingPayload<MAX_BLOCK_SIZE>>>,
) -> anyhow::Result<()> {
    let PipewireHostConfig {
        buffer_size,
        sys,
        ir_raw_samples,
        full_wavenet_model,
        mut slimmable_producer,
        mut os_producer,
        oversample,
    } = config;

    let full_wavenet_model = full_wavenet_model;

    // =========================================================
    // 1. PIPEWIRE LOOP INITIALIZATION
    // =========================================================
    let thread_loop = unsafe {
        pipewire::thread_loop::ThreadLoopBox::new(Some(identity::PW_THREAD_LOOP_NAME), None)
    }?;
    let context = pipewire::context::ContextBox::new(thread_loop.loop_(), None)?;
    let core = context.connect(None)?;

    // =========================================================
    // 2. DSP BRIDGE ALLOCATION (Lock-Free Communication)
    // =========================================================
    let bridge_ptr = bridge::allocate_dsp_bridge();

    // Place the recording producer on the stack so both the RT closure
    // (via a raw pointer) and the shutdown path can access it without
    // locking. Producer is not Clone; a raw pointer avoids shared-ownership
    // plumbing while respecting the SPSC contract (single writer at a time).
    let mut recording_producer_slot = recording_producer;
    let rec_ptr: *mut Option<rtrb::Producer<RingPayload<MAX_BLOCK_SIZE>>> =
        &raw mut recording_producer_slot;

    // =========================================================
    // 3. CORE OPTIMIZATION (CPU Affinity)
    // =========================================================
    let target_cpu = rt_setup::select_optimal_cpu().unwrap_or(0);

    // =========================================================
    // 4. PROTECTED CONFIGURATION SCOPE (RAII)
    // =========================================================
    let (capture_stream, capture_listener, playback_stream, playback_listener);
    {
        let _lock = thread_loop.lock();

        let latency_str = format!("{}/48000", buffer_size);

        let (cs, cl) = capture::setup_capture_stream(
            &core,
            bridge_ptr,
            buffer_size,
            ir_raw_samples.clone(),
            &sys,
            target_cpu,
            consumer,
            gc_producer,
            gc_overflow.clone(),
            resampler_consumer,
            cabsim_consumer,
            rt_status.clone(),
            slimmable_consumer,
            os_consumer,
            oversample,
            rec_ptr,
        )?;
        capture_stream = cs;
        capture_listener = cl;

        let (ps, pl) = playback::setup_playback_stream(
            &core,
            bridge_ptr,
            buffer_size,
            &latency_str,
            rt_status.clone(),
        )?;
        playback_stream = ps;
        playback_listener = pl;
    }

    let _app_state = AppState {
        capture_stream,
        capture_listener,
        playback_stream,
        playback_listener,
    };

    let _cpu_dma_lock = rt_setup::lock_cpu_c_states();

    sys.emit_irq_advisory(target_cpu);

    // =========================================================
    // 5. RT THREAD START (Background)
    // =========================================================
    thread_loop.start();

    // =========================================================
    // 6. MAIN CONTROL LOOP (Main Thread, Non-RT)
    // =========================================================
    let mut was_silent = false;
    let mut was_fading = false;
    while !SHUTDOWN.load(Ordering::Acquire) {
        // pairs with Release store em main.rs:90
        let active = rt_status.active_rate.load(Ordering::Relaxed);
        if active != 0 {
            neural_amp_modeler_rs::common::diagnostics::ACTIVE_SAMPLE_RATE
                .store(active, Ordering::Relaxed);
        }

        handlers::handle_resampler_rebuild(&rt_status, &sys, &mut resampler_producer);
        handlers::handle_quantum_log(&rt_status);
        handlers::handle_cabsim_rebuild(
            &rt_status,
            ir_raw_samples.as_deref(),
            &sys,
            &mut cabsim_producer,
        );
        handlers::handle_slimmable_rebuild(
            &rt_status,
            full_wavenet_model.as_deref(),
            &mut slimmable_producer,
        );
        handlers::handle_oversample_rebuild(&rt_status, &sys, &mut os_producer);

        (was_silent, was_fading) =
            rt_setup::poll_rt_status(&rt_status, &sys, was_silent, was_fading, unsafe {
                &*(bridge_ptr.as_ptr())
            });

        let drained = neural_amp_modeler_rs::common::spsc::drain_gc_channels(
            &mut gc_consumer,
            &gc_overflow,
            &rt_status,
        );
        rt_status
            .drains
            .fetch_add(drained as u32, Ordering::Relaxed);

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // =========================================================
    // 7. GRACEFUL SHUTDOWN
    // =========================================================
    if let Some(ref mut producer) = recording_producer_slot {
        let _ = producer.push(RingPayload::StreamStop);
    }

    thread_loop.stop();

    Ok(())
}
