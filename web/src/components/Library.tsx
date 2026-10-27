import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useWindowVirtualizer } from "@tanstack/react-virtual";
import { ArrowLeft, ArrowRight, Play } from "lucide-react";
import type { Title } from "../types";
import { formatTime, idPath } from "../types";
import { Artwork, Empty, SectionHeading, TitleCard } from "./shared";

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
      <SectionHeading title="Continue watching">
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
              <p>{item.show_title ? item.title : item.year}</p>
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
