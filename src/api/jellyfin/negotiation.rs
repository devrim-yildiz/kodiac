//! Conservative Jellyfin PlaybackInfo device-profile matching.
//!
//! Direct-play profiles match container and codecs, codec/container conditions and
//! bitrate limits. MP4 remuxing is advertised only for supported copied codecs;
//! transcoding targets H.264/AAC in MP4 or six-second MPEG-TS HLS. Unsupported
//! required conditions reject a match. Absolute AudioStreamIndex values are mapped
//! to audio-only FFmpeg ordinals for clients such as Infuse. Swiftfin profiles use
//! the same negotiation path. A request without a device profile is metadata discovery.

//! Conservative profile matching: unsupported conditions never become a promise to play.
use crate::{
    db::MediaItem,
    error::{AppError, Result},
};
use serde::Deserialize;
use serde_json::{Value, json};
/// Optional client profile, selected tracks, resume position and delivery permissions.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PlaybackRequest {
    /// Jellyfin DeviceProfile describing transport, codecs and constraints.
    pub device_profile: Option<Value>,
    /// Absolute ffprobe audio stream index, translated to an audio-only ordinal.
    pub audio_stream_index: Option<u64>,
    /// Absolute subtitle stream index, or -1 to disable subtitles.
    pub subtitle_stream_index: Option<i64>,
    /// Requested resume position in 100-nanosecond ticks.
    pub start_time_ticks: Option<u64>,
    /// Client bitrate ceiling in bits per second.
    pub max_streaming_bitrate: Option<u64>,
    /// Whether the client permits delivery of the original file.
    pub enable_direct_play: Option<bool>,
    /// Whether the client permits container remuxing.
    pub enable_direct_stream: Option<bool>,
    /// Whether the client permits video conversion.
    pub enable_transcoding: Option<bool>,
}
fn accepts(list: &Value, codec: &str) -> bool {
    list.as_str()
        .is_none_or(|s| s.is_empty() || s.split(',').any(|s| s.trim().eq_ignore_ascii_case(codec)))
}
fn conditions(profile: &Value, video: &Value, audio: &Value, bitrate: Option<u64>) -> bool {
    profile["Conditions"].as_array().is_none_or(|cs| {
        cs.iter().all(|c| {
            let property = c["Property"].as_str().unwrap_or("");
            let actual = match property {
                "Width" => video["width"].as_f64(),
                "Height" => video["height"].as_f64(),
                "VideoBitDepth" => video["pix_fmt"].as_str().map(|p| {
                    if p.contains("10") {
                        10.0
                    } else if p.contains("12") {
                        12.0
                    } else {
                        8.0
                    }
                }),
                "VideoLevel" => video["level"].as_f64(),
                "AudioChannels" => audio["channels"].as_f64(),
                "AudioSampleRate" => audio["sample_rate"].as_str().and_then(|s| s.parse().ok()),
                "VideoBitrate" | "AudioBitrate" => if property == "VideoBitrate" {
                    &video["bit_rate"]
                } else {
                    &audio["bit_rate"]
                }
                .as_str()
                .and_then(|s| s.parse().ok()),
                "Bitrate" => bitrate.map(|n| n as f64),
                _ => None,
            };
            if property == "VideoProfile" {
                return video["profile"]
                    .as_str()
                    .is_some_and(|p| match c["Condition"].as_str() {
                        Some("Equals" | "EqualsAny") => c["Value"]
                            .as_str()
                            .unwrap_or("")
                            .split('|')
                            .any(|s| s.eq_ignore_ascii_case(p)),
                        Some("NotEquals") => {
                            !c["Value"].as_str().unwrap_or("").eq_ignore_ascii_case(p)
                        }
                        _ => false,
                    });
            }
            let Some(actual) = actual else {
                return c["IsRequired"] == false;
            };
            let Some(expected) = c["Value"].as_str().and_then(|s| s.parse::<f64>().ok()) else {
                return false;
            };
            match c["Condition"].as_str() {
                Some("Equals") => actual == expected,
                Some("NotEquals") => actual != expected,
                Some("LessThanEqual") => actual <= expected,
                Some("GreaterThanEqual") => actual >= expected,
                _ => false,
            }
        })
    })
}
fn profiles_match(
    profile: &Value,
    video: &Value,
    audio: &Value,
    bitrate: Option<u64>,
    container: &str,
) -> bool {
    profile["CodecProfiles"].as_array().is_none_or(|ps| {
        ps.iter().all(|p| {
            let codec = if p["Type"] == "Video" {
                &video["codec_name"]
            } else {
                &audio["codec_name"]
            };
            !accepts(&p["Codec"], codec.as_str().unwrap_or(""))
                || conditions(p, video, audio, bitrate)
        })
    }) && profile["ContainerProfiles"].as_array().is_none_or(|ps| {
        ps.iter()
            .all(|p| !accepts(&p["Container"], container) || conditions(p, video, audio, bitrate))
    })
}
/// Evaluate a PlaybackInfo profile and build a media source with supported delivery URLs.
///
/// # Errors
/// Returns bad-request when a requested absolute audio index does not exist.
pub fn source(item: &MediaItem, request: &PlaybackRequest) -> Result<Value> {
    let mut source = super::dto::source(item);
    let streams: Vec<Value> = serde_json::from_str(&item.streams_json).unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .cloned()
        .unwrap_or(Value::Null);
    let audios: Vec<_> = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect();
    let audio = if let Some(index) = request.audio_stream_index {
        audios
            .iter()
            .find(|s| s["index"].as_u64() == Some(index))
            .copied()
            .ok_or_else(|| AppError::BadRequest("invalid AudioStreamIndex".into()))?
    } else {
        audios
            .iter()
            .find(|s| s["disposition"]["default"] == 1)
            .copied()
            .or_else(|| audios.first().copied())
            .unwrap_or(&Value::Null)
    };
    let ordinal = audios.iter().position(|s| *s == audio);
    let vc = video["codec_name"].as_str().unwrap_or("");
    let ac = audio["codec_name"].as_str().unwrap_or("");
    let bitrate = item
        .runtime_seconds
        .filter(|n| *n > 0.0)
        .map(|n| (item.size_bytes as f64 * 8.0 / n) as u64);
    let mut direct;
    let mut remux = false;
    let mut transcode = false;
    let mut hls = false;
    if let Some(profile) = &request.device_profile {
        let limit = request
            .max_streaming_bitrate
            .or(profile["MaxStreamingBitrate"].as_u64());
        let within = limit.is_none_or(|n| bitrate.is_some_and(|b| b <= n));
        direct = within
            && profiles_match(profile, &video, audio, bitrate, &item.container)
            && profile["DirectPlayProfiles"].as_array().is_some_and(|ps| {
                ps.iter().any(|p| {
                    p["Type"] == "Video"
                        && accepts(&p["Container"], &item.container)
                        && accepts(&p["VideoCodec"], vc)
                        && (ac.is_empty() || accepts(&p["AudioCodec"], ac))
                })
            });
        // Negotiate only transports and output codecs actually implemented.
        let targets: Vec<&Value> = profile["TranscodingProfiles"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .filter(|p| {
                        p["Type"] == "Video"
                            && p["Container"].as_str() == Some("mp4")
                            && p["Protocol"]
                                .as_str()
                                .is_none_or(|s| s.eq_ignore_ascii_case("http"))
                            && p["Context"]
                                .as_str()
                                .is_none_or(|s| s.eq_ignore_ascii_case("Streaming"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        remux = within
            && profiles_match(profile, &video, audio, bitrate, "mp4")
            && matches!(vc, "h264" | "hevc" | "av1")
            && matches!(ac, "" | "aac" | "mp3" | "ac3" | "eac3" | "alac")
            && targets.iter().any(|p| {
                accepts(&p["VideoCodec"], vc) && (ac.is_empty() || accepts(&p["AudioCodec"], ac))
            });
        // The initial beta encoder targets 4Mbps video + 192kbps audio and does not
        // claim to satisfy lower bitrate limits or unsupported output conditions.
        let output_video = json!({"codec_name":"h264","width":video["width"],"height":video["height"],"pix_fmt":"yuv420p","profile":"High","bit_rate":"4000000"});
        let output_audio = json!({"codec_name":"aac","channels":2,"bit_rate":"192000"});
        let output_matches = |container: &str, target: &Value| {
            profiles_match(
                profile,
                &output_video,
                &output_audio,
                Some(4_192_000),
                container,
            ) && conditions(target, &output_video, &output_audio, Some(4_192_000))
        };
        hls = item
            .runtime_seconds
            .is_some_and(|n| n > 0.0 && n <= 604800.0)
            && profile["TranscodingProfiles"].as_array().is_some_and(|ps| {
                ps.iter().any(|p| {
                    p["Type"] == "Video"
                        && p["Protocol"]
                            .as_str()
                            .is_some_and(|s| s.eq_ignore_ascii_case("hls"))
                        && accepts(&p["Container"], "ts")
                        && accepts(&p["VideoCodec"], "h264")
                        && accepts(&p["AudioCodec"], "aac")
                        && p["Context"]
                            .as_str()
                            .is_none_or(|s| s.eq_ignore_ascii_case("Streaming"))
                        && output_matches("ts", p)
                })
            });
        transcode = limit.is_none_or(|n| n >= 4_192_000)
            && (hls
                || targets.iter().any(|p| {
                    accepts(&p["VideoCodec"], "h264")
                        && accepts(&p["AudioCodec"], "aac")
                        && output_matches("mp4", p)
                }))
            && !vc.is_empty();
    } else {
        // GET PlaybackInfo without a device profile is metadata discovery; clients
        // decide whether they can decode the original file themselves.
        direct = true;
    }
    direct &= request.enable_direct_play != Some(false);
    remux &= request.enable_direct_stream != Some(false);
    transcode &= request.enable_transcoding != Some(false);
    source["SupportsDirectPlay"] = json!(direct);
    source["SupportsDirectStream"] =
        json!(request.enable_direct_stream != Some(false) && (direct || remux));
    if source["SupportsDirectStream"] == false {
        source["DirectStreamUrl"] = Value::Null;
    }
    source["SupportsTranscoding"] = json!(transcode);
    source["DefaultAudioStreamIndex"] = audio["index"].clone();
    source["DefaultSubtitleStreamIndex"] = json!(request.subtitle_stream_index.unwrap_or(-1));
    source["Bitrate"] = json!(bitrate);
    if remux || transcode {
        let mode = if remux { "remux" } else { "transcode" };
        let mut url = format!(
            "/Videos/{}/stream.mp4?mode={mode}&StartTimeTicks={}",
            item.id,
            request.start_time_ticks.unwrap_or(0)
        );
        let use_hls = hls && !remux;
        if use_hls {
            url = format!(
                "/Videos/{}/master.m3u8?StartTimeTicks={}",
                item.id,
                request.start_time_ticks.unwrap_or(0)
            );
        }
        if let Some(n) = ordinal {
            url.push_str(&format!("&audio_index={n}"));
        }
        if remux && !direct {
            source["DirectStreamUrl"] = json!(url);
        }
        source["TranscodingUrl"] = json!(url);
        source["TranscodingContainer"] = json!(if use_hls { "ts" } else { "mp4" });
        source["TranscodingSubProtocol"] = json!(if use_hls { "hls" } else { "http" });
    }
    Ok(source)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_constraints_are_conservative() {
        let video = json!({"width":1920,"height":1080,"pix_fmt":"yuv420p10le"});
        let audio = json!({"channels":6});
        assert!(!conditions(
            &json!({"Conditions":[{"Property":"VideoBitDepth","Condition":"LessThanEqual","Value":"8","IsRequired":true}]}),
            &video,
            &audio,
            None
        ));
        assert!(!conditions(
            &json!({"Conditions":[{"Property":"Unknown","Condition":"Equals","Value":"1","IsRequired":true}]}),
            &video,
            &audio,
            None
        ));
        assert!(accepts(&json!("h264,hevc"), "hevc"));
        assert!(!accepts(&json!("h264"), "hevc"));
    }
    #[test]
    fn conversion_checks_output_constraints_for_http_and_hls() {
        let item = MediaItem {
            is_anime: false,
            episode_end: None,
            episode_label: None,
            id:"item".into(),library_id:"lib".into(),path:"/movie.mkv".into(),kind:"movie".into(),
            title:"Movie".into(),plot:String::new(),year:None,runtime_seconds:Some(120.0),series_name:None,
            season:None,episode:None,poster_path:None,backdrop_path:None,container:"mkv".into(),
            video_codec:Some("hevc".into()),audio_codec:Some("aac".into()),size_bytes:1000,
            streams_json:json!([{"index":0,"codec_type":"video","codec_name":"hevc","width":3840,"height":2160},{"index":1,"codec_type":"audio","codec_name":"aac","channels":6}]).to_string(),
        };
        for (container, protocol) in [("mp4", "http"), ("ts", "hls")] {
            for (property, expected, accepted) in [
                ("Width", "1920", false),
                ("Width", "4096", true),
                ("AudioChannels", "1", false),
                ("AudioChannels", "2", true),
                ("Unknown", "1", false),
                ("VideoLevel", "51", false),
            ] {
                let request:PlaybackRequest = serde_json::from_value(json!({"DeviceProfile":{
                    "TranscodingProfiles":[{"Type":"Video","Container":container,"Protocol":protocol,"VideoCodec":"h264","AudioCodec":"aac"}],
                    "CodecProfiles":[{"Type":if property=="AudioChannels" {"Audio"} else {"Video"},"Codec":if property=="AudioChannels" {"aac"} else {"h264"},"Conditions":[{"Property":property,"Condition":"LessThanEqual","Value":expected,"IsRequired":true}]}]
                }})).unwrap();
                assert_eq!(
                    source(&item, &request).unwrap()["SupportsTranscoding"],
                    accepted,
                    "{protocol}: {property} <= {expected}"
                );
            }
            let request:PlaybackRequest = serde_json::from_value(json!({"DeviceProfile":{
                "TranscodingProfiles":[{"Type":"Video","Container":container,"Protocol":protocol,"VideoCodec":"h264","AudioCodec":"aac"}],
                "ContainerProfiles":[{"Container":container,"Conditions":[{"Property":"Height","Condition":"LessThanEqual","Value":"1080","IsRequired":true}]}]
            }})).unwrap();
            assert_eq!(
                source(&item, &request).unwrap()["SupportsTranscoding"],
                false
            );
        }
    }
}
