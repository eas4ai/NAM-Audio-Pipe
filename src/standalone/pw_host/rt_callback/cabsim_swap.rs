// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! Cab-sim Convolution Engine Draining (Zero-Alloc Swap)
//! Replaces the active convolution engine without using memory allocation in the critical path.

use neural_amp_modeler_rs::common::spsc::{GcItem, GcOverflowBuffer, RtStatusFlags};
use neural_amp_modeler_rs::dsp::cabsim::adapter::CabSimAdapter;

use rtrb::Consumer;

/// Drains the cab-sim convolution adapter SPSC channel and swaps the active adapter atomically.
///
/// Follows the same cascade pattern as `drain_resamplers`:
/// GC channel → parking_lot → overflow buffer.
///
/// An `Option` is used so that `None` can be sent to clear/bypass the convolution.
#[inline(always)]
pub fn drain_cabsims(
    cabsim_consumer: &mut Consumer<Option<CabSimAdapter>>,
    active_cabsim: &mut Option<CabSimAdapter>,
    gc_producer: &mut rtrb::Producer<GcItem>,
    parking_lot: &mut [Option<GcItem>; 16],
    gc_overflow_for_process: &GcOverflowBuffer,
    rt_status_for_process: &RtStatusFlags,
) {
    while let Ok(new_adapter) = cabsim_consumer.pop() {
        let old_adapter = std::mem::replace(active_cabsim, new_adapter);

        if let Some(old) = old_adapter {
            neural_amp_modeler_rs::common::spsc::gc_cascade(
                Some(GcItem::CabConvAdapter(Box::new(old))),
                gc_producer,
                parking_lot,
                gc_overflow_for_process,
                rt_status_for_process,
            );
        }
    }
}
