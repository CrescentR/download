# StreamFlow Desktop

StreamFlow Desktop is a Tauri desktop application built on top of `yt-dlp`. It gives the downloader a clean visual workflow while keeping the existing `yt_dlp` core untouched.

The app is designed as a lightweight download console:

- paste or analyze a video URL
- choose a quality preset
- start downloads through the local `yt-dlp` runtime
- watch active tasks, progress, speed, and completion state
- switch between Chinese and English UI text
- detect whether FFmpeg is installed, and guide or trigger installation when it is missing

## Settings Design

The settings panel is designed as the control center for user preferences and runtime health.

Current settings:

- Download path: choose the default output folder through the native system folder picker.
- Default quality: choose the quality preset used by new downloads.
- Language: switch between Chinese and English.
- Dependencies: inspect FFmpeg status, recheck availability, install when safe, or copy platform-specific install commands.
- Runtime source: inspect how StreamFlow is invoking `yt-dlp`.

Planned settings that fit the same structure:

- Filename template presets.
- Proxy and cookie browser defaults.
- Concurrent fragment count and speed limit defaults.
- Automatic update checks for `yt-dlp` and the desktop shell.

## Project Layout

```text
gui/
  index.html                 Vite entry
  package.json               frontend and Tauri scripts
  public/favicon.ico         browser/app favicon for preview builds
  src/
    main.js                  StreamFlow UI, i18n, task actions, FFmpeg prompts
    styles.css               StreamFlow visual system
    assets/                  bundled preview thumbnails
  src-tauri/
    Cargo.toml               Rust/Tauri package
    tauri.conf.json          app window and bundling config
    icons/icon.ico           Windows app icon
    src/lib.rs               Tauri commands for yt-dlp, FFmpeg, and task state
```

## Runtime Model

StreamFlow is a shell around `yt-dlp`, not a fork of its downloader logic.

At runtime the app chooses the downloader in this order:

1. `YTDLP_STUDIO_BIN`, if set. Use this for a packaged sidecar binary.
2. The repository source module: `python -m yt_dlp`.
3. A `yt-dlp` executable available on `PATH`.

FFmpeg is checked separately with:

```text
ffmpeg -version
```

FFmpeg is required for many common stream workflows: video/audio merging, remuxing, audio extraction, thumbnail embedding, subtitle embedding, and metadata-heavy post-processing.

## FFmpeg Installation Flow

When FFmpeg is missing, the UI shows an install banner and a detailed dependency section in Settings.

StreamFlow detects platform and package-manager availability. When automatic installation is supported, StreamFlow opens a separate system terminal and starts the installer there. The main app does not wait for the installer process, so the UI remains responsive.

- Windows:
  - `winget install --id Gyan.FFmpeg -e --accept-package-agreements --accept-source-agreements`
  - `choco install ffmpeg -y`
  - `scoop install ffmpeg`
- macOS:
  - `brew install ffmpeg`

For Linux, StreamFlow detects common package managers and shows a copyable terminal command, but does not silently run `sudo` from the GUI because password prompts and privilege escalation differ by distribution:

- Debian/Ubuntu: `sudo apt-get update && sudo apt-get install -y ffmpeg`
- Fedora: `sudo dnf install -y ffmpeg`
- Arch: `sudo pacman -S --needed ffmpeg`

If no supported package manager is found, StreamFlow shows manual installation guidance. After installation, users may need to restart the app so the updated `PATH` is visible, then click Recheck.

## Development Setup

Requirements:

- Node.js
- npm
- Rust toolchain
- Python 3.10+
- FFmpeg, recommended for full download/post-processing support

Install dependencies:

```powershell
cd gui
npm install
```

Run the desktop app in development mode:

```powershell
npm run dev
```

Run only the frontend preview:

```powershell
npm run dev:frontend
```

The browser preview uses sample rows and does not start real downloads. Real downloads and FFmpeg installation are available inside the Tauri desktop runtime.

## Build And Package

Build the frontend:

```powershell
npm run build:frontend
```

Check the Rust/Tauri backend:

```powershell
cd gui/src-tauri
cargo check
```

Build distributable packages:

```powershell
cd gui
npm run build
```

On Windows this produces:

```text
gui/src-tauri/target/release/ytdlp-studio.exe
gui/src-tauri/target/release/bundle/msi/
gui/src-tauri/target/release/bundle/nsis/
```

For macOS and Linux releases, build on those platforms or through platform-specific CI runners.

## Common Development Workflow

1. Update UI in `src/main.js` and `src/styles.css`.
2. Update Tauri commands in `src-tauri/src/lib.rs` when native behavior is needed.
3. Run `npm run build:frontend`.
4. Run `cargo check` from `gui/src-tauri`.
5. Use `npm run dev` for desktop behavior, or `npm run dev:frontend` for fast visual checks.
6. Run `npm run build` before distributing.

## Notes For Packaging yt-dlp

The current development flow uses the repository's Python module directly. For a fully self-contained app, package a `yt-dlp` executable as a Tauri sidecar and set `YTDLP_STUDIO_BIN` at launch, or adapt the Tauri config to bundle the sidecar explicitly.

FFmpeg can also be distributed as a sidecar if you want offline installers. That requires platform-specific licensing and bundle-size decisions, so the current app starts with detection plus guided installation.
