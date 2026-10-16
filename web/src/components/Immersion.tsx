import { useEffect, useRef, useState } from "react";
import { Volume2, VolumeX } from "lucide-react";
import { api } from "../api";
import { usePlayer } from "../store";
import type { Title } from "../types";
export interface Experience {
  trailer_url: string | null;
  trailer_key: string | null;
  chapters: {
    start_time: string;
    end_time: string;
    tags?: { title?: string };
  }[];
  next_episode: Title | null;
  people: { name: string; role: string }[];
}
export function AmbientTrailer({
  id,
  hovered,
}: {
  id: string;
  hovered: boolean;
}) {
  const [x, setX] = useState<Experience | null>(null),
    [ready, setReady] = useState(false),
    [muted, setMuted] = useState(true);
  const video = useRef<HTMLVideoElement>(null);
  const active = usePlayer((s) => !!s.item);
  useEffect(() => {
    setX(null);
    setReady(false);
    const a = new AbortController();
    api<Experience>(`/api/v1/items/${encodeURIComponent(id)}/experience`, {
      signal: a.signal,
    })
      .then(setX)
      .catch(() => {});
    return () => a.abort();
  }, [id]);
  useEffect(() => {
    if (!hovered || active) {
      setReady(false);
      video.current?.pause();
      return;
    }
    const timer = setTimeout(() => setReady(true), 2000);
    return () => clearTimeout(timer);
  }, [hovered, active, id]);
  useEffect(() => {
    if (ready && video.current)
      void video.current.play().catch(() => setReady(false));
  }, [ready]);
  if (!x?.trailer_url && !x?.trailer_key) return null;
  return (
    <>
      {x.trailer_url ? (
        <video
          ref={video}
          className={`ambient-trailer ${ready ? "is-playing" : ""}`}
          src={ready ? x.trailer_url : undefined}
          muted={muted}
          playsInline
          loop
          preload="none"
          aria-label="Trailer preview"
        />
      ) : (
        ready && (
          <iframe
            className="ambient-trailer is-playing"
            title="Trailer preview"
            allow="autoplay; encrypted-media"
            src={`https://www.youtube-nocookie.com/embed/${encodeURIComponent(x.trailer_key!)}?autoplay=1&mute=${muted ? 1 : 0}&controls=0&playsinline=1&rel=0`}
          />
        )
      )}{" "}
      {ready && (
        <button
          className="preview-toggle glass-button"
          onClick={() => setMuted((v) => !v)}
          aria-label={muted ? "Unmute trailer" : "Mute trailer"}
        >
          {muted ? <VolumeX size={17} /> : <Volume2 size={17} />}
        </button>
      )}
    </>
  );
}
export function ThemeMusic({
  showId,
  title,
}: {
  showId: string;
  title: string;
}) {
  const audio = useRef<HTMLAudioElement>(null);
  const [available, setAvailable] = useState(true),
    [muted, setMuted] = useState(false),
    [blocked, setBlocked] = useState(false);
  const active = usePlayer((s) => !!s.item);
  useEffect(() => {
    const a = audio.current;
    if (!a || active) return;
    a.volume = 0;
    void a.play().catch(() => setBlocked(true));
    const timer = setInterval(() => {
      a.volume = Math.min(0.12, a.volume + 0.008);
    }, 100);
    return () => {
      clearInterval(timer);
      a.pause();
      a.volume = 0;
    };
  }, [showId, active]);
  return (
    <>
      <audio
        ref={audio}
        src={`/api/v1/shows/${encodeURIComponent(showId)}/theme`}
        loop
        muted={muted || active}
        onError={() => setAvailable(false)}
      />
      {available && !active && (
        <div className="theme-bar glass">
          <Volume2 size={14} />
          <span>{title} · Theme</span>
          <button
            onClick={() => {
              if (blocked) {
                void audio.current
                  ?.play()
                  .then(() => setBlocked(false))
                  .catch(() => {});
                return;
              }
              setMuted((v) => !v);
            }}
            aria-label={
              blocked ? "Play theme" : muted ? "Unmute theme" : "Mute theme"
            }
          >
            {blocked ? "Play theme" : muted ? "Unmute" : "Mute"}
          </button>
        </div>
      )}
    </>
  );
}
