//! Interactive audio player shell.
//!
//! Wires `uniasset` (file loading / decoding) with `uniasset_audioplayer`
//! (HAL + Mixer) to play audio files from the command line.
//!
//! ## Commands
//!
//! | Command           | Description                          |
//! |-------------------|--------------------------------------|
//! | `play <file>`     | Load and play an audio file          |
//! | `pause <id>`      | Pause a stream                       |
//! | `resume <id>`     | Resume a paused stream               |
//! | `stop <id>`       | Stop and remove a stream             |
//! | `list`            | List all active streams              |
//! | `help`            | Show this help                       |
//! | `quit`            | Exit the player                      |

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use uniasset::audio::{AudioAsset, SampleFormat};
use uniasset_audioplayer::mixer::AudioStream;
use uniasset_audioplayer::player::AudioPlayer;
use uniasset_audioplayer::AudioError;

// ── AssetStream: adapts uniasset::AudioAsset → AudioStream ──────────────

/// Bridges a [`uniasset::audio::AudioAsset`] to the [`AudioStream`] trait
/// expected by the mixer.
///
/// `AudioAsset` internally uses `UnsafeCell` and is `!Sync`, so we protect
/// it behind a `Mutex`. The mutex is uncontended on the audio hot path:
/// only the audio thread calls `read()`, while `seek()` (from the control
/// thread) is a rare user action.
struct AssetStream {
    /// The loaded and prepared audio asset, behind a mutex for thread safety.
    asset: Mutex<AudioAsset>,
    /// Sample rate reported by the asset.
    sample_rate: u32,
    /// Channel count reported by the asset.
    channels: u16,
    /// Spare raw-byte buffer reused across `read()` calls to avoid per-call
    /// allocation on the audio hot path. Protected by a mutex; uncontended
    /// because only the audio thread calls `read()`.
    raw_buf: Mutex<Vec<u8>>,
    /// Set to `true` when `asset.read()` returns 0 frames (EOF).
    eof: AtomicBool,
}

impl AssetStream {
    /// Load an audio file from `path` and prepare it for playback.
    ///
    /// The asset is decoded to Float32 PCM internally so `read()` can
    /// reinterpret the raw bytes directly as `f32` samples.
    fn from_file(path: &str) -> Result<Self, String> {
        let asset = AudioAsset::default();

        // Always request Float32 — our AudioStream output is f32.
        asset
            .load_file(path, SampleFormat::Float32)
            .map_err(|e| format!("Failed to load '{}': {}", path, e))?;

        // Pre-decode to PCM for efficient reads on the audio thread.
        asset
            .prepare()
            .map_err(|e| format!("Failed to prepare '{}': {}", path, e))?;

        let sample_rate = asset
            .get_sample_rate()
            .map_err(|e| format!("Failed to query sample rate: {}", e))?;
        let channels = asset
            .get_channel_count()
            .map_err(|e| format!("Failed to query channel count: {}", e))?;

        Ok(Self {
            asset: Mutex::new(asset),
            sample_rate,
            channels,
            raw_buf: Mutex::new(Vec::new()),
            eof: AtomicBool::new(false),
        })
    }

    /// Return the total frame count from the asset.
    fn frame_count(&self) -> Result<u64, String> {
        self.asset
            .lock()
            .get_frame_count()
            .map_err(|e| format!("{}", e))
    }
}

impl AudioStream for AssetStream {
    fn read(&self, buffer: &mut [f32], frame_count: u64) -> usize {
        let channels = self.channels as usize;
        let max_frames = (buffer.len() / channels).min(frame_count as usize);

        if max_frames == 0 {
            return 0;
        }

        // Float32 = 4 bytes per sample
        let raw_bytes_needed = max_frames * channels * 4;

        // Lock the asset and scratch buffer (both uncontended on the audio
        // thread; only contended during rare user `seek` calls).
        let asset = self.asset.lock();
        let mut raw_buf = self.raw_buf.lock();
        raw_buf.resize(raw_bytes_needed, 0u8);

        let frames_read = match asset.read(&mut raw_buf, max_frames as u32) {
            Ok(n) => n as usize,
            Err(_) => {
                drop(asset);
                drop(raw_buf);
                self.eof.store(true, Ordering::Release);
                return 0;
            }
        };

        if frames_read == 0 {
            self.eof.store(true, Ordering::Release);
            return 0;
        }

        let samples_read = frames_read * channels;

        // Reinterpret Float32 bytes → f32 (safe: we requested Float32 at load time).
        let float_bytes: &[u8] = &raw_buf[..samples_read * 4];
        for (i, chunk) in float_bytes.chunks_exact(4).enumerate() {
            buffer[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        samples_read
    }

    fn seek(&self, frame: u64) -> Result<(), AudioError> {
        self.eof.store(false, Ordering::Release);
        self.asset
            .lock()
            .seek(frame as i64)
            .map_err(|e| AudioError::StreamError(e.to_string()))
    }

    fn is_eof(&self) -> bool {
        self.eof.load(Ordering::Relaxed)
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

// ── Shell state ─────────────────────────────────────────────────────────

/// Per-stream entry tracked by the shell.
struct StreamEntry {
    /// The mixer playback handle (volume, pause, resume, seek).
    handle: uniasset_audioplayer::mixer::PlayHandle,
    /// Keeps the underlying audio asset alive.
    #[allow(dead_code)]
    stream: Arc<AssetStream>,
    /// Original file path (for display).
    path: String,
}

/// Shared shell state behind a mutex.
///
/// The control thread is the primary writer; the EOF cleanup thread also
/// writes occasionally (removing finished entries).
struct ShellState {
    player: AudioPlayer,
    entries: HashMap<usize, StreamEntry>,
    next_id: usize,
}

fn main() {
    println!("══════════════════════════════════════════");
    println!("  uniasset Audio Player Shell");
    println!("══════════════════════════════════════════");

    let player = match AudioPlayer::new() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to open audio device: {}", e);
            return;
        }
    };

    let format = player.format();
    println!(
        "Device: {} Hz, {} channel(s)",
        format.sample_rate, format.channels
    );
    println!("Type 'help' for commands, 'quit' to exit.\n");

    let state = Arc::new(Mutex::new(ShellState {
        player,
        entries: HashMap::new(),
        next_id: 0,
    }));

    // ── Background EOF cleanup thread ──────────────────────────────────
    let cleanup_state = Arc::clone(&state);
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        let mut s = cleanup_state.lock();
        s.player.cleanup_eof();
        // Remove entries whose handles are no longer alive (EOF reached).
        s.entries.retain(|_id, entry| entry.handle.is_alive());
    });

    // ── REPL ────────────────────────────────────────────────────────────
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("> ");
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // Ctrl+D
            Ok(_) => {}
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (cmd, args) = match trimmed.split_once(char::is_whitespace) {
            Some((c, a)) => (c.to_lowercase(), a.trim().to_string()),
            None => (trimmed.to_lowercase(), String::new()),
        };

        match cmd.as_str() {
            "play" | "p" => cmd_play(&state, &args),
            "pause" => cmd_pause(&state, &args),
            "resume" => cmd_resume(&state, &args),
            "stop" => cmd_stop(&state, &args),
            "list" | "ls" => cmd_list(&state),
            "help" | "h" | "?" => cmd_help(),
            "quit" | "q" | "exit" => {
                println!("Bye!");
                break;
            }
            _ => {
                println!("Unknown command '{}'. Type 'help' for commands.", cmd);
            }
        }
    }
}

// ── Command handlers ────────────────────────────────────────────────────

/// `play <file>` — load an audio file and start playback.
fn cmd_play(state: &Arc<Mutex<ShellState>>, path: &str) {
    if path.is_empty() {
        println!("Usage: play <file>");
        return;
    }

    let asset_stream = match AssetStream::from_file(path) {
        Ok(s) => s,
        Err(e) => {
            println!("Error: {}", e);
            return;
        }
    };

    let sample_rate = asset_stream.sample_rate();
    let channels = asset_stream.channels();
    let frame_count = asset_stream.frame_count().unwrap_or(0);
    let duration = if sample_rate > 0 {
        frame_count as f64 / sample_rate as f64
    } else {
        0.0
    };

    // Keep a concrete Arc<AssetStream> for the entry, and clone it
    // into a type-erased Arc<dyn AudioStream> for the mixer.
    let stream_arc = Arc::new(asset_stream);
    let trait_obj: Arc<dyn AudioStream> = stream_arc.clone();

    let mut s = state.lock();
    let id = s.next_id;
    s.next_id += 1;

    let handle = s.player.add_stream(trait_obj);

    s.entries.insert(
        id,
        StreamEntry {
            handle,
            stream: stream_arc,
            path: path.to_string(),
        },
    );

    println!(
        "[id={}] Playing '{}': {} Hz, {} ch, {} frames ({:.1}s)",
        id, path, sample_rate, channels, frame_count, duration
    );
}

/// `pause <id>` — pause a stream.
fn cmd_pause(state: &Arc<Mutex<ShellState>>, arg: &str) {
    let id = match parse_id(arg) {
        Some(id) => id,
        None => {
            println!("Usage: pause <id>");
            return;
        }
    };

    let s = state.lock();
    match s.entries.get(&id) {
        Some(entry) => {
            entry.handle.pause();
            println!("[id={}] Paused '{}'", id, entry.path);
        }
        None => println!("No stream with id {}", id),
    }
}

/// `resume <id>` — resume a paused stream.
fn cmd_resume(state: &Arc<Mutex<ShellState>>, arg: &str) {
    let id = match parse_id(arg) {
        Some(id) => id,
        None => {
            println!("Usage: resume <id>");
            return;
        }
    };

    let s = state.lock();
    match s.entries.get(&id) {
        Some(entry) => {
            entry.handle.resume();
            println!("[id={}] Resumed '{}'", id, entry.path);
        }
        None => println!("No stream with id {}", id),
    }
}

/// `stop <id>` — stop and remove a stream.
fn cmd_stop(state: &Arc<Mutex<ShellState>>, arg: &str) {
    let id = match parse_id(arg) {
        Some(id) => id,
        None => {
            println!("Usage: stop <id>");
            return;
        }
    };

    let mut s = state.lock();
    match s.entries.remove(&id) {
        Some(entry) => {
            // Mark the stream as EOF so cleanup_eof removes it from the mixer.
            entry.handle.stop();
            println!("[id={}] Stopped '{}'", id, entry.path);
            // Rebuild the mixer snapshot to actually remove the stream.
            s.player.cleanup_eof();
        }
        None => println!("No stream with id {}", id),
    }
}

/// `list` — show all active streams.
fn cmd_list(state: &Arc<Mutex<ShellState>>) {
    let s = state.lock();

    if s.entries.is_empty() {
        println!("No active streams.");
        return;
    }

    println!(
        "{:>4}  {:8}  {:>6}  {:>3}  {}",
        "ID", "State", "Rate", "Ch", "File"
    );
    println!("{}", "-".repeat(60));

    // Sort by ID for consistent output.
    let mut ids: Vec<usize> = s.entries.keys().copied().collect();
    ids.sort();

    for id in ids {
        let entry = &s.entries[&id];
        let state_str = if entry.handle.is_paused() {
            "PAUSED"
        } else {
            "playing"
        };
        let rate = entry.stream.sample_rate();
        let ch = entry.stream.channels();

        println!(
            "{:>4}  {:8}  {:>6}  {:>3}  {}",
            id, state_str, rate, ch, entry.path
        );
    }
}

/// `help` — show available commands.
fn cmd_help() {
    println!("Commands:");
    println!("  play <file>    Load and play an audio file");
    println!("  pause <id>     Pause a stream");
    println!("  resume <id>    Resume a paused stream");
    println!("  stop <id>      Stop and remove a stream");
    println!("  list           List all active streams");
    println!("  help           Show this help");
    println!("  quit           Exit the player");
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Parse a stream ID from a string argument.
fn parse_id(arg: &str) -> Option<usize> {
    arg.trim().parse::<usize>().ok()
}
