# Jellyfin compatibility

Kodiac implements a selected HTTP contract, not the Jellyfin server or its plugin
system. Routes are case-sensitive. Configure clients with the server base URL
(for example `http://server:8097`), without an `/api/v1` suffix. The compatibility
`Version` field is 10.10.7; `ServerVersion` identifies the actual Kodiac release.
It does not claim to run Jellyfin itself. Discovery is HTTP identity discovery;
UDP broadcast discovery is not implemented.

## Supported routes

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
Kodiac resolves it to `0:a:N` for FFmpeg and embeds that ordinal in negotiated
URLs. Invalid indices return 400. `StartTimeTicks`, resume positions and runtime
use 100-nanosecond units. Playback reports persist across restart.

## Infuse and Swiftfin

In either client, add a Jellyfin server using the base URL and the Kodiac account.
Infuse commonly decodes the original container itself; its selected absolute audio
index is translated as above. Swiftfin may submit direct-play and conversion
profiles; the same conservative matcher applies. Successful profile fixtures are
not a substitute for testing released client versions on hardware.

| Client/path | Release evidence | Remaining work |
| --- | --- | --- |
| Native browser / Chromium | Synthetic HTTP and browser integration suite | Device/codec-specific coverage |
| Infuse | Server-side navigation/profile/audio-index contracts tested | Successful real-device login and playback not established |
| Swiftfin | Shared server-side profile contract | Physical iOS/tvOS integration not established |
| Safari | fMP4 generation and timestamp-origin handling implemented | Real AAC/fMP4 playback and seek validation |

Unsupported areas include administration APIs, plugins, live TV, DVR, remote
control, adaptive streaming ladders, full HDR/Dolby Vision tone mapping and bitmap
subtitle burn-in. Report client version, platform, sanitized profile and observed
endpoint behavior when filing compatibility issues. Never attach tokens or media URLs
with query credentials.
