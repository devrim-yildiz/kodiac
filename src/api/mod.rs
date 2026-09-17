
pub mod jellyfin {
    pub mod negotiation {
        
        use crate::{
            db::MediaItem,
            error::{AppError, Result},
        };
        use serde::Deserialize;
        use serde_json::{Value, json};
        #[derive(Default, Deserialize)]
        #[serde(default, rename_all = "PascalCase")]
        pub struct PlaybackRequest {
            pub device_profile: Option<Value>,
            pub audio_stream_index: Option<u64>,
            pub subtitle_stream_index: Option<i64>,
            pub start_time_ticks: Option<u64>,
            pub max_streaming_bitrate: Option<u64>,
            pub enable_direct_play: Option<bool>,
            pub enable_direct_stream: Option<bool>,
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
                let codec_ok = profile["CodecProfiles"].as_array().is_none_or(|ps| {
                    ps.iter().all(|p| {
                        let codec = if p["Type"] == "Video" { vc } else { ac };
                        !accepts(&p["Codec"], codec) || conditions(p, &video, audio, bitrate)
                    })
                });
                let container_ok = profile["ContainerProfiles"].as_array().is_none_or(|ps| {
                    ps.iter().all(|p| {
                        !accepts(&p["Container"], &item.container) || conditions(p, &video, audio, bitrate)
                    })
                });
                direct = within
                    && codec_ok
                    && container_ok
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
                    && codec_ok
                    && matches!(vc, "h264" | "hevc" | "av1")
                    && matches!(ac, "" | "aac" | "mp3" | "ac3" | "eac3" | "alac")
                    && targets.iter().any(|p| {
                        accepts(&p["VideoCodec"], vc) && (ac.is_empty() || accepts(&p["AudioCodec"], ac))
                    });
                // The initial beta encoder targets 4Mbps video + 192kbps audio and does not
                // claim to satisfy lower bitrate limits or unsupported output conditions.
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
                        })
                    });
                transcode = limit.is_none_or(|n| n >= 4_192_000)
                    && (hls
                        || targets.iter().any(|p| {
                            accepts(&p["VideoCodec"], "h264") && accepts(&p["AudioCodec"], "aac")
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
            source["SupportsDirectStream"] = json!(direct || remux);
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
        }
    }
    pub mod navigation;
    pub mod dto;
}
pub mod v1;
use axum::{extract::Request, middleware::Next, response::Response};
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers
        .entry("cache-control")
        .or_insert("no-store".parse().unwrap());
    response
}
