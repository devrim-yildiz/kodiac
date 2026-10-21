# Release v1.1.0

## Source history provenance

The July 10 through October 21, 2026 Git history is a reconstructed source-import
sequence prepared for the public release. Its 61 timestamps organize the imported
components and release work; they are not contemporaneous records of when those
features were originally implemented. Intermediate imports are not supported
builds. The v1.1.0 tag identifies the complete release tree. Later development
should preserve actual commit times and use ordinary incremental commits.

## Release validation

Run the commands in CONTRIBUTING after building the frontend. The release gates
are a locked Rust check, strict Clippy, rustfmt, warning-free rustdoc, frontend
build/formatting, synthetic Rust/FFmpeg integration tests, boot/restart checks and
Chromium integration tests. Results from the packaging run are recorded below.

Do not interpret a local workflow definition or a badge as a hosted GitHub CI run.
Container builds cover the selected target platform; physical VAAPI/NVIDIA/QSV/AMF
hardware and Safari, Infuse and Swiftfin devices require separate testing. TMDB
mock tests establish queue/cache/retry behavior, not a live key's permissions.

## Packaging audit

The release source contains text assets, lockfiles and migrations. Compiled
frontend output is generated before Rust builds and ignored by Git. Runtime data,
secrets, logs, node_modules, private validation artifacts and local deployment
scripts are excluded. Audit all reachable commit blobs, not just the final tree,
before publication. No GitHub remote or public image registry is assumed.
