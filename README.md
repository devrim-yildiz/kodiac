# Medienserver

A single-process media server with an embedded SQLite catalog.

## Run

Build the Rust application after preparing the browser assets when present.
Configure `--media-path`, `--db-path`, `--bind` and `--port` separately.
FFmpeg and ffprobe must be available on PATH.

## Media delivery

Original files support byte ranges. Compatible compressed tracks can be remuxed.
Conversion uses a bounded FFmpeg child process with software fallback.
Use a private library and temporary database during development.

## Metadata

Local NFO and artwork take priority over optional remote enrichment.
Never put production credentials or catalog data in the repository.
