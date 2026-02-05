This folder contains third-party dependencies that are bundled with Sacho.

Sacho requires GStreamer for video capture and playback. Instead of bundling the full GStreamer MSI installer, we bundle only the specific DLLs that Sacho needs. This is a commercial application and we can only include lgpl licensed DLLs containing no unlicensed codecs.