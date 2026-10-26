export interface Genre {
  id: number;
  name: string;
}
export interface Title {
  id: string;
  title: string;
  kind: "movie" | "show" | "episode";
  is_anime?: boolean;
  rating?: number | null;
  quality?: string[];
  overview?: string;
  plot?: string;
  year?: number | null;
  runtime_seconds?: number | null;
  poster_url?: string | null;
  backdrop_url?: string | null;
  still_url?: string | null;
  season_count?: number;
  episode_count?: number;
  genres?: Genre[];
  season_id?: string;
  episode_number?: number | null;
  episode_end?: number | null;
  matched_season?: number | null;
  matched_episode?: number | null;
  episode_label?: string | null;
  season_number?: number | null;
  series_name?: string | null;
  show_id?: string | null;
  show_title?: string | null;
  position_ticks?: number;
  container?: string;
  video_codec?: string | null;
  audio_codec?: string | null;
}
export interface Season {
  id: string;
  show_id: string;
  season_number: number | null;
  title: string;
  overview: string;
  episode_count: number;
  poster_url?: string;
}
export interface Profile {
  id: string;
  name: string;
  is_default: boolean;
}
export interface User {
  id: string;
  username: string;
}
export interface Page<T> {
  items: T[];
  total: number;
}
export interface Stream {
  Index: number;
  Type: "Video" | "Audio" | "Subtitle";
  Codec?: string;
  Language?: string;
  Title?: string;
  Width?: number;
  Height?: number;
  Channels?: number;
  IsDefault?: boolean;
  IsForced?: boolean;
  IsTextSubtitleStream?: boolean;
  VideoRange?: string;
}
export interface Source {
  Container: string;
  MediaStreams: Stream[];
  DefaultAudioStreamIndex?: number;
}
export interface PlaybackInfo {
  MediaSources: Source[];
}
export interface History {
  media_item_id: string;
  position_ticks: number;
  updated_at: number;
}
export const idPath = (id: string) => encodeURIComponent(id);
export const formatTime = (seconds: number) => {
  const s = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return s >= 3600
    ? `${Math.floor(s / 3600)}:${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`
    : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};
export const runtime = (seconds?: number | null) =>
  !seconds || seconds <= 0
    ? ""
    : `${Math.floor(seconds / 3600) ? `${Math.floor(seconds / 3600)}h ` : ""}${Math.round((seconds % 3600) / 60)}m`;
export const episodeCode = (
  season?: number | null,
  episode?: number | null,
  label?: string | null,
) =>
  [
    season != null ? `S${String(season).padStart(2, "0")}` : "",
    label
      ? `E${label}`
      : episode != null
        ? `E${String(episode).padStart(2, "0")}`
        : "",
  ].join("");
