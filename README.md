<div align="center">

# KODIAC

**Single-binary personal cinema engine and media streaming server written in Rust.**

[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)
[![Rust Edition 2024](https://img.shields.io/badge/rust-edition%202024-orange.svg)](Cargo.toml)
[![CI workflow](https://img.shields.io/badge/CI-Verify-blue.svg)](.github/workflows/ci.yml)
[![Architecture: arm64 | amd64](https://img.shields.io/badge/arch-arm64%20%7C%20amd64-blue.svg)](docs/ARCHITECTURE.md)

</div>

---

Kodiac serves private movie and television libraries through a Rust HTTP server and
an embedded React 19 client. It supports original-file playback, streamed container
remuxing, H.264/AAC conversion, local NFO metadata, optional TMDB enrichment, and
account-scoped playback profiles. The application ships as one executable;
FFmpeg and ffprobe are separate runtime dependencies. Node.js is needed only to
build the frontend or run its development server.

## Key architectural principles

- **Embedded web client.** Vite builds the React SPA; `rust-embed` bundles its assets
  into the release executable. No separate web server is required.
- **Bounded streaming.** Direct playback supports HTTP byte ranges. Remuxing copies
  compressed packets without decoding. FFmpeg output passes through an OS pipe and
  bounded memory channel without temporary segment files. HTTP buffers are copied;
  the transport is not literally zero-copy.
- **Embedded relational storage.** SQLite uses WAL, foreign keys, a connection pool
  of up to eight connections, five-second busy/acquisition timeouts and additive
  embedded migrations. No external database service is required.
- **Jellyfin API subset.** Server identity, login, Movies/Series views, navigation,
  PlaybackInfo and resume reporting support compatible client workflows. Infuse
  and Swiftfin device validation remains outstanding; see the compatibility matrix.
- **Persistent playback UI.** A shared player survives catalog navigation. TanStack
  Virtual bounds rendered catalog rows; motion and ambient artwork respect the
  existing client controls.

## Hardware acceleration

| Platform | Modes | Limits |
| --- | --- | --- |
| Raspberry Pi 5 | Direct play, remux, software H.264 | Use `HW_ACCEL=software`; no built-in encoder or implemented V4L2 request decoder |
| macOS / Apple Silicon | VideoToolbox, software | Requires an FFmpeg build exposing `h264_videotoolbox` |
| Linux x86_64 / ARM64 | VAAPI, NVENC, software | Device and driver support required; VAAPI defaults to `/dev/dri/renderD128` |
| Windows | QSV, NVENC, AMF, software | Backend selection exists; physical device validation is outstanding |

`auto` tries compiled platform encoders, then software. Each stream must produce
media before the HTTP success response; initialization failure tries the next
backend. Failure after streaming has begun ends that response. Remuxing consumes
the same concurrency budget as conversion. Software conversion uses the runtime
preset, `ultrafast` by default. HLS is one fixed H.264/AAC rendition, not adaptive
bitrate streaming. HDR tone mapping and bitmap subtitle burn-in are not provided.

## Quick start

### Docker Compose

Build locally with Docker Compose. No public registry image is claimed.
Create a private initial administrator password with at least 12 characters:

```sh
umask 077
openssl rand -base64 32 > admin-password.secret
mkdir -p data
# The image runs as UID/GID 10001. On Linux, prepare the bind mount:
sudo chown 10001:10001 data admin-password.secret
export KODIAC_MEDIA_PATH=/absolute/path/to/media
docker compose up -d --build
```

The included [compose.yaml](compose.yaml) publishes `8097:8096`, mounts media
read-only, persists the database and artwork under `./data`, and loads the first
password from a Compose secret. Open `http://localhost:8097` and sign in as
`admin` using the contents of `admin-password.secret` (`sudo cat admin-password.secret`
on Linux after the ownership change). The file is used only to
create the initial account; changing it does not rotate an existing password.

The Linux container's secret file must be readable by UID 10001. Arrange its
ownership or a group/ACL for that UID while keeping it private. Docker Desktop
file-sharing permissions differ from Linux bind mounts.

### Native binary execution

Install Rust 1.99.1 (the release toolchain), Node.js 22.12+ or 24+, npm,
FFmpeg and ffprobe. Build the frontend before Rust, because its output is embedded:

```sh
npm ci --prefix web
npm --prefix web run build
cargo build --release --locked
umask 077
mkdir -p data
openssl rand -base64 32 > admin-password.secret
./target/release/kodiac \
  --media-path /mnt/media \
  --db-path ./data/media.db \
  --bind 127.0.0.1 --port 8096 \
  --admin-password-file ./admin-password.secret
```

Use `--bind 0.0.0.0` for LAN access. `--bind` accepts an IP address; `--port`
selects the port. Run `./target/release/kodiac --help` for all environment variables
and flags. `TMDB_TOKEN` is optional; a library works with local metadata alone.
Configure hardware with `HW_ACCEL=software`, `auto`, `vaapi`, `nvenc`, `qsv`,
`amf`, or `videotoolbox`. Settings saved in the administrator UI override startup
hardware and TMDB values.

Back up the database and cached artwork before upgrading. Stop the service or use
a SQLite-consistent backup; copying the live main database alone can omit WAL
transactions. Rollback requires the matching pre-upgrade database and executable.

## Performance and validation

Kodiac avoids video decoding when the client can play the original stream or a
remux. CPU, memory, startup latency and query latency depend on media, client,
cache state, FFmpeg and storage. This release does not publish an independently
reproducible Raspberry Pi comparison against Jellyfin or Plex, or guarantee a
sub-150 ms first frame, a fixed RAM footprint or a particular CPU percentage.

The test suite covers synthetic FFmpeg media, range requests, process cleanup,
authentication, migrations, catalog hierarchy, metadata retries and Jellyfin
profile negotiation. Browser tests use an isolated synthetic catalog. See
[release validation](docs/RELEASE.md) for commands and remaining device limits.

## Documentation

- [Architecture overview](docs/ARCHITECTURE.md)
- [Client compatibility and API](docs/JELLYFIN_COMPATIBILITY.md)
- [Contributing guide](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Release provenance and validation](docs/RELEASE.md)

Build the Rust technical manual after building the frontend:

```sh
cargo doc --no-deps --open
```

## License

Kodiac is licensed under the GNU General Public License v3.0 only.
See [LICENSE](LICENSE) and [third-party notices](NOTICE).
