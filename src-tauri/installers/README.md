# Installer Resources

This folder contains third-party installers that are bundled with Sacho.

## GStreamer Runtime

Sacho requires GStreamer for video capture and playback. The installer bundles a private copy of GStreamer so users don't need to install it separately.

### Downloading GStreamer

1. Go to https://gstreamer.freedesktop.org/download/
2. Download the **MSVC 64-bit runtime** installer (NOT the development version)
   - File should be named something like: `gstreamer-1.0-msvc-x86_64-1.x.x.msi`
3. Rename the file to: `gstreamer-1.0-msvc-x86_64.msi`
4. Place it in this folder (`src-tauri/installers/`)

### Version Requirements

- Minimum version: GStreamer 1.20.0
- Recommended: Latest stable release
- Must be the MSVC build (not MinGW)
- Must be the runtime package (not development)

### Size Considerations

The GStreamer runtime MSI is approximately 50-100 MB. This will be included in the final Sacho installer.

If the MSI is not present during build, the installer will still be created, but users will need to install GStreamer system-wide separately.

### LGPL Compliance

Sacho only uses LGPL-licensed GStreamer plugins. The following plugins are NOT used:
- x264 (GPL)
- x265 (GPL) 
- Any other GPL-only plugins

The GStreamer runtime package includes both LGPL and GPL plugins, but Sacho's code only links to LGPL components.
