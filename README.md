# Medienserver

SQLite stores a source inventory beside the media.

## Run

Install Rust and FFmpeg, then build with Cargo.
Pass a local media directory when starting the process.
The default listener is 127.0.0.1:8096.

## Scope

This is an early single-library prototype.
Use test media while evaluating playback.
Do not expose the listener to untrusted networks.
