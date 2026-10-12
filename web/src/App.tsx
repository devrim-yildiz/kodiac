import * as inlineMovieSheetDependency0 from "react";
import * as inlineMovieSheetDependency1 from "@radix-ui/react-dialog";
import * as inlineMovieSheetDependency2 from "lucide-react";
import * as inlineMovieSheetDependency3 from "./components/useSpatialPresence";
import * as inlineMovieSheetDependency4 from "./api";
import * as inlineMovieSheetDependency6 from "./types";
import * as inlineMovieSheetDependency7 from "./components/shared";
import ShowPage from "./components/ShowPage";
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

const MovieSheet = (() => {
const useEffect = inlineMovieSheetDependency0.useEffect;
const useState = inlineMovieSheetDependency0.useState;
const Dialog = inlineMovieSheetDependency1;
const AudioLines = inlineMovieSheetDependency2.AudioLines;
const Captions = inlineMovieSheetDependency2.Captions;
const Film = inlineMovieSheetDependency2.Film;
const Play = inlineMovieSheetDependency2.Play;
const X = inlineMovieSheetDependency2.X;
const useSpatialPresence = inlineMovieSheetDependency3.useSpatialPresence;
const api = inlineMovieSheetDependency4.api;
const aborted = inlineMovieSheetDependency4.aborted;
const message = inlineMovieSheetDependency4.message;
type Title = import("./types").Title;
const idPath = inlineMovieSheetDependency6.idPath;
const Artwork = inlineMovieSheetDependency7.Artwork;
const Genres = inlineMovieSheetDependency7.Genres;
const Meta = inlineMovieSheetDependency7.Meta;
const useSource = inlineMovieSheetDependency7.useSource;


function MovieSheet({
  movie,
  open,
  onClose,
  onPlay,
}: {
  movie: Title;
  open: boolean;
  onClose: () => void;
  onPlay: (movie: Title) => void;
}) {
  const spatial = useSpatialPresence(open);
  const [detail, setDetail] = useState(movie);
  const [error, setError] = useState("");
  const source = useSource(movie.id);
  useEffect(() => {
    const controller = new AbortController();
    api<Title>(`/api/v1/movies/${idPath(movie.id)}`, {
      signal: controller.signal,
    })
      .then(setDetail)
      .catch((error) => {
        if (!aborted(error)) setError(message(error));
      });
    return () => controller.abort();
  }, [movie.id]);
  const audio =
    source?.MediaStreams.filter((stream) => stream.Type === "Audio") || [];
  const subtitles =
    source?.MediaStreams.filter((stream) => stream.Type === "Subtitle") || [];
  return (
    <Dialog.Root
      open={spatial.present}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay ref={spatial.overlay} className="dialog-overlay" />
        <Dialog.Content
          ref={spatial.panel}
          className="movie-sheet glass"
          aria-describedby="movie-synopsis"
        >
          <div className="sheet-art">
            <Artwork src={detail.backdrop_url || detail.poster_url} eager />
            <div className="sheet-shade" />
          </div>
          <Dialog.Close
            className="sheet-close icon-button glass"
            aria-label="Close movie details"
          >
            <X size={21} />
          </Dialog.Close>
          <div className="sheet-body">
            <Genres item={detail} />
            <Dialog.Title>{detail.title}</Dialog.Title>
            <Meta item={detail} source={source} />
            <button
              className="primary-button"
              disabled={Boolean(error)}
              onClick={() => onPlay(detail)}
            >
              <Play size={18} fill="currentColor" /> Play / Resume
            </button>
            <Dialog.Description id="movie-synopsis" className="sheet-synopsis">
              {detail.overview || "No synopsis available yet."}
            </Dialog.Description>
            {error && (
              <p className="inline-error" role="alert">
                {error}
              </p>
            )}
            <div className="spec-grid">
              <div>
                <span>
                  <Film size={15} /> VIDEO
                </span>
                <p>
                  {source?.Container?.toUpperCase() || "Source format"}{" "}
                  {source?.MediaStreams.find(
                    (s) => s.Type === "Video",
                  )?.Codec?.toUpperCase()}
                </p>
                <p>
                  {source?.MediaStreams.find((s) => s.Type === "Video")?.Width
                    ? `${source.MediaStreams.find((s) => s.Type === "Video")?.Width} × ${source.MediaStreams.find((s) => s.Type === "Video")?.Height}`
                    : "Details appear when probed"}
                </p>
              </div>
              <div>
                <span>
                  <AudioLines size={15} /> AUDIO
                </span>
                {audio.length ? (
                  audio.map((track) => (
                    <p key={track.Index}>
                      {track.Language || "Unknown language"} ·{" "}
                      {track.Codec?.toUpperCase()}
                      {track.Channels ? ` · ${track.Channels} ch` : ""}
                      {track.IsDefault ? " · Default" : ""}
                    </p>
                  ))
                ) : (
                  <p>No audio metadata</p>
                )}
              </div>
              <div>
                <span>
                  <Captions size={15} /> SUBTITLES
                </span>
                {subtitles.length ? (
                  subtitles.map((track) => (
                    <p key={track.Index}>
                      {track.Language || "Unknown language"} ·{" "}
                      {track.Codec?.toUpperCase()}
                      {track.IsForced ? " · Forced" : ""}
                      {!track.IsTextSubtitleStream ? " · Native player" : ""}
                    </p>
                  ))
                ) : (
                  <p>No subtitle tracks</p>
                )}
              </div>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

return MovieSheet;
})();
