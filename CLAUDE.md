# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Sacho™ — The Songwriter's Notebook™. A cross-platform desktop app (Windows/Mac/Linux) that automatically captures MIDI, audio, and video recordings triggered by MIDI input with configurable idle timeout, so musicians never lose spontaneous ideas.

## Tech Stack

- **Frontend:** SvelteKit 5 (SPA mode) + TypeScript, built with Vite
- **Backend:** Tauri 2 with Rust
- **Audio:** cpal (cross-platform: WASAPI/CoreAudio/ALSA)
- **MIDI:** midir (input), midly (file I/O)
- **Video/Encoding:** GStreamer 0.24 (LGPL plugins only — commercial app; open encoders only: AV1/VP9/VP8, MJPEG passthrough)
- **Database:** SQLite via rusqlite (bundled)
- **Concurrency:** tokio async runtime, parking_lot locks, crossbeam channels, ringbuf for pre-roll

## Commands

```bash
npm install                    # Install Node + triggers Rust dependency resolution
npm run tauri dev              # Run full app in development (frontend HMR + backend rebuild)
npm run tauri build            # Production build (NSIS installer on Windows)
npm run check                  # TypeScript + Svelte validation
npm run check:watch            # Watch mode for type checking
```

Frontend dev server runs on `http://localhost:1420`. GStreamer must be installed on the system for development.

## Architecture

### Frontend-Backend IPC

Tauri `invoke()` for request-response. Backend emits events (`recording-started`, `recording-stopped`, `recording-state-changed`, `auto-select-progress`) that frontend stores listen to. All IPC types are defined in `src/lib/api.ts` mirroring Rust serde structs.

### Frontend State

Svelte writable stores in `src/lib/stores/` — `recording.ts`, `devices.ts`, `sessions.ts`, `settings.ts`, `similarity.ts`. Stores auto-sync with backend via event listeners and persist device selections through `updateConfig()`.

### Backend State (Tauri managed state)

`RwLock<Config>`, `RwLock<RecordingState>`, `RwLock<DeviceManager>`, `SessionDatabase`, `Arc<Mutex<MidiMonitor>>`. All in `src-tauri/src/lib.rs` setup.

### Recording Pipeline

1. **MidiMonitor** (`recording/monitor.rs`) — listens for MIDI on trigger devices, auto-starts recording
2. **Pre-roll** (`recording/preroll.rs`) — ring buffers capture audio/MIDI before trigger
3. **Audio streaming** — cpal + GStreamer pipeline
4. **Video capture** (`recording/video.rs`) — GStreamer, with async encoding via `encoding/encoder.rs`
5. **Idle timeout** — auto-stops after configurable silence duration
6. **Session save** — files to `~/Music/Sacho/` with metadata, indexed in SQLite

### Video Encoding

Async encoder on separate thread pool. Raw sources re-encoded (AV1/VP9/VP8), MJPEG passed through. 5-tier quality presets. Optional encode-during-preroll trades CPU for longer pre-roll buffer.

### Key Modules

| Path | Purpose |
|------|---------|
| `src-tauri/src/commands.rs` | All Tauri IPC command handlers |
| `src-tauri/src/config.rs` | TOML config load/save |
| `src-tauri/src/devices/enumeration.rs` | Audio, MIDI, video device discovery |
| `src-tauri/src/recording/` | Core recording engine |
| `src-tauri/src/session/` | Session storage, metadata, SQLite DB |
| `src-tauri/src/similarity/` | MIDI feature extraction, PCA, k-means clustering |
| `src-tauri/src/encoding/` | GStreamer video encoder + quality presets |
| `src/lib/api.ts` | Frontend type definitions + IPC bindings |
| `src/lib/stores/` | Svelte reactive state management |

## Conventions

- Rust errors use `anyhow::Result<T>` for propagation
- Tauri commands are snake_case in Rust, invoked as strings from TS (`invoke('get_audio_devices')`)
- Frontend types in `api.ts` must stay in sync with Rust serde structs in `commands.rs`
- Config stored at platform-appropriate path (e.g., `%APPDATA%\com.sacho.app\config.toml` on Windows)
- No test suite currently — validation via `npm run check` and manual testing with `npm run tauri dev`
