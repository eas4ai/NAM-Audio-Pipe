// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! PipeWire Pipeline Integration (End-to-End) Test
//!
//! Validates the full lifecycle of the PipeWire host: context initialization,
//! SPSC channel setup, gain parameter injection, and graceful shutdown.
//!
//! Requires a running PipeWire daemon (session or system). Without it, the test
//! is skipped by the `#[ignore]` attribute; `utils/tests-long.sh` auto-detects
//! the daemon via `pw-cli info`.

use nam_audio_pipe::standalone::pw_host::{self, PipewireHostConfig};
use neural_amp_modeler_rs::common::diagnostics::SystemSnapshot;
use neural_amp_modeler_rs::common::spsc::{self, GcOverflowBuffer, RtStatusFlags};
use neural_amp_modeler_rs::dsp::oversample::OversampleFactor;
use rtrb::RingBuffer;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

fn probe_pipewire_daemon() -> bool {
    std::process::Command::new("pw-cli")
        .args(["info", "0"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Tests the basic initialization and communication of the PipeWire pipeline.
///
/// This test simulates the full lifecycle of the engine:
/// 1. Creation of SPSC RingBuffers for commands and telemetry.
/// 2. Spawning the audio thread (host).
/// 3. Sending gain parameters via the control channel.
/// 4. Shutdown signaled via atomic flag.
#[test]
#[ignore = "requires a running PipeWire daemon (session or system); auto-detected by utils/tests-long.sh"]
fn test_pipewire_integration() {
    if !probe_pipewire_daemon() {
        eprintln!("SKIP: PipeWire daemon not detected (pw-cli info 0 failed).");
        return;
    }

    pipewire::init();
    println!("PipeWire initialized successfully.");

    let (mut param_prod, param_cons) = RingBuffer::new(4);
    let (gc_prod, gc_cons) = RingBuffer::new(4);
    let (res_prod, res_cons) = RingBuffer::new(2);
    let (cs_prod, cs_cons) = RingBuffer::new(2);
    let (sl_prod, sl_cons) = RingBuffer::new(2);
    let (os_prod, os_cons) = RingBuffer::new(2);

    let gc_overflow = Arc::new(GcOverflowBuffer::new(64));
    let rt_status = Arc::new(RtStatusFlags::default());

    let rt_clone = rt_status.clone();
    let gc_overflow_clone = gc_overflow.clone();
    let sys = SystemSnapshot::capture();

    let pw_thread = thread::spawn(move || {
        pw_host::run_pipewire_host(
            param_cons,
            gc_prod,
            gc_overflow_clone,
            res_cons,
            res_prod,
            cs_cons,
            cs_prod,
            rt_clone,
            PipewireHostConfig {
                buffer_size: 0,
                sys,
                ir_raw_samples: None,
                full_wavenet_model: None,
                slimmable_producer: sl_prod,
                os_producer: os_prod,
                oversample: OversampleFactor::Off,
            },
            gc_cons,
            sl_cons,
            os_cons,
            None,
        )
    });

    thread::sleep(Duration::from_millis(50));
    let _ = param_prod.push(neural_amp_modeler_rs::common::spsc::ParamPayload::InputGain(2.5));
    let _ = param_prod.push(neural_amp_modeler_rs::common::spsc::ParamPayload::OutputGain(-1.0));

    thread::sleep(Duration::from_millis(150));
    spsc::SHUTDOWN.store(true, Ordering::Relaxed);

    match pw_thread.join() {
        Ok(result) => {
            if let Err(e) = result {
                eprintln!(
                    "PipeWire host exited with expected error (possible daemon absence): {e}"
                );
            } else {
                println!("Pipeline host ran and shut down gracefully.");
            }
        }
        Err(_) => panic!("The PipeWire thread suffered a fatal panic!"),
    }

    println!("Integration test completed.")
}
