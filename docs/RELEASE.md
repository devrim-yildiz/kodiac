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

## Packaging results

Validated on October 22, 2026 using Rust 1.99.1 on Linux ARM64 in an isolated
Docker environment, and Node.js 25.8.2 / Chromium on macOS for frontend checks.
The Mac host had no native Rust installation. The generated manual is available
under the ignored `target/doc/kodiac/index.html` build output.

| Check | Result |
| --- | --- |
| `cargo check --locked` | Passed |
| `cargo fmt --check` | Passed |
| Strict Clippy across all targets | Passed |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked` | Passed, zero warnings |
| Default Rust suite | 28 passed; four environment-dependent tests ignored |
| Explicit real FFmpeg HTTP suite | Three passed; resolves three default-suite skips |
| Frontend typecheck and production build | Passed |
| Frontend Prettier check | Passed |
| Chromium browser integration suite | 16 passed |
| Optimized Linux ARM64 binary | Built and booted |
| Authenticated boot, restart, default profile and SIGTERM | Passed with temporary data |
| Compose interpolation/configuration | Passed with an explicit media-path variable |
| Local Markdown links | Resolved |

The private historical snapshot migration test was not run; synthetic migration
coverage includes 5,172 source records and playback preservation. The macOS-only
VideoToolbox test was not compiled on Linux. Physical GPU/client tests and a full
Dockerfile image build were not performed in this packaging run. No production
service, media library or runtime database was changed. No hosted GitHub CI run
or public publication is claimed.

Before tagging, the source allowlist is checked for UTF-8 text, forbidden file
paths and emoji characters. After history generation, audit every reachable blob
for binary content and known credential patterns, verify all 61 commits have
unique trees and strictly chronological timestamps, and check the annotated tag,
clean working tree and absence of remotes. Pattern scanning cannot prove the
absence of every possible secret; the import also excludes private source folders
and all machine-specific deployment scripts by construction.
