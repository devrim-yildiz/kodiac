import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useWindowVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowLeft, ArrowRight, Info, Play } from "lucide-react";
import type { Title } from "../types";
import { formatTime, idPath } from "../types";
import { AmbientTrailer } from "./Immersion";
import { usePlayer } from "../store";
import {
  Artwork,
  Empty,
  Genres,
  Meta,
  SectionHeading,
  TitleCard,
  useSource,
} from "./shared";

export function Hero({
  items,
  onOpen,
  onPlay,
}: {
  items: Title[];
  onOpen: (item: Title) => void;
  onPlay: (item: Title) => void;
}) {
  const featured = useMemo(() => {
    const withArt = items.filter((item) => item.backdrop_url);
    return (withArt.length ? withArt : items).slice(0, 5);
  }, [items]);
  const [hovered, setHovered] = useState(false);
  const [index, setIndex] = useState(0);
  const item = featured[index % Math.max(1, featured.length)];
  const source = useSource(item?.kind === "movie" ? item.id : undefined);
  useEffect(() => {
    if (featured.length < 2) return;
    const interval = setInterval(() => {
      if (
        !hovered &&
        !document.hidden &&
        !usePlayer.getState().expanded &&
        !document.querySelector('[role="dialog"]')
      )
        setIndex((n) => n + 1);
    }, 15000);
    return () => clearInterval(interval);
  }, [featured.length, hovered]);
  if (!item)
    return (
      <section className="hero hero-empty">
        <div className="hero-content">
          <span className="eyebrow">THE BEST SEAT IS YOURS</span>
          <h1>
            Your cinema.
            <br />
            <span>At home.</span>
          </h1>
          <p>
            Your movies and series will appear here when your library is
            indexed.
          </p>
        </div>
      </section>
    );
  return (
    <section
      className="hero"
      aria-label="Featured title"
      onMouseEnter={() => setHovered(true)}
      onPointerMove={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setHovered(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setHovered(false);
      }}
    >
      {item.kind !== "show" && (
        <AmbientTrailer id={item.id} hovered={hovered} />
      )}
      <Artwork
        key={item.id}
        src={
          item.backdrop_url ||
          item.poster_url ||
          (item.kind === "show"
            ? `/api/v1/shows/${idPath(item.id)}/images/backdrop`
            : `/api/v1/items/${idPath(item.id)}/backdrop`)
        }
        className="hero-art"
        eager
      />
      <div className="hero-shade" />
      <div className="hero-content">
        <span className="eyebrow hero-kicker">
          <span className="live-dot" /> IN YOUR LIBRARY
        </span>
        <Genres item={item} />
        <h1 key={item.id}>{item.title}</h1>
        <Meta item={item} source={source} />
        <p className="hero-overview">
          {item.overview || "No synopsis available yet."}
        </p>
        <div className="hero-actions">
          <button className="primary-button" onClick={() => onPlay(item)}>
            <Play size={19} fill="currentColor" />
            {item.kind === "show"
              ? "Watch series"
              : item.position_ticks
                ? "Resume"
                : "Play"}
          </button>
          <button
            className="hero-detail glass-button"
            onClick={() => onOpen(item)}
          >
            <Info size={19} /> More info
          </button>
        </div>
      </div>
      <div className="hero-footer">
        <a href="#library">
          <ArrowDown size={14} /> EXPLORE YOUR COLLECTION
        </a>
        <div className="hero-pagination" aria-label="Featured titles">
          {featured.map((title, n) => (
            <button
              key={title.id}
              className={n === index % featured.length ? "active" : ""}
              onClick={() => setIndex(n)}
              aria-label={`Feature ${title.title}`}
              aria-pressed={n === index % featured.length}
            >
              <span />
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}
export function ContinueWatching({
  items,
  onPlay,
}: {
  items: Title[];
  onPlay: (item: Title) => void;
}) {
  const shelf = useRef<HTMLDivElement>(null);
  if (!items.length) return null;
  return (
    <section className="continue-section content-width">
      <SectionHeading
        eyebrow="RIGHT WHERE YOU LEFT OFF"
        title="Continue watching"
      >
        <div className="shelf-arrows">
          <button
            className="icon-button"
            aria-label="Previous continue watching titles"
            onClick={() =>
              shelf.current?.scrollBy({ left: -600, behavior: "auto" })
            }
          >
            <ArrowLeft size={19} />
          </button>
          <button
            className="icon-button"
            aria-label="Next continue watching titles"
            onClick={() =>
              shelf.current?.scrollBy({ left: 600, behavior: "auto" })
            }
          >
            <ArrowRight size={19} />
          </button>
        </div>
      </SectionHeading>
      <div className="continue-shelf" ref={shelf}>
        {items.map((item) => {
          const seconds = (item.position_ticks || 0) / 10_000_000;
          const percent = item.runtime_seconds
            ? Math.min(100, (seconds / item.runtime_seconds) * 100)
            : 0;
          return (
            <button
              className="resume-card"
              key={item.id}
              onClick={() => onPlay(item)}
              aria-label={`Resume ${item.title}`}
            >
              <div className="resume-image">
                <Artwork
                  src={`/api/v1/items/${idPath(item.id)}/${item.kind === "episode" ? "poster" : "backdrop"}`}
                  title={item.title}
                />
                <span className="resume-play">
                  <Play size={21} fill="currentColor" />
                </span>
                <span className="remaining">
                  {item.runtime_seconds
                    ? `${Math.max(1, Math.ceil((item.runtime_seconds - seconds) / 60))} min left`
                    : formatTime(seconds)}
                </span>
                <div
                  className="watch-line"
                  role="progressbar"
                  aria-label={`Progress for ${item.title}`}
                  aria-valuenow={Math.round(percent)}
                  aria-valuemin={0}
                  aria-valuemax={100}
                >
                  <span style={{ width: `${percent}%` }} />
                </div>
              </div>
              <h3>{item.show_title || item.title}</h3>
              <p>{item.show_title ? item.title : "Continue film"}</p>
            </button>
          );
        })}
      </div>
    </section>
  );
}
export function VirtualLibrary({
  items,
  onOpen,
}: {
  items: Title[];
  onOpen: (item: Title) => void;
}) {
  const grid = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState({ width: 1200, top: 0 });
  useLayoutEffect(() => {
    const node = grid.current;
    if (!node) return;
    const update = () =>
      setLayout({
        width: node.clientWidth,
        top: node.getBoundingClientRect().top + window.scrollY,
      });
    const observer = new ResizeObserver(update);
    observer.observe(node);
    if (node.parentElement) observer.observe(node.parentElement);
    const above = document.querySelector(".hero");
    if (above) observer.observe(above);
    window.addEventListener("resize", update);
    update();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
    };
  }, [items.length]);
  const columns = Math.max(
    2,
    Math.floor((layout.width + 22) / (layout.width < 600 ? 154 : 186)),
  );
  const cardWidth = (layout.width - (columns - 1) * 22) / columns;
  const rowHeight = cardWidth * 1.5 + 88;
  const virtual = useWindowVirtualizer({
    count: Math.ceil(items.length / columns),
    estimateSize: () => rowHeight,
    overscan: 3,
    scrollMargin: layout.top,
    getItemKey: (row) => items[row * columns]?.id || row,
  });
  useEffect(() => virtual.measure(), [virtual, rowHeight, columns]);
  return (
    <div
      ref={grid}
      className="virtual-grid"
      data-testid="virtual-grid"
      data-total={items.length}
      style={{ height: virtual.getTotalSize() }}
    >
      {virtual.getVirtualItems().map((row) => (
        <div
          key={row.key}
          className="virtual-row"
          data-index={row.index}
          style={{
            height: rowHeight,
            transform: `translateY(${row.start - layout.top}px)`,
            gridTemplateColumns: `repeat(${columns},minmax(0,1fr))`,
          }}
        >
          {items
            .slice(row.index * columns, (row.index + 1) * columns)
            .map((item) => (
              <TitleCard key={item.id} item={item} onOpen={onOpen} />
            ))}
        </div>
      ))}
      {!items.length && (
        <Empty title="No titles found">
          Try another search, or choose a different library view.
        </Empty>
      )}
    </div>
  );
}
