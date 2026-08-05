// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! PM QoS and hardware audio detection.
//!
//! Functions to lock deep CPU C-States and detect
//! the system's default hardware sink via PipeWire.

/// Dynamically detects the system's default hardware sink via `pw-metadata`.
///
/// This function attempts to identify which physical device audio should be sent to
/// by default. It parses the output of the PipeWire `pw-metadata` utility.
///
/// Returns `Some(name)` if a valid sink that is not NAM-rs itself is found,
/// or `None` otherwise (allowing routing to be decided by WirePlumber).
pub fn detect_hardware_sink() -> Option<String> {
    // Runs the external command to read PipeWire server metadata
    let out = std::process::Command::new("pw-metadata")
        .args(["-n", "default", "0", "default.audio.sink"])
        .output()
        .ok()?;

    // Converts the raw output to string (UTF-8 lossy)
    let s = String::from_utf8_lossy(&out.stdout);

    // Manual parsing: locates the "name" key in the JSON-like output
    let start = s.find("\"name\":\"")?;
    let rest = &s[start + 8..];
    let end = rest.find('"')?;
    let name = &rest[..end];

    // We avoid the "infinite loop" routing if the detected default is NAM-rs's own input.
    if name == crate::standalone::pw_host::identity::PW_CAPTURE_NODE_NAME {
        None
    } else {
        // Returns the real hardware name (e.g. 'alsa_output.pci-0000_00_1f.3.analog-stereo')
        Some(name.to_string())
    }
}

/// Prevents the processor from entering power-saving C-States,
/// guaranteeing 0ms wake-up latency for RT audio processing.
///
/// **Warning:** This protection is **system-wide (global)** and affects all CPU cores,
/// not just the thread executing this function.
///
/// Uses the Linux kernel PM QoS interface to request zero latency.
///
/// RETURN: The `File` handle. It MUST be kept alive in the main scope.
/// If the file descriptor is closed (drop), the kernel revokes the protection.
pub fn lock_cpu_c_states() -> Option<std::fs::File> {
    match std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/cpu_dma_latency")
    {
        Ok(mut file) => {
            // Value 0 indicates zero tolerance to power transition latency.
            let zero: i32 = 0;
            if std::io::Write::write_all(&mut file, &zero.to_ne_bytes()).is_ok() {
                log::info!("⚡ PM QoS Lock: Deep CPU C-States disabled (Zero DMA Latency).");
                return Some(file);
            }
            log::warn!("PM QoS: Failed to write to /dev/cpu_dma_latency.");
            None
        }
        Err(e) => {
            // Often fails if write permission is missing or the file does not exist.
            log::warn!(
                "PM QoS: Access denied to /dev/cpu_dma_latency ({}). \
                 Consider creating a udev rule for the 'audio' group.",
                e
            );
            None
        }
    }
}
