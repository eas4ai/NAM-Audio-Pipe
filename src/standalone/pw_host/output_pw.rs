// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! PipeWire output DSP pipeline and host configuration (standalone).

use neural_amp_modeler_rs::common::diagnostics::SystemSnapshot;
use neural_amp_modeler_rs::common::spsc::RtStatusFlags;
use neural_amp_modeler_rs::dsp::oversample::OversampleFactor;
use neural_amp_modeler_rs::dsp::pipeline::DspBridgeReader;
use pipewire as pw;
use std::sync::atomic::Ordering;

/// Holds essential PipeWire instances (`StreamBox` and `Listener`).
///
/// RAII-only struct: fields are never read directly — they exist solely
/// to keep streams and listeners alive via drop semantics. The compiler
/// may warn about unused fields; that is expected and safe here.
/// Removing the fields would cause premature deallocation and audio dropout.
pub struct AppState<S1, L1, S2, L2> {
    /// PipeWire capture stream (RAII anchor).
    pub capture_stream: S1,
    /// Listener bound to the capture stream (RAII anchor).
    pub capture_listener: L1,
    /// PipeWire playback stream (RAII anchor).
    pub playback_stream: S2,
    /// Listener bound to the playback stream (RAII anchor).
    pub playback_listener: L2,
}

/// Configuration for PipeWire host initialization.
pub struct PipewireHostConfig {
    /// Requested audio buffer size.
    pub buffer_size: u32,
    /// System snapshot for diagnostics.
    pub sys: SystemSnapshot,
    /// Raw IR samples for adaptive partition rebuild (None if no IR loaded).
    pub ir_raw_samples: Option<Vec<f32>>,
    /// Full WaveNet model stored for main-thread slimmable rebuild.
    pub full_wavenet_model: Option<Box<neural_amp_modeler_rs::models::StaticModel>>,
    /// Producer to send slimmable-rebuilt models to the audio thread.
    pub slimmable_producer: rtrb::Producer<Option<Box<neural_amp_modeler_rs::models::StaticModel>>>,
    /// Producer to send oversampling engines rebuilt on main thread to the audio thread.
    pub os_producer: rtrb::Producer<Box<neural_amp_modeler_rs::dsp::oversample::OsEnginePair>>,
    /// Initial oversampling factor for the neural stage.
    pub oversample: OversampleFactor,
}

/// Playback DSP Pipeline (Bridge → Hardware).
#[inline(always)]
pub fn playback_dsp_cycle(
    stream: &pw::stream::Stream,
    bridge: DspBridgeReader,
    last_bridge_gen: &mut u64,
    rt_status: &RtStatusFlags,
) {
    bridge.read_block(last_bridge_gen, |buf_l, buf_r| {
        let n_samples = buf_l.len();
        if n_samples == 0 {
            return;
        }

        let mut buf = match stream.dequeue_buffer() {
            Some(b) => b,
            None => {
                rt_status.output_buffer_miss.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        let datas = buf.datas_mut();
        if datas.len() < 2 {
            return;
        }

        // Splits the Left and Right channels for final delivery.
        let (datas_left, datas_right) = datas.split_at_mut(1);
        let data_l = &mut datas_left[0];
        let data_r = &mut datas_right[0];

        let max_l = data_l.as_raw().maxsize as usize / std::mem::size_of::<f32>();
        let max_r = data_r.as_raw().maxsize as usize / std::mem::size_of::<f32>();
        let n_out = n_samples.min(max_l).min(max_r);
        if n_out == 0 {
            return;
        }

        // Copies the processed sound directly to your sound card outputs.
        if let Some(raw_l) = data_l.data() {
            let out_l =
                unsafe { std::slice::from_raw_parts_mut(raw_l.as_mut_ptr().cast::<f32>(), n_out) };
            unsafe {
                core::ptr::copy_nonoverlapping(buf_l.as_ptr(), out_l.as_mut_ptr(), n_out);
            }
        }
        if let Some(raw_r) = data_r.data() {
            let out_r =
                unsafe { std::slice::from_raw_parts_mut(raw_r.as_mut_ptr().cast::<f32>(), n_out) };
            unsafe {
                core::ptr::copy_nonoverlapping(buf_r.as_ptr(), out_r.as_mut_ptr(), n_out);
            }
        }

        // Informs the hardware exactly how much sound was delivered this time.
        {
            let chunk = data_l.chunk_mut();
            *chunk.size_mut() = (n_out * std::mem::size_of::<f32>()) as u32;
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = std::mem::size_of::<f32>() as i32;
        }
        {
            let chunk = data_r.chunk_mut();
            *chunk.size_mut() = (n_out * std::mem::size_of::<f32>()) as u32;
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = std::mem::size_of::<f32>() as i32;
        }
    });
}

/// Builds an F32P stereo audio format SPA Pod for PipeWire negotiation.
///
/// # Safety
/// The returned binary pod points directly to the provided `format_buf`.
pub unsafe fn build_spa_format_pod<'a>(
    audio_info: &pw::spa::param::audio::AudioInfoRaw,
    format_buf: &'a mut [u8; 1024],
) -> anyhow::Result<&'a pw::spa::pod::Pod> {
    unsafe {
        let mut builder: pw::spa::sys::spa_pod_builder = std::mem::zeroed();
        // Prepares a "builder" to create the audio format contract.
        pw::spa::sys::spa_pod_builder_init(
            &mut builder,
            format_buf.as_mut_ptr().cast(),
            format_buf.len() as u32,
        );

        // Builds the binary document (SPA Pod) describing the audio (e.g.: 48kHz, Stereo).
        // This document is what PipeWire uses to understand how to send sound to us.
        let pod_ptr = pw::spa::sys::spa_format_audio_raw_build(
            &mut builder,
            pw::spa::param::ParamType::EnumFormat.as_raw(),
            &audio_info.as_raw(),
        );

        if pod_ptr.is_null() {
            // If failure, the system won't know how to negotiate sound with your card.
            return Err(anyhow::anyhow!(
                "Failed to build the audio negotiation document (SPA Pod)"
            ));
        }

        // Returns the contract ready to be signed and used by the system.
        Ok(&*(pod_ptr as *const pw::spa::pod::Pod))
    }
}
