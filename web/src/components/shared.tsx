import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { ArrowRight, Film, LoaderCircle, Play, Star, Tv } from "lucide-react";
import { api, aborted, message } from "../api";
import type { Source, Title } from "../types";
import { idPath, runtime } from "../types";

export function Artwork({
  src,
  title = "",
  className = "",
  eager = false,
}: {
  src?: string | null;
  title?: string;
  className?: string;
  eager?: boolean;
}) {
  const [failed, setFailed] = useState(false);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!failed || !src) return;
    const t = setTimeout(() => {
      setRetry((n) => n + 1);
      setFailed(false);
    }, 15000);
    return () => clearTimeout(t);
  }, [failed, src]);
  useEffect(() => setFailed(false), [src]);
  return (
    <div
      className={`artwork ${className} ${!src || failed ? "artwork-empty" : ""}`}
    >
      {src && !failed ? (
        <img
          src={
            retry
              ? `${src}${src.includes("?") ? "&" : "?"}artwork_retry=${retry}`
              : src
          }
          alt=""
          loading={eager ? "eager" : "lazy"}
          decoding="async"
          onError={() => setFailed(true)}
        />
      ) : (
        <div className="artwork-fallback">
          <span className="artwork-preparing">No artwork</span>
          {title && <span>{title}</span>}
        </div>
      )}
    </div>
  );
}
export function Loading({
  label = "Loading your library…",
}: {
  label?: string;
}) {
  return (
    <div className="loading-state" role="status">
      <LoaderCircle size={22} className="spin" />
      {label}
    </div>
  );
}
export function Empty({
  title,
  children,
  action,
}: {
  title: string;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <Film size={32} strokeWidth={1} />
      <h3>{title}</h3>
      <p>{children}</p>
      {action}
    </div>
  );
}
export function Quality({ source }: { source?: Source | null }) {
  const video = source?.MediaStreams.find((stream) => stream.Type === "Video");
  return (
    <>
      {video?.Width && video.Width >= 3840 ? (
        <span className="quality-badge">4K UHD</span>
      ) : video?.Height && video.Height >= 1080 ? (
        <span className="quality-badge">HD</span>
      ) : null}
      {video?.VideoRange &&
        ["HDR10", "HLG", "HDR", "DolbyVision"].includes(video.VideoRange) && (
          <span className="quality-badge">
            {video.VideoRange === "DolbyVision"
              ? "DOLBY VISION"
              : video.VideoRange}
          </span>
        )}
    </>
  );
}
export function useSource(id?: string) {
  const [source, setSource] = useState<Source | null>(null);
  useEffect(() => {
    setSource(null);
    if (!id) return;
    const controller = new AbortController();
    api<{ MediaSources: Source[] }>(`/Items/${idPath(id)}/PlaybackInfo`, {
      signal: controller.signal,
    })
      .then((data) => setSource(data.MediaSources[0] || null))
      .catch(() => {});
    return () => controller.abort();
  }, [id]);
  return source;
}
export function Meta({
  item,
  source,
}: {
  item: Title;
  source?: Source | null;
}) {
  return (
    <div className="metadata">
      <span>{item.year || ""}</span>
      <span>
        {item.kind === "show"
          ? `${item.season_count ?? 0} seasons · ${item.episode_count ?? 0} episodes`
          : runtime(item.runtime_seconds)}
      </span>
      <Quality source={source} />
    </div>
  );
}
export function Genres({ item }: { item: Title }) {
  return (
    <div className="genres">
      <span>
        {item.kind === "show"
          ? "SERIES"
          : item.kind === "episode"
            ? "EPISODE"
            : "FILM"}
      </span>
      {item.genres?.map((genre) => (
        <span key={genre.id}>{genre.name}</span>
      ))}
    </div>
  );
}
export function TitleCard({
  item,
  onOpen,
}: {
  item: Title;
  onOpen: (item: Title) => void;
}) {
  return (
    <button
      className="title-card group"
      data-title-id={item.id}
      onClick={() => onOpen(item)}
      aria-label={`Details for ${item.title}`}
    >
      <div className="poster-wrap">
        <Artwork
          src={
            item.poster_url ||
            (item.kind === "show"
              ? `/api/v1/shows/${idPath(item.id)}/images/poster`
              : `/api/v1/items/${idPath(item.id)}/poster`)
          }
          title={item.title}
        />
        {item.rating != null && (
          <span className="rating-badge">
            <Star size={10} className="inline-block" aria-hidden="true" />{" "}
            {item.rating.toFixed(1)}
          </span>
        )}
        {item.quality?.length ? (
          <div className="poster-quality">
            {item.quality.slice(0, 2).map((q) => (
              <span key={q} className="quality-badge">
                {q}
              </span>
            ))}
          </div>
        ) : null}
        <span className="poster-play">
          <Play size={21} fill="currentColor" />
        </span>
        {item.kind === "show" && (
          <span className="poster-type">
            <Tv size={13} /> SERIES
          </span>
        )}
      </div>
      <h3>{item.title}</h3>
      <p>
        {item.kind === "show"
          ? `${item.season_count ?? 0} ${(item.season_count ?? 0) === 1 ? "season" : "seasons"}`
          : [item.year, runtime(item.runtime_seconds)]
              .filter(Boolean)
              .join(" · ")}
      </p>
    </button>
  );
}
export function SectionHeading({
  title,
  count,
  children,
}: {
  eyebrow?: string;
  title: string;
  count?: string;
  children?: ReactNode;
}) {
  return (
    <div className="section-heading">
      <div>
        <h2>
          {title}
          {count && <span className="heading-count">{count}</span>}
        </h2>
      </div>
      {children}
    </div>
  );
}
export function Action({
  onClick,
  children,
  secondary = false,
  disabled = false,
}: {
  onClick: () => void;
  children: ReactNode;
  secondary?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      className={secondary ? "secondary-button" : "primary-button"}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
      {secondary && <ArrowRight size={17} />}
    </button>
  );
}
export const handle = (
  promise: Promise<unknown>,
  onError: (message: string) => void,
) => {
  promise.catch((error) => {
    if (!aborted(error)) onError(message(error));
  });
};
