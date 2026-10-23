//! PascalCase Jellyfin response projections from local users and probed source media.

use crate::{auth::User, db::MediaItem};
use serde::Serialize;
use serde_json::{Value, json};

/// Paginated Jellyfin Items response with total count and starting offset.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemsResponse {
    /// Records in the current page.
    pub items: Vec<Value>,
    /// Total matching records before pagination.
    pub total_record_count: i64,
    /// Zero-based first record offset.
    pub start_index: i64,
}
/// Project a local account into a Jellyfin user response.
pub fn user(user: &User, server: &str) -> Value {
    json!({"Id":user.id,"Name":user.username,"ServerId":server,"HasPassword":true,"HasConfiguredPassword":true,"HasConfiguredEasyPassword":false,
        "Configuration":{"AudioLanguagePreference":"","PlayDefaultAudioTrack":true,"SubtitleMode":"Default","EnableNextEpisodeAutoPlay":true},
        "Policy":{"IsAdministrator":true,"IsHidden":false,"IsDisabled":false,"EnableMediaPlayback":true,"EnableVideoPlaybackTranscoding":true,"EnableAudioPlaybackTranscoding":true,"EnablePlaybackRemuxing":true,"EnableAllFolders":true,"EnableContentDownloading":true,"EnableSyncTranscoding":false}})
}
fn video_range(stream: &Value) -> &'static str {
    match stream["color_transfer"].as_str() {
        Some("smpte2084") => "HDR10",
        Some("arib-std-b67") => "HLG",
        _ => "SDR",
    }
}
/// Project probed tracks and source properties into Jellyfin MediaSourceInfo.
pub fn source(item: &MediaItem) -> Value {
    let raw: Vec<Value> = serde_json::from_str(&item.streams_json).unwrap_or_default();
    let streams: Vec<Value> = raw.iter().map(|stream| json!({
        "Index": stream["index"], "Type": match stream["codec_type"].as_str() { Some("video")=>"Video",Some("audio")=>"Audio",Some("subtitle")=>"Subtitle",_=>"Data" },
        "VideoRange":video_range(stream),"Codec":stream["codec_name"],"Width":stream["width"],"Height":stream["height"],"Channels":stream["channels"],
        "SampleRate":stream["sample_rate"].as_str().and_then(|s| s.parse::<u64>().ok()),"Language":stream["tags"]["language"],"Title":stream["tags"]["title"],
        "IsExternal":stream["external_path"].is_string(),"IsDefault":stream["disposition"]["default"]==1,"IsForced":stream["disposition"]["forced"]==1,
        "IsTextSubtitleStream":stream["codec_type"]=="subtitle" && crate::streaming::subtitles::is_text(stream["codec_name"].as_str().unwrap_or("")),
        "DeliveryMethod":if stream["codec_type"]=="subtitle" && crate::streaming::subtitles::is_text(stream["codec_name"].as_str().unwrap_or("")) {Some("External")} else {None},
        "DeliveryUrl":if stream["codec_type"]=="subtitle" && crate::streaming::subtitles::is_text(stream["codec_name"].as_str().unwrap_or("")) {Some(format!("/Videos/{}/{}/Subtitles/{}/Stream.vtt",item.id,item.id,stream["index"]))}else{None},
        "DisplayTitle":stream["tags"]["title"].as_str().or(stream["tags"]["language"].as_str()).unwrap_or("Unknown")
    })).collect();
    json!({"Id":item.id,"Name":item.title,"Protocol":"Http","Type":"Default","Container":item.container,
        "Path":format!("/Videos/{}/stream?Static=true",item.id),"DirectStreamUrl":format!("/Videos/{}/stream?Static=true",item.id),
        "Size":item.size_bytes,"RunTimeTicks":item.runtime_seconds.map(|n|(n*10_000_000.0) as i64),
        "SupportsDirectPlay":true,"SupportsDirectStream":true,"SupportsTranscoding":false,
        "IsRemote":false,"RequiresOpening":false,"RequiresClosing":false,"MediaStreams":streams,
        "DefaultAudioStreamIndex":raw.iter().find(|s|s["codec_type"]=="audio").map(|s|s["index"].clone())})
}
/// Project one media item with server identity, resume ticks and completion state.
pub fn item(item: &MediaItem, server: &str, position: i64, completed: bool) -> Value {
    json!({"Id":item.id,"ServerId":server,"Name":item.title,"Overview":item.plot,"ProductionYear":item.year,
        "Type":if item.kind=="episode" {"Episode"} else {"Movie"},"MediaType":"Video","IsFolder":false,
        "Container":item.container,"VideoType":"VideoFile","LocationType":"FileSystem",
        "RunTimeTicks":item.runtime_seconds.map(|n|(n*10_000_000.0) as i64),"SeriesName":item.series_name,
        "ParentIndexNumber":item.season,"IndexNumber":item.episode,"MediaSources":[source(item)],
        "ImageTags":if item.poster_path.is_some() {json!({"Primary":item.id})} else {json!({})},
        "ParentId":item.series_name.as_ref().map(|n|super::navigation::season_id(n,item.season.unwrap_or(0))).unwrap_or_else(||super::navigation::MOVIES.into()),
        "SeriesId":item.series_name.as_ref().map(|n|super::navigation::series_id(n)),
        "SeasonId":item.series_name.as_ref().map(|n|super::navigation::season_id(n,item.season.unwrap_or(0))),
        "BackdropImageTags":if item.backdrop_path.is_some() {json!([item.id])}else{json!([])},"UserData":{"PlaybackPositionTicks":position,"PlayCount":i32::from(completed),"IsFavorite":false,"Played":completed,"Key":item.id}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hdr_labels_require_transfer_metadata() {
        assert_eq!(video_range(&json!({"pix_fmt":"yuv420p10le"})), "SDR");
        assert_eq!(video_range(&json!({"color_transfer":"smpte2084"})), "HDR10");
        assert_eq!(
            video_range(&json!({"color_transfer":"arib-std-b67"})),
            "HLG"
        );
    }
    #[test]
    fn wire_names_and_user_policy_match_jellyfin() {
        let response = serde_json::to_value(ItemsResponse {
            items: vec![],
            total_record_count: 17,
            start_index: 10,
        })
        .unwrap();
        assert_eq!(
            response,
            json!({"Items":[],"TotalRecordCount":17,"StartIndex":10})
        );
        let policy = user(
            &User {
                id: "user".into(),
                username: "admin".into(),
            },
            "server",
        );
        assert_eq!(policy["Policy"]["EnableVideoPlaybackTranscoding"], true);
        assert_eq!(policy["ServerId"], "server");
    }
}
