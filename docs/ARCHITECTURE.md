# Architecture

## Process model

One executable hosts Axum 0.8 on Tokio. Shared `AppState` contains the SQLx pool,
persistent server ID, runtime configuration, search snapshot, concurrency budgets
and cancellation token. The native `/api/v1` and Jellyfin route groups share
account/session validation. Public routes are health, server identity, login and
embedded web assets. The SPA is built ahead of Rust compilation and embedded
with `rust-embed`; FFmpeg/ffprobe run as supervised child processes.

Startup validates the canonical media root, opens/migrates SQLite, bootstraps an
administrator when necessary, and starts scanner, TMDB, frame-artwork and search
workers. SIGINT/SIGTERM cancels work, drains HTTP serving, joins workers and
closes the pool. This is a single-process household server, not a distributed
scheduler. There is no external job broker.

## Media delivery and FIFO buffers

Direct responses use asynchronous file reads and a `ReaderStream`, supporting
HEAD, ETag/Last-Modified, conditional requests and one inclusive byte range.
Malformed or multipart ranges are ignored; valid unsatisfiable ranges return 416.
Canonical paths must stay inside the configured media root. Artwork may also
reside under the canonical metadata cache root.

The stream pipeline is implemented by `streaming::stream` and functions in
`streaming::transcode`, rather than types named StreamPipeline or Transcoder.
Remux uses FFmpeg `-c copy`; conversion selects a hardware encoder or libx264 and
AAC. Stdin is disabled; stdout is an OS FIFO pipe. A four-entry Tokio channel
bridges 64 KiB reads to the HTTP body. This bounds queued output and applies
backpressure without temporary media files. Byte buffers are copied at the bridge,
so packet-copy remuxing should not be described as end-to-end zero-copy.

An owned semaphore permit is acquired atomically without waiting. Exhaustion
returns 429. Both remux and conversion consume permits. The supervisor owns the
permit until child termination and reaping, including disconnect and cancellation.
Startup waits at most 12 seconds and holds no more than 8 MiB until real media
arrives. Steady-state reads and blocked sends have 30-second timeouts; the overall
runtime has a configurable cap. An encoder listed by FFmpeg is only a candidate.
Failure before media output tries another backend; failure after the response
starts terminates that response.

Fragmented MP4 uses `frag_keyframe+empty_moov+default_base_moof`. FFmpeg writes each
track's timescale; AAC uses audio sampling timing, not a forced video clock. The
server does not patch `mdhd` boxes. A copy seek can start before the requested
position at a keyframe; ffprobe determines the decode origin, returned as
`x-media-start-seconds`. The browser maps it with a SourceBuffer timestamp offset.
Safari playback needs real-device validation beyond these timing rules.

HLS is stateless VOD with independently encoded six-second MPEG-TS segments and
discontinuity markers. It has no adaptive ladder, shared on-disk segment cache or
cross-request encoder session. Subtitle extraction has its own two-process budget;
supported text codecs become bounded WebVTT. Bitmap subtitles are not burned in.

## SQLite pool and relational catalog

SQLx opens up to eight pooled SQLite connections with WAL, NORMAL synchronization,
foreign keys, a five-second busy timeout and five-second acquisition timeout.
Explicit mmap pragmas are not configured. WAL permits concurrent readers and one
writer; writes still serialize. Embedded migrations run before HTTP startup.
Fresh Unix database files are created with mode 0600; existing modes are preserved.

| Table | Responsibility |
| --- | --- |
| `media_items` | Lossless source inventory, stable file IDs, probe JSON and availability |
| `shows` | Library-scoped series identities and presentation metadata |
| `seasons` | Ordered show children; 0 is specials, -1 represents unknown |
| `episodes` | Source-backed episode records referencing seasons |
| `movies` | Source-backed movie presentation records |
| `profiles` | Account-owned named profiles with one default per account |
| `playback_history` | Profile/media resume ticks and completion state |
| `playback_sessions` | Legacy account-scoped progress adapter |
| `collections` and join tables | Movie/show groupings |
| `tmdb_jobs`, `frame_jobs` | Durable metadata and fallback-artwork jobs |

Triggers synchronize source records into relational rows and map legacy playback
to default profiles. Migrations preserve source IDs; multiple files for an episode
remain distinct. Title/order/availability and resume indices support common read
paths. A stopped-service copy or SQLite backup is required for consistent backups;
WAL is not a backup mechanism. Restore the corresponding schema snapshot on rollback.

## Discovery and metadata

A single scanner walks directories asynchronously, skips symlinks and trailer
sidecars, and filters recognized extensions. Traversal is sequential, without
parallel directory walking or gitignore-style rules. Unchanged size, nanosecond
mtime and probe version reuse bounded ffprobe results. Sidecars are rediscovered
on every scan. Filename regexes extract release title/year/SxxExx hints; local NFO
XML wins over hints. NFO reads are capped at 2 MiB and reject DTD/entity declarations.
Only a complete scan hides missing files; failures retain prior availability.

One asynchronous TMDB worker consumes SQLite jobs. API and image requests share
a four-token bucket replenished at four requests per second; HTTP 429 applies
Retry-After to the shared budget. Durable retries and revision checks avoid losing
pending work or applying stale enrichment. Local metadata/artwork remains preferred.
A separate FFmpeg worker generates missing artwork. Neither worker requires a key
for basic local library operation.

## Browser catalog and persistent playback

React 19, Zustand, Radix UI and Tailwind form the SPA. TanStack Virtual renders
visible catalog rows with overscan instead of mounting every poster. Data paging
and DOM virtualization are separate concerns. Show/season pages use relational API
IDs; source media IDs still drive playback. Fuzzy search reads a shared immutable
snapshot rebuilt outside its write lock every 25 seconds.

The video element and playback engine live above navigable catalog views. Docking
into the mini-player changes presentation while retaining the active decoder.
Direct play uses the media element; remux/conversion uses MediaSource or
ManagedMediaSource when supported. Stream reads are backpressured, buffered history
is pruned, unbuffered seeks restart the source, and progress writes serialize.
Switching profile/source intentionally starts a new playback session.
