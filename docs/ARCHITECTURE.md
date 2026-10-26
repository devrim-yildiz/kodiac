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

## Storage failures and independent workers

Run the production Compose stack with separate `server`, `scanner`, `artwork`, and
`metadata` roles. Keep `/data` (SQLite, WAL, and artwork cache) on reliable local
storage; media mounts remain read-only. A failing media disk can still prevent
playback, but a blocked scanner or FFmpeg worker has its own process, memory/PID
budget, and restart policy. The default `all` role preserves standalone behavior;
use the separate roles when process isolation is required.

Both scan APIs enqueue a durable request in `server` mode. The single scanner
process consumes requests and reports progress through `worker_status`; run only
one scanner per database. Artwork/metadata jobs have a five-minute deadline;
scans allow six hours overall, with 30 seconds per directory operation and
90 seconds per media item. Worker shutdown is
bounded because kernel disk I/O cannot always be cancelled. Search remains with
the server because its snapshot is in memory. Password verification has a
separate two-thread admission budget so media I/O cannot starve logins.

`/api/v1/live` checks the HTTP process without reading storage. `/api/v1/health`
checks SQLite with a two-second deadline. Response initialization has a
15-second deadline (streaming bodies keep their existing lifetime limits).
Logs contain request IDs, path without query strings, status, duration, and
worker/error categories and OS/SQLite codes. Secrets, authorization headers,
and remote error URLs are excluded. Compose rotates logs at 10 MB, three files.
The admin diagnostics show worker states/heartbeat ages; an unavailable artwork
size is reported as null instead of starting unlimited blocked directory walks.

Docker RAM caps require host memory-cgroup support. Hosts without that support
ignore `mem_limit`; process separation alone cannot prevent a host-wide OOM.
PID/thread budgets and job/request deadlines still apply independently.

### Anime classification and numbering

Migration 0007 adds anime classification, source episode ranges/labels and optional
verified TMDB numbering to `media_items`. The first startup reclassifies existing
anime filename hints atomically from SQLite without touching media files. It
retains source IDs, local NFO metadata, playback rows and collection membership;
samples become unavailable without deleting their records. Regular scanning also
uses show/season directories, inherited show NFOs, explicit movie NFOs and artwork.

Absolute source labels remain independent of canonical season numbering. Metadata
uses one unambiguous TMDB Absolute episode group; incomplete or ambiguous mappings
stay unassigned. Fractional/range ordering drives native next-episode discovery.
Both browser discovery and Jellyfin expose the mixed Anime collection; unknown
seasons remain separate from specials.
