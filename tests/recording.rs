// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! Integration tests for the recording subsystem.
//!
//! Validates end-to-end recording via the SPSC ring buffer → `disk_writer_loop`
//! → valid WAV file on disk. Requires `io_uring` support (Linux >= 5.1).

use nam_audio_pipe::recording::buffer::{
    AlignedBlock, AudioMetadata, MAX_BLOCK_SIZE, OVERRUN_COUNT, RING_CAPACITY, RingPayload,
    create_audio_ring_buffer,
};
use neural_amp_modeler_rs::common::spsc::SHUTDOWN;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn io_uring_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(v) = std::fs::read_to_string("/proc/sys/kernel/io_uring_disabled")
            && v.trim() == "2"
        {
            return false;
        }
        let params: [u8; 128] = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::syscall(libc::SYS_io_uring_setup, 2, params.as_ptr()) };
        if ret >= 0 {
            unsafe { libc::close(ret as _) };
            return true;
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nam-recording-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

fn wav_sample_count(path: &std::path::Path) -> u32 {
    let reader = hound::WavReader::open(path).expect("failed to open WAV for reading");
    let spec = reader.spec();
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.sample_rate, 48000);
    assert_eq!(spec.bits_per_sample, 32);
    assert_eq!(spec.sample_format, hound::SampleFormat::Float);
    reader.duration()
}

/// RAII guard that resets SHUTDOWN after each test.
struct ShutdownGuard;

impl ShutdownGuard {
    fn new() -> Self {
        SHUTDOWN.store(false, Ordering::SeqCst);
        Self
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        SHUTDOWN.store(false, Ordering::SeqCst);
    }
}

/// RAII guard that cleans up the temp directory on drop.
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// RAII guard that changes CWD for the duration of a test.
struct CwdGuard(PathBuf);

impl CwdGuard {
    fn enter(dir: &PathBuf) -> Self {
        let prev = std::env::current_dir().expect("failed to read current dir");
        std::env::set_current_dir(dir).expect("failed to chdir to temp dir");
        Self(prev)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[test]
fn disk_writer_loop_creates_valid_wav() {
    let _lock = TEST_MUTEX.lock().unwrap();
    if !io_uring_available() {
        eprintln!("SKIP: io_uring unavailable");
        return;
    }
    let _sd = ShutdownGuard::new();
    let dir = temp_dir();
    let _cwd = CwdGuard::enter(&dir);
    let _guard = DirGuard(dir.clone());

    let (mut producer, consumer) = create_audio_ring_buffer::<{ MAX_BLOCK_SIZE }>(RING_CAPACITY);

    let handle = std::thread::Builder::new()
        .name("nam-test-recording-io".into())
        .spawn(move || {
            tokio_uring::start(async {
                nam_audio_pipe::recording::disk_writer_loop(consumer)
                    .await
                    .expect("disk_writer_loop should succeed");
            });
        })
        .expect("failed to spawn test I/O thread");

    let meta = AudioMetadata {
        sample_rate: 48000.0,
        bit_depth: 32,
        channels: 2,
    };
    producer
        .push(RingPayload::Metadata(meta))
        .expect("metadata push should succeed");

    const BLOCK_SAMPLES: usize = 480;
    let interleaved_len = BLOCK_SAMPLES * 2;
    for block_idx in 0..10u32 {
        let mut block = AlignedBlock::<MAX_BLOCK_SIZE>::new();
        for i in 0..BLOCK_SAMPLES {
            let v = (block_idx * BLOCK_SAMPLES as u32 + i as u32) as f32 * 0.001;
            block.data[i * 2] = v;
            block.data[i * 2 + 1] = -v;
        }
        block.valid_len = interleaved_len;
        producer
            .push(RingPayload::Audio(block))
            .expect("audio push should succeed");
    }

    producer
        .push(RingPayload::StreamStop)
        .expect("StreamStop push should succeed");

    // Signal shutdown so disk_writer_loop exits after draining.
    // StreamStop finalizes the WAV; SHUTDOWN triggers the loop to exit.
    SHUTDOWN.store(true, Ordering::SeqCst);

    handle.join().expect("I/O thread should complete");

    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&dir).expect("failed to read temp dir") {
        let e = entry.expect("dir entry error");
        let p = e.path();
        if p.extension().is_some_and(|ext| ext == "wav") {
            found = Some(p);
            break;
        }
    }

    let wav_path = found.expect("no WAV file created by disk_writer_loop");
    let sample_count = wav_sample_count(&wav_path);
    assert_eq!(
        sample_count, 4800,
        "expected 4800 samples per channel (10 × 480), got {sample_count}"
    );

    let overruns = OVERRUN_COUNT.load(Ordering::Relaxed);
    assert_eq!(overruns, 0, "unexpected ring buffer overruns: {overruns}");
}

#[test]
fn disk_writer_loop_metadata_then_stream_stop_creates_empty_wav() {
    let _lock = TEST_MUTEX.lock().unwrap();
    if !io_uring_available() {
        eprintln!("SKIP: io_uring unavailable");
        return;
    }
    let _sd = ShutdownGuard::new();
    let dir = temp_dir();
    let _cwd = CwdGuard::enter(&dir);
    let _guard = DirGuard(dir.clone());

    let (mut producer, consumer) = create_audio_ring_buffer::<{ MAX_BLOCK_SIZE }>(RING_CAPACITY);

    let handle = std::thread::Builder::new()
        .name("nam-test-recording-io".into())
        .spawn(move || {
            tokio_uring::start(async {
                nam_audio_pipe::recording::disk_writer_loop(consumer)
                    .await
                    .expect("disk_writer_loop should succeed");
            });
        })
        .expect("failed to spawn test I/O thread");

    let meta = AudioMetadata {
        sample_rate: 44100.0,
        bit_depth: 32,
        channels: 2,
    };
    producer
        .push(RingPayload::Metadata(meta))
        .expect("metadata push should succeed");

    producer
        .push(RingPayload::StreamStop)
        .expect("StreamStop push should succeed");

    SHUTDOWN.store(true, Ordering::SeqCst);

    handle.join().expect("I/O thread should complete");

    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&dir).expect("failed to read temp dir") {
        let e = entry.expect("dir entry error");
        let p = e.path();
        if p.extension().is_some_and(|ext| ext == "wav") {
            found = Some(p);
            break;
        }
    }

    let wav_path = found.expect("no WAV file created");
    let reader = hound::WavReader::open(&wav_path).expect("failed to open WAV");
    assert_eq!(reader.spec().sample_rate, 44100);
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.duration(), 0);
}

#[test]
fn disk_writer_loop_discards_audio_before_metadata() {
    let _lock = TEST_MUTEX.lock().unwrap();
    if !io_uring_available() {
        eprintln!("SKIP: io_uring unavailable");
        return;
    }
    let _sd = ShutdownGuard::new();
    let dir = temp_dir();
    let _cwd = CwdGuard::enter(&dir);
    let _guard = DirGuard(dir.clone());

    let (mut producer, consumer) = create_audio_ring_buffer::<{ MAX_BLOCK_SIZE }>(RING_CAPACITY);

    let handle = std::thread::Builder::new()
        .name("nam-test-recording-io".into())
        .spawn(move || {
            tokio_uring::start(async {
                nam_audio_pipe::recording::disk_writer_loop(consumer)
                    .await
                    .expect("disk_writer_loop should succeed");
            });
        })
        .expect("failed to spawn test I/O thread");

    // Push Audio BEFORE Metadata — should be discarded silently
    let mut block = AlignedBlock::<MAX_BLOCK_SIZE>::new();
    for i in 0..64 {
        block.data[i * 2] = 1.0;
        block.data[i * 2 + 1] = -1.0;
    }
    block.valid_len = 128;
    producer
        .push(RingPayload::Audio(block))
        .expect("audio push should succeed");

    let meta = AudioMetadata {
        sample_rate: 48000.0,
        bit_depth: 32,
        channels: 2,
    };
    producer
        .push(RingPayload::Metadata(meta))
        .expect("metadata push should succeed");

    let mut block2 = AlignedBlock::<MAX_BLOCK_SIZE>::new();
    block2.valid_len = 4;
    block2.data[0] = 0.5;
    block2.data[1] = -0.5;
    block2.data[2] = 0.6;
    block2.data[3] = -0.6;
    producer
        .push(RingPayload::Audio(block2))
        .expect("audio push should succeed");

    producer
        .push(RingPayload::StreamStop)
        .expect("StreamStop push should succeed");

    SHUTDOWN.store(true, Ordering::SeqCst);

    handle.join().expect("I/O thread should complete");

    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&dir).expect("failed to read temp dir") {
        let e = entry.expect("dir entry error");
        let p = e.path();
        if p.extension().is_some_and(|ext| ext == "wav") {
            found = Some(p);
            break;
        }
    }

    let wav_path = found.expect("no WAV file created");
    // Only 4 floats: 2 samples × 2 channels
    assert_eq!(wav_sample_count(&wav_path), 2);
}
