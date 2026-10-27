import { useEffect, useState } from "react";
import * as Tabs from "@radix-ui/react-tabs";
import { ArrowLeft, Play } from "lucide-react";
import { allPages, aborted, message } from "../api";
import type { Season, Title } from "../types";
import { episodeCode, idPath, runtime } from "../types";
import {
  Artwork,
  Empty,
  Genres,
  Loading,
  Meta,
  SectionHeading,
} from "./shared";

export default function ShowPage({
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
              <Play size={18} fill="currentColor" /> Play episode
            </button>
          )}
        </div>
      </section>
      <section className="content-width episode-section">
        <SectionHeading
          title="Episodes"
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
                      {episodeCode(
                        season?.season_number,
                        item.episode_number,
                        item.episode_label,
                      )}
                    </span>
                    {item.matched_season != null &&
                      season?.season_number == null && (
                        <span>
                          TMDB{" "}
                          {episodeCode(
                            item.matched_season,
                            item.matched_episode,
                          )}
                        </span>
                      )}
                    <span>{runtime(item.runtime_seconds)}</span>
                  </div>
                  <h3>
                    {episodeCode(
                      season?.season_number,
                      item.episode_number,
                      item.episode_label,
                    )}{" "}
                    · {item.title}
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
