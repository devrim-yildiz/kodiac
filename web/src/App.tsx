import MovieSheet from "./components/MovieSheet";
import * as inlinePlayerDependency0 from "react";
import * as inlinePlayerDependency1 from "@radix-ui/react-slider";
import * as inlinePlayerDependency2 from "@radix-ui/react-dropdown-menu";
import * as inlinePlayerDependency3 from "lucide-react";
import * as inlinePlayerDependency4 from "./store";
import * as inlinePlayerDependency5 from "./api";
import * as inlinePlayerDependency7 from "./player/engine";
import * as inlinePlayerDependency8 from "./types";
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

const Player = (() => {
const useEffect = inlinePlayerDependency0.useEffect;
const useRef = inlinePlayerDependency0.useRef;
const useState = inlinePlayerDependency0.useState;
const Slider = inlinePlayerDependency1;
const Menu = inlinePlayerDependency2;
const ArrowDownLeft = inlinePlayerDependency3.ArrowDownLeft;
const ArrowUpRight = inlinePlayerDependency3.ArrowUpRight;
const Captions = inlinePlayerDependency3.Captions;
const Check = inlinePlayerDependency3.Check;
const ChevronLeft = inlinePlayerDependency3.ChevronLeft;
const LoaderCircle = inlinePlayerDependency3.LoaderCircle;
const Maximize = inlinePlayerDependency3.Maximize;
const Minimize2 = inlinePlayerDependency3.Minimize2;
const MoreHorizontal = inlinePlayerDependency3.MoreHorizontal;
const Pause = inlinePlayerDependency3.Pause;
const PictureInPicture2 = inlinePlayerDependency3.PictureInPicture2;
const Play = inlinePlayerDependency3.Play;
const RotateCcw = inlinePlayerDependency3.RotateCcw;
const RotateCw = inlinePlayerDependency3.RotateCw;
const Volume2 = inlinePlayerDependency3.Volume2;
const VolumeX = inlinePlayerDependency3.VolumeX;
const X = inlinePlayerDependency3.X;
const usePlayer = inlinePlayerDependency4.usePlayer;
const playTitle = inlinePlayerDependency4.playTitle;
const api = inlinePlayerDependency5.api;
type Experience = import("./components/Immersion").Experience;
const canBuffer = inlinePlayerDependency7.canBuffer;
const flushProgress = inlinePlayerDependency7.flushProgress;
const PlaybackEngine = inlinePlayerDependency7.PlaybackEngine;
type PlaybackEngine = import("./player/engine").PlaybackEngine;
const formatTime = inlinePlayerDependency8.formatTime;
type StreamMode = import("./store").StreamMode;


const language = (value?: string) =>
  ({
    eng: "English",
    en: "English",
    deu: "Deutsch",
    ger: "Deutsch",
    de: "Deutsch",
    fra: "Français",
    fre: "Français",
    spa: "Español",
    jpn: "日本語",
    und: "Unknown language",
  })[value || ""] ||
  value ||
  "Unknown language";
function Player() {
  const state = usePlayer();
  const [experience, setExperience] = useState<Experience | null>(null);
  const [countdown, setCountdown] = useState(10);
  const [dismissed, setDismissed] = useState(false);
  useEffect(() => {
    setExperience(null);
    setCountdown(10);
    setDismissed(false);
    if (!state.item) return;
    const a = new AbortController();
    api<Experience>(
      `/api/v1/items/${encodeURIComponent(state.item.id)}/experience`,
      { signal: a.signal },
    )
      .then(setExperience)
      .catch(() => {});
    return () => a.abort();
  }, [state.item?.id]);
  const nearingEnd =
    state.item?.kind === "episode" &&
    state.duration > 30 &&
    state.position >= state.duration - 30;
  useEffect(() => {
    if (!nearingEnd || !state.playing || dismissed || !experience?.next_episode)
      return;
    const timer = setInterval(
      () => setCountdown((n) => Math.max(0, n - 1)),
      1000,
    );
    return () => clearInterval(timer);
  }, [nearingEnd, state.playing, dismissed, experience?.next_episode]);
  useEffect(() => {
    if (countdown !== 0 || dismissed || !experience?.next_episode) return;
    setDismissed(true);
    window.dispatchEvent(new Event("player-save"));
    void flushProgress()
      .then(() =>
        api("/api/v1/playback", {
          method: "POST",
          body: JSON.stringify({
            item_id: state.item?.id,
            profile_id: state.profileId,
            position_ticks: Math.floor(state.position * 10000000),
            stopped: true,
            completed: true,
          }),
        }),
      )
      .then(() => {
        const current = usePlayer.getState();
        if (
          current.item?.id === state.item?.id &&
          current.profileId === state.profileId
        )
          return playTitle(experience.next_episode!, 0);
      })
      .catch(() =>
        usePlayer.getState().patch({
          error: "Next episode could not start. Choose it from the season.",
        }),
      );
  }, [countdown, dismissed, experience]);

  const {
    item,
    source,
    expanded,
    playing,
    position,
    duration,
    volume,
    muted,
    loading,
    error,
    patch,
  } = state;
  const video = useRef<HTMLVideoElement>(null);
  const engine = useRef<PlaybackEngine | null>(null);
  const surface = useRef<HTMLElement>(null);
  const [scrubbing, setScrubbing] = useState<number | null>(null);
  const [pipAvailable, setPipAvailable] = useState(false);
  useEffect(() => {
    const current = usePlayer.getState();
    if (!current.item || !current.source || !video.current) return;
    const instance = new PlaybackEngine(
      video.current,
      current.item,
      current.source,
      current.profileId,
      current.mode,
      current.audioIndex,
      current.position,
    );
    engine.current = instance;
    const save = () => instance.save(true);
    window.addEventListener("player-save", save);
    return () => {
      window.removeEventListener("player-save", save);
      instance.destroy();
      engine.current = null;
    };
    // Layout and browsing state deliberately do not participate in the source lifetime.
  }, [state.nonce, state.mode, state.audioIndex, item?.id]);
  useEffect(() => {
    engine.current?.subtitle(state.subtitleIndex);
  }, [state.nonce, state.subtitleIndex, state.mode, state.audioIndex]);
  useEffect(() => {
    if (video.current) {
      video.current.volume = Number.isFinite(volume) ? volume : 0.8;
      video.current.muted = muted;
    }
  }, [volume, muted, state.nonce]);
  useEffect(() => {
    setPipAvailable(Boolean(document.pictureInPictureEnabled));
  }, []);
  useEffect(() => {
    if (!expanded) return;
    const before = document.activeElement as HTMLElement | null;
    const shell = document.getElementById("browse-shell");
    if (shell) shell.inert = true;
    const oldOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    surface.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => {
      if (shell) shell.inert = false;
      document.body.style.overflow = oldOverflow;
      if (before?.isConnected) before.focus({ preventScroll: true });
    };
  }, [expanded]);
  const togglePlay = () => {
    if (video.current?.paused)
      video.current
        .play()
        .catch(() => patch({ error: "Press play to start playback." }));
    else video.current?.pause();
  };
  useEffect(() => {
    if (!item) return;
    const keydown = (event: KeyboardEvent) => {
      if (
        (event.target as HTMLElement).closest(
          'input,select,textarea,[role="slider"],[role="menu"],[role="dialog"]',
        ) ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey
      )
        return;
      if (event.key === "Escape" && expanded && !document.fullscreenElement) {
        event.preventDefault();
        patch({ expanded: false });
        return;
      }
      if (!expanded) return;
      if (event.key === " " || event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (video.current?.paused) void video.current.play();
        else video.current?.pause();
      }
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        engine.current?.seek(
          (video.current?.currentTime || 0) +
            (event.key === "ArrowLeft" ? -10 : 10),
        );
      }
      if (event.key.toLowerCase() === "m")
        patch({ muted: !usePlayer.getState().muted });
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [item, expanded, patch]);
  const dock = async () => {
    if (document.fullscreenElement) await document.exitFullscreen();
    engine.current?.save();
    patch({ expanded: false });
    await flushProgress();
    window.dispatchEvent(new Event("history-changed"));
  };
  const close = async () => {
    if (document.pictureInPictureElement)
      await document.exitPictureInPicture().catch(() => {});
    if (document.fullscreenElement) await document.exitFullscreen();
    engine.current?.save(true);
    usePlayer.getState().close();
    await flushProgress();
    window.dispatchEvent(new Event("history-changed"));
  };
  const settings = (mode: StreamMode) => {
    if (!item) return;
    if (
      mode !== "direct" &&
      !canBuffer(
        mode,
        item,
        source?.MediaStreams.filter((s) => s.Type === "Audio")[
          state.audioIndex ?? 0
        ]?.Codec,
      )
    )
      return;
    patch({
      mode,
      error: "",
      start: position,
      ...(mode === "direct" ? { audioIndex: null } : {}),
    });
  };
  return (
    <section
      ref={surface}
      className={`player ${item ? "is-active" : ""} ${expanded ? "is-expanded" : "is-docked"}`}
      aria-label={expanded ? "Cinema player" : "Mini player"}
      aria-hidden={!item}
    >
      <div className="video-stage">
        <video
          ref={video}
          id="persistent-video"
          playsInline
          preload="metadata"
          onClick={togglePlay}
          aria-label={item ? `Playing ${item.title}` : "Video player"}
        />
        {expanded &&
          item?.kind === "episode" &&
          position < 180 &&
          duration - position > 30 &&
          (() => {
            const intro = experience?.chapters?.find((c) =>
              /intro|opening/i.test(c.tags?.title || ""),
            );
            const start = Number(intro?.start_time || 0);
            const end = Number(intro?.end_time || 0);
            if (intro && (position < start || position >= end)) return null;
            return (
              <button
                className="skip-intro glass-button"
                onClick={() =>
                  engine.current?.seek(
                    intro ? end : Math.min(duration, position + 85),
                  )
                }
              >
                {intro ? "Skip intro" : "+85s Skip Intro"}
              </button>
            );
          })()}
        {expanded && nearingEnd && experience?.next_episode && !dismissed && (
          <div className="episode-prompt glass" role="status">
            <span className="eyebrow">KEEP THE STORY GOING</span>
            <h3>Next Episode in {countdown}s…</h3>
            <p>{experience.next_episode.title}</p>
            <button className="primary-button" onClick={() => setCountdown(0)}>
              Play next episode
            </button>
            <button className="cancel-next" onClick={() => setDismissed(true)}>
              Stay with this episode
            </button>
          </div>
        )}
        {expanded && loading && (
          <div className="video-loading" role="status">
            <LoaderCircle className="spin" size={28} />
            <span>Preparing your cinema…</span>
          </div>
        )}
      </div>
      {item && (
        <>
          {expanded && (
            <header className="cinema-header">
              <button
                className="glass-button"
                onClick={() => void dock()}
                aria-label="Browse while playing"
              >
                <ChevronLeft size={18} /> Browse library
              </button>
              <span className="cinema-wordmark">
                KODIAC <span>CINEMA</span>
              </span>
              <button
                className="icon-button glass"
                onClick={() => void dock()}
                aria-label="Dock player"
              >
                <Minimize2 size={20} />
              </button>
            </header>
          )}
          <div className="player-panel glass">
            <div className="now-playing">
              <span className="eyebrow">
                {expanded ? "NOW PLAYING" : "CONTINUE EXPLORING"}
              </span>
              <strong>{item.title}</strong>
              <span>
                {item.series_name ||
                  item.show_title ||
                  item.year ||
                  "Your private cinema"}
              </span>
            </div>
            <div className="transport">
              <button
                className="icon-button skip"
                onClick={() => engine.current?.seek(position - 10)}
                aria-label="Back 10 seconds"
              >
                <RotateCcw size={19} />
                <span>10</span>
              </button>
              <button
                className="play-toggle"
                onClick={togglePlay}
                aria-label={playing ? "Pause" : "Play"}
              >
                {playing ? (
                  <Pause size={21} fill="currentColor" />
                ) : (
                  <Play size={21} fill="currentColor" />
                )}
              </button>
              <button
                className="icon-button skip"
                onClick={() => engine.current?.seek(position + 10)}
                aria-label="Forward 10 seconds"
              >
                <RotateCw size={19} />
                <span>10</span>
              </button>
            </div>
            <div className="scrubber">
              <span>{formatTime(scrubbing ?? position)}</span>
              <Slider.Root
                className="slider"
                aria-label="Playback position"
                min={0}
                max={Math.max(duration, 1)}
                step={0.1}
                value={[scrubbing ?? position]}
                onValueChange={([value]) => setScrubbing(value)}
                onValueCommit={([value]) => {
                  engine.current?.seek(value);
                  setScrubbing(null);
                }}
                disabled={!duration}
              >
                <Slider.Track className="slider-track">
                  <Slider.Range className="slider-range" />
                </Slider.Track>
                <Slider.Thumb
                  className="slider-thumb"
                  aria-label="Playback position"
                  aria-valuetext={`${formatTime(scrubbing ?? position)} of ${formatTime(duration)}`}
                />
              </Slider.Root>
              <span>{formatTime(duration)}</span>
            </div>
            <div className="volume-control">
              <button
                className="icon-button"
                onClick={() => patch({ muted: !muted })}
                aria-label={muted ? "Unmute" : "Mute"}
              >
                {muted || volume === 0 ? (
                  <VolumeX size={19} />
                ) : (
                  <Volume2 size={19} />
                )}
              </button>
              <Slider.Root
                className="slider volume-slider"
                aria-label="Volume"
                min={0}
                max={1}
                step={0.01}
                value={[muted ? 0 : volume]}
                onValueChange={([value]) =>
                  patch({ volume: value, muted: false })
                }
              >
                <Slider.Track className="slider-track">
                  <Slider.Range className="slider-range" />
                </Slider.Track>
                <Slider.Thumb
                  className="slider-thumb"
                  aria-label="Volume"
                  aria-valuetext={`${Math.round((muted ? 0 : volume) * 100)} percent`}
                />
              </Slider.Root>
            </div>
            <Menu.Root>
              <Menu.Trigger asChild>
                <button className="icon-button" aria-label="Playback settings">
                  <MoreHorizontal size={23} />
                </button>
              </Menu.Trigger>
              <Menu.Portal>
                <Menu.Content
                  className="menu glass playback-menu"
                  side="top"
                  sideOffset={15}
                  align="end"
                >
                  <Menu.Label className="menu-label">PLAYBACK</Menu.Label>
                  <Menu.RadioGroup
                    value={state.mode}
                    onValueChange={(value) => settings(value as StreamMode)}
                  >
                    {(["direct", "remux", "transcode"] as const).map((mode) => (
                      <Menu.RadioItem
                        className="menu-item"
                        key={mode}
                        value={mode}
                        disabled={
                          mode !== "direct" &&
                          !canBuffer(
                            mode,
                            item,
                            source?.MediaStreams.filter(
                              (s) => s.Type === "Audio",
                            )[state.audioIndex ?? 0]?.Codec,
                          )
                        }
                      >
                        {mode === "direct"
                          ? "Direct play"
                          : mode === "remux"
                            ? "Remux · Original quality"
                            : "Transcode · H.264"}
                        <Menu.ItemIndicator>
                          <Check size={15} />
                        </Menu.ItemIndicator>
                      </Menu.RadioItem>
                    ))}
                  </Menu.RadioGroup>
                  <Menu.Separator className="menu-separator" />
                  <Menu.Label className="menu-label">AUDIO</Menu.Label>
                  <Menu.RadioGroup
                    value={String(state.audioIndex ?? -1)}
                    onValueChange={(value) => {
                      const index = Number(value);
                      if (index < 0) {
                        patch({ audioIndex: null, mode: "direct", error: "" });
                        return;
                      }
                      const audio = source?.MediaStreams.filter(
                        (s) => s.Type === "Audio",
                      )[index];
                      if (!audio) return;
                      const mode = canBuffer("remux", item, audio.Codec)
                        ? "remux"
                        : "transcode";
                      if (!canBuffer(mode, item, audio.Codec)) {
                        patch({
                          error:
                            "Audio switching requires MediaSource support in this browser.",
                        });
                        return;
                      }
                      patch({ audioIndex: index, mode, error: "" });
                    }}
                  >
                    <Menu.RadioItem className="menu-item" value="-1">
                      Default audio
                      <Menu.ItemIndicator>
                        <Check size={15} />
                      </Menu.ItemIndicator>
                    </Menu.RadioItem>
                    {source?.MediaStreams.filter((s) => s.Type === "Audio").map(
                      (track, index) => (
                        <Menu.RadioItem
                          className="menu-item"
                          key={track.Index}
                          value={String(index)}
                        >
                          {language(track.Language)} ·{" "}
                          {track.Codec?.toUpperCase()}{" "}
                          {track.Channels ? `· ${track.Channels} ch` : ""}
                          <Menu.ItemIndicator>
                            <Check size={15} />
                          </Menu.ItemIndicator>
                        </Menu.RadioItem>
                      ),
                    )}
                  </Menu.RadioGroup>
                  <Menu.Separator className="menu-separator" />
                  <Menu.Label className="menu-label">
                    <Captions size={14} /> SUBTITLES
                  </Menu.Label>
                  <Menu.RadioGroup
                    value={String(state.subtitleIndex ?? -1)}
                    onValueChange={(value) =>
                      patch({
                        subtitleIndex:
                          Number(value) === -1 ? null : Number(value),
                      })
                    }
                  >
                    <Menu.RadioItem className="menu-item" value="-1">
                      Off
                      <Menu.ItemIndicator>
                        <Check size={15} />
                      </Menu.ItemIndicator>
                    </Menu.RadioItem>
                    {source?.MediaStreams.filter(
                      (s) => s.Type === "Subtitle" && s.IsTextSubtitleStream,
                    ).map((track) => (
                      <Menu.RadioItem
                        className="menu-item"
                        key={track.Index}
                        value={String(track.Index)}
                      >
                        {language(track.Language)}
                        {track.IsForced ? " · Forced" : ""}
                        <Menu.ItemIndicator>
                          <Check size={15} />
                        </Menu.ItemIndicator>
                      </Menu.RadioItem>
                    ))}
                  </Menu.RadioGroup>
                </Menu.Content>
              </Menu.Portal>
            </Menu.Root>
            {expanded && pipAvailable && (
              <button
                className="icon-button pip-button"
                aria-label="Picture in picture"
                onClick={() => {
                  const v = video.current;
                  if (v)
                    void (
                      document.pictureInPictureElement
                        ? document.exitPictureInPicture()
                        : v.requestPictureInPicture()
                    ).catch(() =>
                      patch({
                        error:
                          "Picture in picture is unavailable for this video.",
                      }),
                    );
                }}
              >
                <PictureInPicture2 size={20} />
              </button>
            )}
            <button
              className="icon-button expand-button"
              aria-label={expanded ? "Enter fullscreen" : "Expand player"}
              onClick={() =>
                expanded
                  ? void surface.current
                      ?.requestFullscreen()
                      .catch(() =>
                        patch({ error: "Fullscreen is unavailable." }),
                      )
                  : patch({ expanded: true })
              }
            >
              {expanded ? <Maximize size={19} /> : <ArrowUpRight size={21} />}
            </button>
            <button
              className="icon-button close-player"
              aria-label="Close player"
              onClick={() => void close()}
            >
              <X size={19} />
            </button>
            {expanded && (
              <button className="dock-label" onClick={() => void dock()}>
                <ArrowDownLeft size={14} /> Keep watching while you browse
              </button>
            )}
          </div>
          {error && (
            <div className="player-error" role="alert">
              {error}
              <button
                aria-label="Dismiss playback message"
                onClick={() => patch({ error: "" })}
              >
                <X size={15} />
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}

return Player;
})();
