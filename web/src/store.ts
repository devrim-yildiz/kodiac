import { create } from "zustand";
import type { User, Profile, Title, Source } from "./types";
import { api } from "./api";
import { idPath } from "./types";

interface Session {
  user: User | null;
  profiles: Profile[];
  profileId: string;
  ready: boolean;
  set: (values: Partial<Omit<Session, "set">>) => void;
}
export const useSession = create<Session>((set) => ({
  user: null,
  profiles: [],
  profileId: "",
  ready: false,
  set,
}));

function savedVolume() {
  try {
    const value = Number(localStorage.getItem("media-volume") ?? 0.8);
    return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0.8;
  } catch {
    return 0.8;
  }
}
export type StreamMode = "direct" | "remux" | "transcode";
interface PlayerState {
  item: Title | null;
  source: Source | null;
  profileId: string;
  expanded: boolean;
  playing: boolean;
  position: number;
  duration: number;
  volume: number;
  muted: boolean;
  loading: boolean;
  error: string;
  audioIndex: number | null;
  subtitleIndex: number | null;
  mode: StreamMode;
  nonce: number;
  start: number;
  patch: (
    values: Partial<Omit<PlayerState, "patch" | "open" | "close">>,
  ) => void;
  open: (item: Title, source: Source, profileId: string, start: number) => void;
  close: () => void;
}
export const usePlayer = create<PlayerState>((set, get) => ({
  item: null,
  source: null,
  profileId: "",
  expanded: false,
  playing: false,
  position: 0,
  duration: 0,
  volume: savedVolume(),
  muted: false,
  loading: false,
  error: "",
  audioIndex: null,
  subtitleIndex: null,
  mode: "direct",
  nonce: 0,
  start: 0,
  patch: set,
  open: (item, source, profileId, start) =>
    set({
      item,
      source,
      profileId,
      start,
      position: start,
      duration: item.runtime_seconds || 0,
      expanded: true,
      playing: false,
      loading: true,
      error: "",
      mode: "direct",
      audioIndex: null,
      subtitleIndex: null,
      nonce: get().nonce + 1,
    }),
  close: () =>
    set({
      item: null,
      source: null,
      expanded: false,
      playing: false,
      loading: false,
      error: "",
      position: 0,
    }),
}));
// High-frequency position changes never write to localStorage.
usePlayer.subscribe((state, before) => {
  if (state.volume !== before.volume) {
    try {
      localStorage.setItem("media-volume", String(state.volume));
    } catch {
      /* storage can be disabled */
    }
  }
});
let opening = 0;
export function cancelOpening() {
  opening++;
}
export async function playTitle(title: Title, resume?: number) {
  const generation = ++opening;
  const profileId = useSession.getState().profileId;
  const [item, info, progress, defaults] = await Promise.all([
    api<Title>(`/api/v1/items/${idPath(title.id)}`),
    api<{ MediaSources: Source[] }>(`/Items/${idPath(title.id)}/PlaybackInfo`),
    resume == null
      ? api<{ items: { media_item_id: string; position_ticks: number }[] }>(
          `/api/v1/playback?profile_id=${idPath(profileId)}&limit=200`,
        )
      : Promise.resolve(null),
    api<{ audio_language_preference: string; subtitle_mode: string }>(
      "/api/v1/playback-defaults",
    ).catch(() => ({ audio_language_preference: "eng", subtitle_mode: "off" })),
  ]);
  if (generation !== opening || profileId !== useSession.getState().profileId)
    return;
  const source = info.MediaSources[0];
  if (!source) throw new Error("No playable media source is available.");
  const position =
    resume ??
    (progress?.items.find((row) => row.media_item_id === title.id)
      ?.position_ticks || 0) / 10_000_000;
  const start =
    item.runtime_seconds && position >= item.runtime_seconds * 0.95
      ? 0
      : position;
  usePlayer
    .getState()
    .open(
      { ...title, ...item, overview: title.overview || item.plot },
      source,
      profileId,
      start,
    );
  const tracks = source.MediaStreams.filter((s) => s.Type === "Audio");
  const preferred = tracks.findIndex(
    (s) =>
      s.Language === defaults.audio_language_preference ||
      (defaults.audio_language_preference === "deu" && s.Language === "ger"),
  );
  const sub = source.MediaStreams.find(
    (s) =>
      s.Type === "Subtitle" &&
      s.IsTextSubtitleStream &&
      (defaults.subtitle_mode === "always" ||
        (defaults.subtitle_mode === "forced" && s.IsForced)),
  );
  usePlayer.getState().patch({
    audioIndex:
      preferred >= 0 &&
      preferred !==
        Math.max(
          0,
          tracks.findIndex(
            (s) => s.Index === source.DefaultAudioStreamIndex || s.IsDefault,
          ),
        )
        ? preferred
        : null,
    subtitleIndex: sub?.Index ?? null,
  });
}
