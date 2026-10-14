import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Search, X, ArrowUpRight } from "lucide-react";
import { api, message } from "../api";
import type { Title } from "../types";
import { Artwork, TitleCard, SectionHeading } from "./shared";
export interface Hub {
  recently_added: Title[];
  top_rated: Title[];
  trending_shows: Title[];
  continue_watching: Title[];
  because_you_watched: Title[];
  because_title: string | null;
  collections: { id: string; title: string; items: Title[] }[];
}
export function Shelf({
  title,
  eyebrow,
  items,
  onOpen,
}: {
  title: string;
  eyebrow?: string;
  items: Title[];
  onOpen: (t: Title) => void;
}) {
  if (!items.length) return null;
  return (
    <section className="curated-section content-width">
      <SectionHeading title={title} eyebrow={eyebrow} />
      <div className="cinematic-shelf">
        {items.map((t) => (
          <TitleCard key={t.id} item={t} onOpen={onOpen} />
        ))}
      </div>
    </section>
  );
}
export function Curated({
  hub,
  onOpen,
}: {
  hub: Hub | null;
  onOpen: (t: Title) => void;
}) {
  if (!hub) return null;
  return (
    <div className="discovery-shelves">
      <Shelf
        title="Just arrived"
        eyebrow="NEW WORLDS AWAIT"
        items={hub.recently_added}
        onOpen={onOpen}
      />
      <Shelf
        title="Exceptional stories"
        eyebrow="THE HIGHEST RATED IN YOUR LIBRARY"
        items={hub.top_rated}
        onOpen={onOpen}
      />
      <Shelf
        title="Your next obsession"
        eyebrow="SERIES TO GET LOST IN"
        items={hub.trending_shows}
        onOpen={onOpen}
      />
      {hub.because_title && (
        <Shelf
          title={`Because you watched ${hub.because_title}`}
          items={hub.because_you_watched}
          onOpen={onOpen}
        />
      )}{" "}
      {hub.collections.map((c) => (
        <Shelf
          key={c.id}
          title={c.title}
          eyebrow="THE COMPLETE EXPERIENCE"
          items={c.items}
          onOpen={onOpen}
        />
      ))}
    </div>
  );
}
interface Results {
  titles: Title[];
  people: { name: string }[];
  franchises: { id: string; title: string }[];
}
export function CommandPalette({
  open,
  onClose,
  onOpen,
  hub,
}: {
  open: boolean;
  onClose: () => void;
  onOpen: (t: Title) => void;
  hub: Hub | null;
}) {
  const [q, setQ] = useState(""),
    [r, setR] = useState<Results>({ titles: [], people: [], franchises: [] }),
    [index, setIndex] = useState(0),
    [error, setError] = useState(""),
    [person, setPerson] = useState<{
      name: string;
      bio: string;
      items: Title[];
    } | null>(null);
  useEffect(() => {
    setIndex(0);
    if (!q.trim()) {
      setR({ titles: [], people: [], franchises: [] });
      return;
    }
    const a = new AbortController();
    api<Results>(`/api/v1/search?q=${encodeURIComponent(q)}`, {
      signal: a.signal,
    })
      .then((v) => {
        if (a.signal.aborted) return;
        setR(v);
        setError("");
      })
      .catch((e) => {
        if (!a.signal.aborted) setError(message(e));
      });
    return () => a.abort();
  }, [q]);
  useEffect(() => {
    if (!open) {
      setQ("");
      setPerson(null);
      setError("");
    }
  }, [open]);
  const actions = [
    ...r.titles.map((t) => ({
      label: t.title,
      group: "Titles",
      t,
      run: () => {
        onOpen(t);
        onClose();
      },
    })),
    ...r.franchises.map((c) => ({
      label: c.title,
      group: "Collections",
      t: hub?.collections.find((x) => x.id === c.id)?.items[0],
      run: () => {
        setPerson({
          name: c.title,
          bio: "Every story in this collection.",
          items: hub?.collections.find((x) => x.id === c.id)?.items || [],
        });
      },
    })),
    ...r.people.map((p) => ({
      label: p.name,
      group: "Cast & Directors",
      t: undefined,
      run: () => {
        api<{ name: string; bio: string; items: Title[] }>(
          `/api/v1/people/${encodeURIComponent(p.name)}`,
        )
          .then(setPerson)
          .catch((e) => setError(message(e)));
      },
    })),
  ];
  return (
    <Dialog.Root
      open={open}
      onOpenChange={(v) => {
        if (!v) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay spotlight-overlay" />
        <Dialog.Content
          className="spotlight glass"
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              setIndex((n) =>
                Math.max(
                  0,
                  Math.min(
                    actions.length - 1,
                    n + (e.key === "ArrowDown" ? 1 : -1),
                  ),
                ),
              );
            }
            if (e.key === "Enter" && !person) {
              e.preventDefault();
              actions[index]?.run();
            }
          }}
        >
          <Dialog.Title className="sr-only">Find your next story</Dialog.Title>
          <Dialog.Description className="sr-only">
            Search titles, collections, actors and directors. Use arrow keys to
            choose and Enter to open.
          </Dialog.Description>
          <div className="spotlight-input">
            <Search size={22} />
            <input
              aria-label="Search titles, people and collections"
              placeholder="A title. A face. A whole new world."
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                setPerson(null);
              }}
              aria-controls="spotlight-results"
              aria-activedescendant={
                actions[index] ? `result-${index}` : undefined
              }
            />
            <Dialog.Close aria-label="Close search" className="icon-button">
              <X size={18} />
            </Dialog.Close>
          </div>
          <div className="spotlight-results" id="spotlight-results">
            {error && <p role="alert">{error}</p>}
            {person ? (
              <>
                <h2>{person.name}</h2>
                <p>{person.bio || "Explore their work in your library."}</p>
                <div className="person-grid">
                  {person.items.map((t) => (
                    <TitleCard
                      key={t.id}
                      item={t}
                      onOpen={(t) => {
                        onOpen(t);
                        onClose();
                      }}
                    />
                  ))}
                </div>
              </>
            ) : actions.length ? (
              actions.map((a, n) => (
                <div key={`${a.group}-${a.label}`}>
                  {(n === 0 || actions[n - 1].group !== a.group) && (
                    <h3 className="eyebrow">{a.group}</h3>
                  )}
                  <button
                    id={`result-${n}`}
                    className={`spotlight-result ${index === n ? "selected" : ""}`}
                    onMouseEnter={() => setIndex(n)}
                    onClick={a.run}
                  >
                    {a.t && <Artwork src={a.t.poster_url} />}
                    <span>
                      <strong>{a.label}</strong>
                      <small>{a.t?.year || a.group}</small>
                    </span>
                    <ArrowUpRight size={17} />
                  </button>
                </div>
              ))
            ) : (
              <div className="spotlight-empty">
                <span className="eyebrow">FOLLOW YOUR CURIOSITY</span>
                <p>
                  {q
                    ? "No matches in your library."
                    : "Search your entire cinema, instantly."}
                </p>
              </div>
            )}
          </div>
          <footer>
            ↑ ↓ to explore <span>↵ to open · esc to close</span>
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
