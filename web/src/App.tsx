import * as inlineShowPageDependency0 from "./components/Immersion";
import * as inlineShowPageDependency1 from "react";
import * as inlineShowPageDependency2 from "@radix-ui/react-tabs";
import * as inlineShowPageDependency3 from "lucide-react";
import * as inlineShowPageDependency4 from "./api";
import * as inlineShowPageDependency6 from "./types";
import * as inlineShowPageDependency7 from "./components/shared";
import { useEffect, useState } from "react";
import type { Title } from "./types";
import { api } from "./api";

export default function App() {
  const [items, setItems] = useState<Title[]>([]);
  const [selected, setSelected] = useState<Title | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    const abort = new AbortController();
    api<{items: Title[]}>("/api/v1/items", {signal: abort.signal})
      .then(page => setItems(page.items))
      .catch(error => { if (!abort.signal.aborted) setError(String(error)); });
    return () => abort.abort();
  }, []);
  const filtered = items.filter(item => item.title.toLowerCase().includes(query.toLowerCase()));
  const visible = filtered.slice(0, 100);
  return <main className="catalog">
    <header><h1>Medienserver</h1><label>Search<input value={query} onChange={e => setQuery(e.target.value)} /></label></header>
    {error && <p role="alert">{error}</p>}
    <section className="catalog-grid">{visible.map(item => <button key={item.id} onClick={() => setSelected(item)}>
      <img src={item.poster_url || ""} alt="" /><h2>{item.title}</h2><p>{item.year}</p>
    </button>)}</section>
    {selected && <section className="prototype-player"><button onClick={() => setSelected(null)}>Close</button>
      <h2>{selected.title}</h2><button onClick={() => setPaused(!paused)}>{paused ? "Resume" : "Pause"}</button><video data-paused={paused} key={selected.id} controls autoPlay src={`/api/v1/items/${selected.id}/stream`} />
    </section>}
  </main>;
}

const ShowPage = (() => {
const ThemeMusic = inlineShowPageDependency0.ThemeMusic;
const useEffect = inlineShowPageDependency1.useEffect;
const useState = inlineShowPageDependency1.useState;
const Tabs = inlineShowPageDependency2;
const ArrowLeft = inlineShowPageDependency3.ArrowLeft;
const Play = inlineShowPageDependency3.Play;
const allPages = inlineShowPageDependency4.allPages;
const aborted = inlineShowPageDependency4.aborted;
const message = inlineShowPageDependency4.message;
type Season = import("./types").Season;
type Title = import("./types").Title;
const episodeCode = inlineShowPageDependency6.episodeCode;
const idPath = inlineShowPageDependency6.idPath;
const runtime = inlineShowPageDependency6.runtime;
const Artwork = inlineShowPageDependency7.Artwork;
const Empty = inlineShowPageDependency7.Empty;
const Genres = inlineShowPageDependency7.Genres;
const Loading = inlineShowPageDependency7.Loading;
const Meta = inlineShowPageDependency7.Meta;
const SectionHeading = inlineShowPageDependency7.SectionHeading;


function ShowPage({
  show,
  onBack,
  onPlay,
}: {
  show: Title;
  onBack: () => void;
  onPlay: (item: Title) => void;
}) {
  const [seasons, setSeasons] = useState<Season[]>([]);
  const [selected, setSelected] = useState("");
  const [episodes, setEpisodes] = useState<Title[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    allPages<Season>(
      `/api/v1/shows/${idPath(show.id)}/seasons`,
      controller.signal,
    )
      .then((items) => {
        setSeasons(items);
        setSelected(
          (items.find((season) => (season.season_number ?? 0) > 0) || items[0])
            ?.id || "",
        );
        if (!items.length) setLoading(false);
      })
      .catch((error) => {
        if (!aborted(error)) {
          setError(message(error));
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [show.id, retry]);
  useEffect(() => {
    if (!selected) return;
    const controller = new AbortController();
    setLoading(true);
    setError("");
    setEpisodes([]);
    allPages<Title>(
      `/api/v1/seasons/${idPath(selected)}/episodes`,
      controller.signal,
    )
      .then((items) => {
        setEpisodes(items);
        setLoading(false);
      })
      .catch((error) => {
        if (!aborted(error)) {
          setError(message(error));
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [selected, retry]);
  const season = seasons.find((s) => s.id === selected);
  const play = (item: Title) =>
    onPlay({
      ...item,
      show_id: show.id,
      show_title: show.title,
      season_number: season?.season_number,
    });
  return (
    <main className="show-page">
      <ThemeMusic showId={show.id} title={show.title} />
      <section className="hero show-hero">
        <Artwork
          src={
            show.backdrop_url ||
            show.poster_url ||
            `/api/v1/shows/${idPath(show.id)}/images/backdrop`
          }
          className="hero-art"
          eager
        />
        <div className="hero-shade" />
        <div className="hero-content">
          <button className="back-link" onClick={onBack}>
            <ArrowLeft size={17} /> Back to library
          </button>
          <Genres item={show} />
          <h1>{show.title}</h1>
          <Meta item={show} />
          <p className="hero-overview">
            {show.overview || "No series synopsis available yet."}
          </p>
          {episodes.length > 0 && (
            <button
              className="primary-button"
              onClick={() => play(episodes[0])}
            >
              <Play size={18} fill="currentColor" /> Watch / Resume
            </button>
          )}
        </div>
      </section>
      <section className="content-width episode-section">
        <SectionHeading
          eyebrow="ONE MORE EPISODE"
          title="The story continues"
          count={String(show.episode_count || episodes.length)}
        />
        {seasons.length > 0 && (
          <Tabs.Root value={selected} onValueChange={setSelected}>
            <Tabs.List className="season-pills" aria-label="Select season">
              {seasons.map((season) => (
                <Tabs.Trigger
                  key={season.id}
                  className="season-pill"
                  value={season.id}
                >
                  {season.season_number === 0
                    ? "Specials"
                    : season.season_number == null
                      ? "Other episodes"
                      : `Season ${season.season_number}`}
                </Tabs.Trigger>
              ))}
            </Tabs.List>
          </Tabs.Root>
        )}
        {season?.overview && (
          <p className="season-overview">{season.overview}</p>
        )}
        {error && (
          <Empty title="Episodes could not load">
            {error}
            <button
              className="secondary-button"
              onClick={() => setRetry((n) => n + 1)}
            >
              Try again
            </button>
          </Empty>
        )}
        {loading ? (
          <Loading label="Loading episodes…" />
        ) : !error && !episodes.length ? (
          <Empty title="No episodes available">
            This season has no available episode files.
          </Empty>
        ) : (
          <div className="episode-list">
            {episodes.map((item, index) => (
              <article className="episode-row" key={item.id}>
                <span className="episode-ordinal">
                  {String(index + 1).padStart(2, "0")}
                </span>
                <button
                  className="episode-image"
                  onClick={() => play(item)}
                  aria-label={`Play ${item.title}`}
                >
                  <Artwork
                    eager={index < 2}
                    src={
                      item.still_url ||
                      `/api/v1/items/${idPath(item.id)}/poster`
                    }
                    title={item.title}
                  />
                  <span className="resume-play">
                    <Play size={21} fill="currentColor" />
                  </span>
                </button>
                <div className="episode-copy">
                  <div className="episode-topline">
                    <span className="eyebrow">
                      {episodeCode(season?.season_number, item.episode_number)}
                    </span>
                    <span>{runtime(item.runtime_seconds)}</span>
                  </div>
                  <h3>
                    {episodeCode(season?.season_number, item.episode_number)} ·{" "}
                    {item.title}
                  </h3>
                  <p>{item.overview || "No episode synopsis available yet."}</p>
                </div>
                <button
                  className="episode-play icon-button"
                  onClick={() => play(item)}
                  aria-label={`Watch ${item.title}`}
                >
                  <Play size={21} fill="currentColor" />
                </button>
              </article>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}

return ShowPage;
})();
