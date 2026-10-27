import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AudioLines, Captions, Film, Play, X } from "lucide-react";
import { useSpatialPresence } from "./useSpatialPresence";
import { api, aborted, message } from "../api";
import type { Title } from "../types";
import { idPath } from "../types";
import { useSession } from "../store";
import { Artwork, Genres, Meta, useSource } from "./shared";

export default function MovieSheet({
  movie,
  open,
  onClose,
  onPlay,
  collections,
  onCollectionChanged,
}: {
  movie: Title;
  open: boolean;
  onClose: () => void;
  onPlay: (movie: Title) => void;
  collections: { id: string; title: string }[];
  onCollectionChanged: () => void;
}) {
  const admin = useSession((s) => s.user?.is_admin);
  const [collectionName, setCollectionName] = useState("");
  const [saving, setSaving] = useState(false);
  const [collectionStatus, setCollectionStatus] = useState("");
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
            <Dialog.Title>{detail.title}</Dialog.Title>
            <Meta item={detail} source={source} />
            <Genres item={detail} />
            <button
              className="primary-button"
              disabled={Boolean(error)}
              onClick={() => onPlay(detail)}
            >
              <Play size={18} fill="currentColor" />{" "}
              {movie.position_ticks ? "Resume" : "Play"}
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
            {admin && (
              <details className="collection-edit">
                <summary>Manage collection</summary>
                <p>
                  Add this movie to an existing collection, or enter a new
                  collection name.
                </p>
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    setSaving(true);
                    setCollectionStatus("");
                    void api(
                      "/api/v1/movies/" + idPath(movie.id) + "/collection",
                      {
                        method: "POST",
                        body: JSON.stringify({ name: collectionName }),
                      },
                    )
                      .then(() => {
                        setCollectionStatus("Added to collection.");
                        onCollectionChanged();
                      })
                      .catch((e) => setCollectionStatus(message(e)))
                      .finally(() => setSaving(false));
                  }}
                >
                  <label>
                    Collection name
                    <input
                      list="collection-names"
                      value={collectionName}
                      onChange={(e) => setCollectionName(e.target.value)}
                      maxLength={120}
                      required
                      disabled={saving}
                    />
                  </label>
                  <datalist id="collection-names">
                    {collections.map((c) => (
                      <option key={c.id} value={c.title} />
                    ))}
                  </datalist>
                  <button
                    className="secondary-button"
                    disabled={saving || !collectionName.trim()}
                  >
                    {saving ? "Adding…" : "Add to collection"}
                  </button>
                  <p role="status">{collectionStatus}</p>
                </form>
              </details>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
