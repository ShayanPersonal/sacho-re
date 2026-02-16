# Minimal FFmpeg Build (FFV1-only)

Builds FFmpeg from source with **only the FFV1 codec enabled**, producing drop-in replacement DLLs for the full FFmpeg libraries bundled by GStreamer.

This eliminates all patent-encumbered codecs (H.264, H.265, AAC, etc.) and reduces `avcodec-61.dll` from ~15 MB to under 500 KB.

## Why

The app uses FFV1 through GStreamer's `avenc_ffv1` element (provided by `gstlibav.dll`). All other codecs (VP8/VP9/AV1/MJPEG) use standalone GStreamer plugins that don't touch FFmpeg. Building FFmpeg with `--disable-everything --enable-encoder=ffv1 --enable-decoder=ffv1` strips out everything except FFV1.

## Prerequisites

- **Windows 10/11 x64**
- **Visual Studio 2019 or 2022** with "Desktop development with C++" workload
- **MSYS2** ([msys2.org](https://www.msys2.org/)) with these packages:
  ```
  pacman -S make nasm diffutils
  ```
- **Optional:** `pacman -S gnupg` for PGP signature verification of the FFmpeg tarball

## Build Steps

1. Open **x64 Native Tools Command Prompt for VS 2022** (or 2019)

2. From that prompt, launch the plain MSYS2 shell with PATH inheritance so MSVC tools are visible.
   Per [FFmpeg's official docs](https://www.ffmpeg.org/platform.html), MSVC builds should use the base MSYS2 shell (`-msys2`), not MinGW/UCRT64 variants which prepend GCC toolchain paths that can interfere:
   ```
   set MSYS2_PATH_TYPE=inherit
   C:\msys64\msys2_shell.cmd -defterm -here -no-start -msys2
   ```

3. In the MSYS2 shell, navigate to this directory:
   ```bash
   cd /c/path/to/sacho-re/ffmpeg-build
   ```

4. Run the build:
   ```bash
   ./build.sh
   ```

5. The script prints copy commands at the end. Run them to replace the bundled DLLs:
   ```bash
   cp dist/bin/avcodec-61.dll ../src-tauri/installers/dlls/
   cp dist/bin/avformat-61.dll ../src-tauri/installers/dlls/
   cp dist/bin/avutil-59.dll ../src-tauri/installers/dlls/
   cp dist/bin/avfilter-10.dll ../src-tauri/installers/dlls/
   cp dist/bin/swresample-5.dll ../src-tauri/installers/dlls/
   cp dist/bin/swscale-8.dll ../src-tauri/installers/dlls/
   ```

## Verification After Copying

1. **Dependency check** — no new missing DLLs:
   ```
   cd src-tauri/installers/dlls
   powershell ./check_deps.ps1
   ```

2. **Dev run** — app starts, GStreamer initializes:
   ```
   npm run tauri dev
   ```

3. **Functional test** — select FFV1 encoding, trigger a recording, verify the `.mkv` output plays

4. **Production build** — installer builds successfully:
   ```
   npm run tauri build
   ```

## Expected Output

| DLL | Full GStreamer size | FFV1-only (expected) |
|-----|-------------------|---------------------|
| avcodec-61.dll | ~15 MB | ~300-500 KB |
| avformat-61.dll | ~2.3 MB | ~100-200 KB |
| avutil-59.dll | ~920 KB | ~500-700 KB |
| avfilter-10.dll | ~224 KB | ~50-100 KB |
| swresample-5.dll | ~132 KB | ~80-120 KB |
| swscale-8.dll | ~692 KB | ~200-400 KB |

## Troubleshooting

### `link` is not MSVC's linker

MSYS2's `/usr/bin/link` can shadow MSVC's `link.exe`. The build script detects this by checking whether `which link` resolves inside `/usr/` and prepends the MSVC bin directory to PATH to fix it automatically.

### ABI compatibility

FFmpeg 7.1.1 produces `avcodec-61.dll` (soversion 61), matching GStreamer 1.24's bundled FFmpeg. The ABI is stable across all FFmpeg 7.x releases, so 7.1.2 or 7.1.3 would also work if needed.

### Missing `make`, `nasm`, or `diffutils`

```
pacman -S make nasm diffutils
```

## How It Works

The key configure flags:

- `--disable-everything` — disables all codecs, muxers, demuxers, protocols, and filters
- `--enable-encoder=ffv1 --enable-decoder=ffv1` — re-enables only FFV1
- `--disable-avdevice --disable-postproc` — libraries not needed at all
- `--disable-network --disable-autodetect` — prevents linking external libraries
- `--extra-cflags="-MD"` — dynamic CRT linkage, matching GStreamer convention
- `--enable-shared` — produces DLLs (not static .lib only)

We keep avformat, avfilter, swscale, and swresample even though they're essentially empty stubs, because `gstlibav.dll` dynamically links all six FFmpeg libraries at load time.
