# Sacho™ - The Songwriter's Notebook™

Sacho is a cross-platform (Windows, Mac, Linux) desktop application for songwriters and other spontaneous creatives to automatically capture MIDI, audio, and video when they play. Never lose a musical idea again.

Sacho is designed for musicians and other creators who create impromptu performances and need an application to automatically record their performances. Currently, when Sacho detects MIDI on a MIDI device configured as "Trigger" in the settings, it begins recording audio and MIDI on devices marked as "Record". When no MIDI is received after a configured amount of time, the recording is saved. Recordings can be triggered and stopped manually. There is also a pre-roll, to capture the moments before a performance begins.

Being a commercial application, when it comes to gstreamer, Sacho should only use lgpl plugins and should never use unlicensed encoders.

## Features

### Automatic Recording

- **MIDI-triggered recording** - Recording starts automatically when you play your MIDI controller
- **Configurable idle timeout** - Recording stops after a period of inactivity 
- **Multi-device support** - Record from multiple audio interfaces, MIDI controllers, and video sources simultaneously
- **Pre-roll** - Configurable pre-roll captures on your configured devices even when no recording has been triggered, allowing for past capture to be retrospectively included at the beginning of the recordings.

### Session Management

- **Organized storage** - Sessions are saved with timestamps in a configurable location
- **Session browser** - Browse, search, and filter your recordings with ease
- **Tags & notes** - Add metadata to organize and remember your ideas

### MIDI Similarity Map

- **Visualize your performances** - See all your MIDI recordings on a 2D map
- **Find similar ideas** - Similar performances are grouped together
- **Automatic clustering** - AI-powered grouping of related musical ideas

### System Integration

- **System tray** - Runs quietly in the background
- **Desktop notifications** - Know when recording starts and stops
- **Minimal footprint** - Lightweight and efficient

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Node.js](https://nodejs.org/) (v18 or later)
- [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)

### Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Project Structure (might be outdated)

```
sacho/
├── src/                          # Frontend (Svelte)
│   ├── lib/
│   │   ├── components/           # UI components
│   │   │   ├── sessions/         # Session browser
│   │   │   ├── devices/          # Device selector
│   │   │   └── similarity/       # Similarity map
│   │   ├── stores/               # Svelte stores
│   │   └── api.ts                # Tauri IPC bindings
│   └── routes/
│       └── +page.svelte          # Main page
├── src-tauri/                    # Backend (Rust)
│   ├── src/
│   │   ├── recording/            # Audio/MIDI/video capture
│   │   ├── session/              # Session storage & database
│   │   ├── similarity/           # MIDI analysis & clustering
│   │   ├── devices/              # Device enumeration
│   │   ├── config.rs             # Settings management
│   │   ├── tray.rs               # System tray
│   │   ├── notifications.rs      # Desktop notifications
│   │   └── commands.rs           # Tauri IPC commands
│   └── Cargo.toml
└── package.json
```

## Configuration

Settings are stored in your platform's config directory:

- **Windows**: `%APPDATA%\com.sacho.app\config.toml`
- **macOS**: `~/Library/Application Support/com.sacho.app/config.toml`
- **Linux**: `~/.config/com.sacho.app/config.toml`

Recordings are saved by default to:

- **Windows**: `%USERPROFILE%\Music\Sacho`
- **macOS**: `~/Music/Sacho`
- **Linux**: `~/Music/Sacho`

## Debugging

The gstreamer CLI tools are installed on the development system. Use them to diagnose video pipeline issues.

## License

All rights reserved.