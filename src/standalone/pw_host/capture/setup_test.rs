// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

use super::*;

#[test]
fn test_create_capture_properties_with_latency() {
    pw::init();
    let props = create_capture_properties(256);
    assert_eq!(props.get("node.latency"), Some("256/48000"));
    assert_eq!(props.get(&pw::keys::MEDIA_TYPE), Some("Audio"));
    assert_eq!(props.get(&pw::keys::MEDIA_CATEGORY), Some("Duplex"));
    assert_eq!(props.get(&pw::keys::MEDIA_ROLE), Some("DSP"));
    assert_eq!(props.get(&pw::keys::MEDIA_CLASS), Some("Audio/Sink"));
}

#[test]
fn test_create_capture_properties_zero_buffer_size() {
    pw::init();
    let props = create_capture_properties(0);
    assert_eq!(props.get("node.latency"), None);
}

#[test]
fn test_build_capture_format_pod() {
    pw::init();
    let mut format_buf = [0u8; 1024];
    let res = build_capture_format_pod(&mut format_buf);
    assert!(res.is_ok());
}
