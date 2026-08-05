// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! Setup and initialization helpers for off-RT resources and SPSC channels.

use crate::standalone::{cli, colors::Colorize};
use neural_amp_modeler_rs::common::spsc::{self, ParamPayload, SpscChannels};
use neural_amp_modeler_rs::diagnostics::SystemSnapshot;
use neural_amp_modeler_rs::dsp::cabsim::adapter::CabSimAdapter;
use neural_amp_modeler_rs::dsp::cabsim::conv::ConvEngine;
use neural_amp_modeler_rs::dsp::cabsim::loader::CabSimIr;
use neural_amp_modeler_rs::loader;
use neural_amp_modeler_rs::models::StaticModel;
use neural_amp_modeler_rs::models::slimmable::clone_wavenet_for_slimmable_storage;
use rtrb::Producer;
use std::path::Path;
use std::sync::atomic::Ordering;

/// Isolate the setup of SPSC lock-free communication channels between host and audio thread.
pub fn setup_communication_channels() -> SpscChannels {
    spsc::setup_spsc(spsc::SPSC_CAPACITY)
}

/// Result of loading the initial neural model.
pub struct InitialModelSetup {
    /// Full WaveNet model clone if slimmable WaveNet architecture, used for dynamic slim rebuilds.
    pub full_wavenet_model: Option<Box<StaticModel>>,
    /// Architecture name string (e.g. "WaveNet", "LSTM", "Linear").
    pub architecture: String,
}

/// Encapsulates neural model loading, active diagnostics state updates, and SPSC payload dispatch.
pub fn load_initial_model(
    model_path: Option<&Path>,
    sys: &SystemSnapshot,
    producer: &mut Producer<ParamPayload>,
) -> InitialModelSetup {
    let mut full_wavenet_model = None;
    let mut architecture = String::new();

    if let Some(path) = model_path {
        log::info!("{} Loading model...", "📂".cyan());
        match loader::load_and_build_model(path, sys, true, loader::LoadOptions::default()) {
            Ok(loaded) => {
                if let Ok(mut name) = neural_amp_modeler_rs::diagnostics::ACTIVE_MODEL_NAME.write()
                {
                    *name = path.to_string_lossy().into_owned();
                }
                neural_amp_modeler_rs::diagnostics::ACTIVE_SAMPLE_RATE
                    .store(loaded.sample_rate, Ordering::Relaxed);

                architecture = loaded.architecture.clone();

                let model_info = loaded.model_info(path);
                if let Ok(mut info_guard) =
                    neural_amp_modeler_rs::diagnostics::ACTIVE_MODEL_INFO.write()
                {
                    *info_guard = Some(model_info);
                }

                full_wavenet_model = loaded.model_l.as_ref().and_then(|m| {
                    if let StaticModel::WavenetDyn(w) = m.as_ref() {
                        clone_wavenet_for_slimmable_storage(w).ok()
                    } else {
                        None
                    }
                });

                let _ = producer.push(ParamPayload::LoadModel {
                    model_l: loaded.model_l,
                    model_r: loaded.model_r,
                    input_mult_adj: loaded.input_mult_adj,
                    output_mult_adj: loaded.output_mult_adj,
                    sample_rate: loaded.sample_rate,
                });
            }
            Err(e) => cli::exit_with_error(format!("Model load failed: {}", e)),
        }
    } else {
        log::warn!(
            "{} No model loaded — operating in True-Bypass mode (clean audio pass-through).\n  \
             Use --model <file.nam> to load a neural amplifier model.",
            "⚠️".yellow()
        );
    }

    InitialModelSetup {
        full_wavenet_model,
        architecture,
    }
}

/// Encapsulates impulse response (Cab-Sim) loading, convolution engine assembly, and SPSC dispatch.
pub fn load_initial_cabsim(
    cab_path: Option<&Path>,
    buffer_size: u32,
    cabsim_producer: &mut Producer<Option<CabSimAdapter>>,
) -> anyhow::Result<Option<Vec<f32>>> {
    let cab_path = match cab_path {
        Some(p) => p,
        None => return Ok(None),
    };

    let active_sr = neural_amp_modeler_rs::diagnostics::ACTIVE_SAMPLE_RATE.load(Ordering::Relaxed);
    let target_rate = if active_sr > 0 { active_sr } else { 48000 };
    let partition_size = if buffer_size > 0 {
        buffer_size as usize
    } else {
        256
    };

    match CabSimIr::load(cab_path, target_rate, true) {
        Ok(cabsim) => {
            let engine = ConvEngine::new(&cabsim.samples, partition_size)
                .map_err(|e| anyhow::anyhow!("Cab-sim engine init: {e}"))?;
            let adapter = CabSimAdapter::new(Box::new(engine))
                .map_err(|e| anyhow::anyhow!("Cab-sim adapter init: {e:?}"))?;
            log::info!(
                "{} Cab-sim IR loaded: {} ({} partitions, FFT={})",
                "🎛️".cyan(),
                cab_path.display(),
                adapter.num_partitions(),
                adapter.engine().fft_size(),
            );
            let raw_samples = cabsim.samples;
            let _ = cabsim_producer.push(Some(adapter));
            Ok(Some(raw_samples))
        }
        Err(e) => {
            log::warn!(
                "{} Cab-sim IR load failed: {} — continuing without cab-sim",
                "⚠️".yellow(),
                e
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_communication_channels() {
        let channels = setup_communication_channels();
        assert!(channels.param_consumer.slots() <= spsc::SPSC_CAPACITY);
    }

    #[test]
    fn test_load_initial_model_none() {
        let mut channels = setup_communication_channels();
        let sys = SystemSnapshot::capture();
        let result = load_initial_model(None, &sys, &mut channels.param_producer);
        assert!(result.full_wavenet_model.is_none());
        assert!(result.architecture.is_empty());
    }

    #[test]
    fn test_load_initial_cabsim_none() {
        let mut channels = setup_communication_channels();
        let result = load_initial_cabsim(None, 256, &mut channels.cabsim_producer).unwrap();
        assert!(result.is_none());
    }
}
