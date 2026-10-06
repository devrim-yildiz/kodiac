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

## Client API


GET routes also accept HEAD through Axum where applicable. All routes below
require authentication except the three explicitly public routes.

| Method | Path | Behavior |
| --- | --- | --- |
| GET | `/System/Info/Public` | Public identity, product and version |
| GET | `/Users/Public` | Public empty user list |
| POST | `/Users/AuthenticateByName` | Public account login and access token |
| GET | `/System/Info` | Authenticated identity |
| GET | `/Users/Me`, `/Users/{id}` | Authenticated account only |
| GET | `/UserViews`, `/Users/{id}/Views` | Movies and Series UserViews |
| GET | `/Items`, `/Users/{id}/Items` | Filtered, ordered and paginated navigation |
| GET | `/Items/{id}`, `/Users/{user_id}/Items/{id}` | Item metadata and resume data |
| GET | `/Shows/{id}/Seasons`, `/Shows/{id}/Episodes` | Television hierarchy |
| GET, POST | `/Items/{id}/PlaybackInfo` | Media sources and device-profile negotiation |
| GET | `/Items/{id}/Images/Primary`, `/Items/{id}/Images/Backdrop` | Contained artwork; `/0` aliases supported |
| GET | `/Videos/{id}/stream`, `/Videos/{id}/stream.mp4` | Original, remux or conversion output |
| GET | `/Videos/{id}/master.m3u8`, `/Videos/{id}/hls/{segment}` | Fixed-rendition VOD HLS |
| GET | `/Videos/{id}/{source}/Subtitles/{index}/Stream.vtt` | Text subtitle extraction |
| POST | `/Sessions/Capabilities`, `/Sessions/Capabilities/Full` | Acknowledged with 204; no persistent device capability registry |
| POST | `/Sessions/Playing`, `/Sessions/Playing/Progress`, `/Sessions/Playing/Stopped` | Persist resume position |
| POST | `/Sessions/Logout` | Revoke the current session |

Navigation supports the implemented subset of `ParentId`, `IncludeItemTypes`,
`SearchTerm`, `StartIndex`, `Limit`, `SortBy`, `SortOrder`, season filters and
recursive selection. Unknown features are not a promise of full Jellyfin behavior.

## Authentication and UserViews

Login accepts Jellyfin `Username` and `Pw`/`Password` aliases. Requests can use
`X-Emby-Token`, Bearer authorization, MediaBrowser token authorization or the
session cookie. Read-only media requests also accept `api_key`/`AccessToken` query
parameters. Do not log full media URLs: they can contain access tokens.
User-specific paths reject access to another account. Native named profiles remain
account-owned; Jellyfin session reports map to that account's default profile.

## PlaybackInfo matching

A POST supplies `DeviceProfile` with DirectPlayProfiles, TranscodingProfiles,
CodecProfiles and ContainerProfiles, optional bitrate ceilings, selected tracks
and enable flags. A GET without a profile describes original media and lets the
client decide whether it can decode it.

1. Direct play matches original container, video/audio codecs, bitrate and
   supported profile constraints.
2. Direct stream offers MP4 remux only for implemented copy-compatible codecs
   and matching HTTP MP4 targets. No video decode is performed.
3. Conversion offers H.264/AAC MP4 or MPEG-TS HLS. The fixed target is 4 Mbps video
   plus 192 kbps audio, so lower ceilings are not advertised as satisfied.
4. Unknown required conditions reject a match. EnableDirectPlay,
   EnableDirectStream and EnableTranscoding restrict advertised options.

`AudioStreamIndex` is an absolute ffprobe stream index, not an audio ordinal.
Medienserver resolves it to `0:a:N` for FFmpeg and embeds that ordinal in negotiated
URLs. Invalid indices return 400. `StartTimeTicks`, resume positions and runtime
use 100-nanosecond units. Playback reports persist across restart.


## Browser development

Run `npm ci --prefix web` and `npm run build --prefix web` before compiling Rust.
The browser client uses typed native APIs and a shared playback engine.
