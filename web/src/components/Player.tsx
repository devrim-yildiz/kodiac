import { useEffect, useRef, useState } from "react";
import * as Slider from "@radix-ui/react-slider";
import * as Menu from "@radix-ui/react-dropdown-menu";
import {
  ArrowUpRight,
  Captions,
  Check,
  ChevronLeft,
  ChevronRight,
  LoaderCircle,
  Maximize,
  Minimize2,
  MoreHorizontal,
  Pause,
  PictureInPicture2,
  Play,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { usePlayer, playTitle } from "../store";
import { api } from "../api";
import type { Experience } from "./Immersion";
import { canBuffer, flushProgress, PlaybackEngine } from "../player/engine";
import { formatTime } from "../types";
import type { StreamMode } from "../store";

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
export function Player() {
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
  const [controls, setControls] = useState(true);
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuPane, setMenuPane] = useState("root");
  const [activity, setActivity] = useState(0);
  const [keyboardFocus, setKeyboardFocus] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [buffered, setBuffered] = useState(0);
  const reveal = () => {
    setControls(true);
    setActivity((n) => n + 1);
  };
  const chromeVisible =
    controls ||
    !playing ||
    loading ||
    !!error ||
    menuOpen ||
    scrubbing !== null ||
    keyboardFocus;
  useEffect(() => {
    reveal();
    setScrubbing(null);
    setBuffered(0);
  }, [item?.id, expanded]);
  useEffect(() => {
    if (
      !expanded ||
      !playing ||
      loading ||
      error ||
      menuOpen ||
      scrubbing !== null ||
      keyboardFocus
    )
      return;
    const timer = setTimeout(() => setControls(false), 2800);
    return () => clearTimeout(timer);
  }, [
    activity,
    expanded,
    playing,
    loading,
    error,
    menuOpen,
    scrubbing,
    keyboardFocus,
  ]);
  useEffect(() => {
    const changed = () => setFullscreen(Boolean(document.fullscreenElement));
    document.addEventListener("fullscreenchange", changed);
    return () => document.removeEventListener("fullscreenchange", changed);
  }, []);
  useEffect(() => {
    const current = usePlayer.getState();
    if (!current.item || !current.source || !video.current) return;
    if (current.mode === "direct" && current.audioIndex != null) {
      const audio = current.source.MediaStreams.filter(
        (s) => s.Type === "Audio",
      )[current.audioIndex]?.Codec;
      const mode = canBuffer("remux", current.item, audio)
        ? "remux"
        : "transcode";
      if (canBuffer(mode, current.item, audio)) {
        current.patch({ mode });
        return;
      }
      current.patch({
        audioIndex: null,
        error: "Audio switching requires MediaSource support in this browser.",
      });
      return;
    }
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
      if (expanded) {
        reveal();
        if (event.key === "Tab") setKeyboardFocus(true);
      }
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
        if (
          event.key === " " &&
          (event.target as HTMLElement).closest("button")
        )
          return;
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
  const tracks = source?.MediaStreams || [];
  const changeAudio = (value: string) => {
    const index = Number(value);
    if (index < 0) {
      patch({ audioIndex: null, mode: "direct", error: "" });
      return;
    }
    const audio = tracks.filter((s) => s.Type === "Audio")[index];
    if (!audio || !item) return;
    const mode = canBuffer("remux", item, audio.Codec) ? "remux" : "transcode";
    if (!canBuffer(mode, item, audio.Codec)) {
      patch({ error: "Audio switching is unavailable in this browser." });
      return;
    }
    patch({ audioIndex: index, mode, error: "" });
  };
  const fullscreenToggle = async () => {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else if (surface.current?.requestFullscreen)
        await surface.current.requestFullscreen();
      else {
        const v = video.current as
          (HTMLVideoElement & { webkitEnterFullscreen?: () => void }) | null;
        if (v?.webkitEnterFullscreen) v.webkitEnterFullscreen();
        else throw new Error();
      }
    } catch {
      patch({ error: "Fullscreen is unavailable in this browser." });
    }
  };
  const intro = experience?.chapters?.find((c) =>
    /intro|opening/i.test(c.tags?.title || ""),
  );
  const showIntro =
    intro &&
    position >= Number(intro.start_time) &&
    position < Number(intro.end_time);
  return (
    <section
      ref={surface}
      className={`player ${item ? "is-active" : ""} ${expanded ? "is-expanded" : "is-docked"} ${chromeVisible ? "controls-visible" : "controls-hidden"}`}
      aria-label={expanded ? "Cinema player" : "Mini player"}
      aria-hidden={!item}
      onPointerMove={reveal}
      onPointerDown={() => setKeyboardFocus(false)}
      onKeyDown={(e) => {
        reveal();
        if (e.key === "Tab") setKeyboardFocus(true);
      }}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setKeyboardFocus(false);
      }}
    >
      <div className="video-stage">
        <video
          ref={video}
          id="persistent-video"
          playsInline
          preload="metadata"
          aria-label={item ? `Playing ${item.title}` : "Video player"}
          onClick={() => {
            if (expanded && matchMedia("(pointer: coarse)").matches) {
              setControls((v) => !v);
              setActivity((n) => n + 1);
            } else {
              togglePlay();
              reveal();
            }
          }}
          onDoubleClick={() => {
            if (expanded && !matchMedia("(pointer: coarse)").matches)
              void fullscreenToggle();
          }}
          onProgress={() => {
            const v = video.current;
            if (v) {
              for (let i = 0; i < v.buffered.length; i++)
                if (
                  v.currentTime >= v.buffered.start(i) &&
                  v.currentTime <= v.buffered.end(i)
                ) {
                  setBuffered(v.buffered.end(i));
                  break;
                }
            }
          }}
        />
        {expanded && showIntro && (
          <button
            className="skip-intro glass-button"
            onClick={() => engine.current?.seek(Number(intro.end_time))}
          >
            Skip intro
          </button>
        )}
        {expanded && nearingEnd && experience?.next_episode && !dismissed && (
          <div className="episode-prompt glass" role="status">
            <h3>Next episode in {countdown}s</h3>
            <p>{experience.next_episode.title}</p>
            <button className="primary-button" onClick={() => setCountdown(0)}>
              Play next episode
            </button>
            <button className="text-button" onClick={() => setDismissed(true)}>
              Keep watching
            </button>
          </div>
        )}
        {expanded && loading && (
          <div className="video-loading" role="status">
            <LoaderCircle className="spin" size={28} />
            <span>Loading video…</span>
          </div>
        )}
      </div>
      {item && (
        <>
          {expanded && (
            <header className="cinema-header">
              <button
                className="icon-button"
                onClick={() => void dock()}
                aria-label="Browse while playing"
              >
                <ChevronLeft size={24} />
              </button>
              <div>
                <strong>{item.title}</strong>
                <span>{item.show_title || item.series_name || item.year}</span>
              </div>
              <button
                className="icon-button"
                onClick={() => void dock()}
                aria-label="Dock player"
              >
                <Minimize2 size={22} />
              </button>
            </header>
          )}
          <div className="player-panel">
            {!expanded && (
              <div className="now-playing">
                <strong>{item.title}</strong>
                <span>{item.show_title || item.year}</span>
              </div>
            )}
            <div className="scrubber">
              <Slider.Root
                className="slider"
                min={0}
                max={Math.max(duration, 1)}
                step={0.1}
                value={[scrubbing ?? position]}
                onValueChange={([value]) => {
                  setScrubbing(value);
                  reveal();
                }}
                onValueCommit={([value]) => {
                  engine.current?.seek(value);
                  setScrubbing(null);
                  reveal();
                }}
                disabled={!duration}
              >
                <Slider.Track className="slider-track">
                  <span
                    className="slider-buffered"
                    style={{
                      width: `${duration ? Math.min(100, (buffered / duration) * 100) : 0}%`,
                    }}
                  />
                  <Slider.Range className="slider-range" />
                </Slider.Track>
                <Slider.Thumb
                  className="slider-thumb"
                  aria-label="Playback position"
                  aria-valuetext={`${formatTime(scrubbing ?? position)} of ${formatTime(duration)}`}
                />
              </Slider.Root>
              <div className="timeline-times">
                <span>{formatTime(scrubbing ?? position)}</span>
                <span>
                  −{formatTime(Math.max(0, duration - (scrubbing ?? position)))}
                </span>
              </div>
            </div>
            <div className="transport">
              <button
                className="icon-button skip"
                onClick={() => {
                  engine.current?.seek(position - 10);
                  reveal();
                }}
                aria-label="Back 10 seconds"
              >
                <Skip direction="back" />
              </button>
              <button
                className="play-toggle"
                onClick={() => {
                  togglePlay();
                  reveal();
                }}
                aria-label={playing ? "Pause" : "Play"}
              >
                {playing ? (
                  <Pause size={28} fill="currentColor" />
                ) : (
                  <Play size={28} fill="currentColor" />
                )}
              </button>
              <button
                className="icon-button skip"
                onClick={() => {
                  engine.current?.seek(position + 10);
                  reveal();
                }}
                aria-label="Forward 10 seconds"
              >
                <Skip direction="forward" />
              </button>
            </div>
            <div className="volume-control">
              <button
                className="icon-button"
                onClick={() => patch({ muted: !muted })}
                aria-label={muted ? "Unmute" : "Mute"}
              >
                {muted || volume === 0 ? (
                  <VolumeX size={21} />
                ) : (
                  <Volume2 size={21} />
                )}
              </button>
              <Slider.Root
                className="slider volume-slider"
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
            <div className="player-actions">
              <Menu.Root
                open={menuOpen}
                onOpenChange={(v) => {
                  setMenuOpen(v);
                  setMenuPane("root");
                  reveal();
                }}
              >
                <Menu.Trigger asChild>
                  <button
                    className="icon-button"
                    aria-label="Playback settings"
                  >
                    <MoreHorizontal size={25} />
                  </button>
                </Menu.Trigger>
                <Menu.Portal container={expanded ? surface.current : undefined}>
                  <Menu.Content
                    className="menu playback-menu"
                    side="top"
                    sideOffset={12}
                    align="end"
                  >
                    {menuPane === "root" ? (
                      <>
                        <Menu.Item
                          className="menu-item"
                          onSelect={(e) => {
                            e.preventDefault();
                            setMenuPane("tracks");
                          }}
                        >
                          <Captions size={18} /> Audio & subtitles
                          <ChevronRight size={16} />
                        </Menu.Item>
                        <Menu.Item
                          className="menu-item"
                          onSelect={(e) => {
                            e.preventDefault();
                            setMenuPane("mode");
                          }}
                        >
                          Playback mode
                          <ChevronRight size={16} />
                        </Menu.Item>
                      </>
                    ) : (
                      <>
                        <Menu.Item
                          className="menu-item"
                          onSelect={(e) => {
                            e.preventDefault();
                            setMenuPane("root");
                          }}
                        >
                          <ChevronLeft size={17} /> Back
                        </Menu.Item>
                        <Menu.Separator className="menu-separator" />
                        {menuPane === "tracks" ? (
                          <>
                            {" "}
                            <Menu.Label className="menu-label">
                              Audio
                            </Menu.Label>
                            <Menu.RadioGroup
                              value={String(state.audioIndex ?? -1)}
                              onValueChange={changeAudio}
                            >
                              <Menu.RadioItem className="menu-item" value="-1">
                                Default audio
                                <Menu.ItemIndicator>
                                  <Check size={16} />
                                </Menu.ItemIndicator>
                              </Menu.RadioItem>
                              {tracks
                                .filter((s) => s.Type === "Audio")
                                .map((t, index) => (
                                  <Menu.RadioItem
                                    key={t.Index}
                                    value={String(index)}
                                    className="menu-item"
                                  >
                                    {language(t.Language)}
                                    {t.Channels ? ` · ${t.Channels} ch` : ""}
                                    <Menu.ItemIndicator>
                                      <Check size={16} />
                                    </Menu.ItemIndicator>
                                  </Menu.RadioItem>
                                ))}
                            </Menu.RadioGroup>
                            <Menu.Separator className="menu-separator" />
                            <Menu.Label className="menu-label">
                              Subtitles
                            </Menu.Label>
                            <Menu.RadioGroup
                              value={String(state.subtitleIndex ?? -1)}
                              onValueChange={(v) =>
                                patch({
                                  subtitleIndex:
                                    Number(v) === -1 ? null : Number(v),
                                })
                              }
                            >
                              <Menu.RadioItem className="menu-item" value="-1">
                                Off
                                <Menu.ItemIndicator>
                                  <Check size={16} />
                                </Menu.ItemIndicator>
                              </Menu.RadioItem>
                              {tracks
                                .filter((s) => s.Type === "Subtitle")
                                .map((t) => (
                                  <Menu.RadioItem
                                    key={t.Index}
                                    className="menu-item"
                                    value={String(t.Index)}
                                    disabled={!t.IsTextSubtitleStream}
                                  >
                                    {language(t.Language)}
                                    {t.IsForced ? " · Forced" : ""}
                                    {!t.IsTextSubtitleStream
                                      ? " · Native app only"
                                      : ""}
                                    <Menu.ItemIndicator>
                                      <Check size={16} />
                                    </Menu.ItemIndicator>
                                  </Menu.RadioItem>
                                ))}
                            </Menu.RadioGroup>
                          </>
                        ) : (
                          <>
                            {" "}
                            <Menu.RadioGroup
                              value={state.mode}
                              onValueChange={(v) => settings(v as StreamMode)}
                            >
                              {(["direct", "remux", "transcode"] as const).map(
                                (mode) => (
                                  <Menu.RadioItem
                                    key={mode}
                                    className="menu-item"
                                    value={mode}
                                    disabled={
                                      mode !== "direct" &&
                                      !canBuffer(
                                        mode,
                                        item,
                                        tracks.filter(
                                          (s) => s.Type === "Audio",
                                        )[state.audioIndex ?? 0]?.Codec,
                                      )
                                    }
                                  >
                                    {mode === "direct"
                                      ? "Direct play"
                                      : mode === "remux"
                                        ? "Remux · original quality"
                                        : "Transcode · H.264"}
                                    <Menu.ItemIndicator>
                                      <Check size={16} />
                                    </Menu.ItemIndicator>
                                  </Menu.RadioItem>
                                ),
                              )}
                            </Menu.RadioGroup>
                          </>
                        )}
                      </>
                    )}
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
                  <PictureInPicture2 size={22} />
                </button>
              )}
              <button
                className="icon-button expand-button"
                aria-label={
                  expanded
                    ? fullscreen
                      ? "Exit fullscreen"
                      : "Enter fullscreen"
                    : "Expand player"
                }
                onClick={() =>
                  expanded ? void fullscreenToggle() : patch({ expanded: true })
                }
              >
                {expanded ? <Maximize size={22} /> : <ArrowUpRight size={22} />}
              </button>
              <button
                className="icon-button close-player"
                aria-label="Close player"
                onClick={() => void close()}
              >
                <X size={22} />
              </button>
            </div>
          </div>
          {error && (
            <div className="player-error" role="alert">
              {error}
              <button
                aria-label="Dismiss playback message"
                onClick={() => patch({ error: "" })}
              >
                <X size={17} />
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
function Skip({ direction }: { direction: "back" | "forward" }) {
  return (
    <span className="skip-icon">
      <svg
        viewBox="0 0 36 36"
        width="36"
        height="36"
        aria-hidden="true"
        style={{
          transform: direction === "forward" ? "scaleX(-1)" : undefined,
        }}
      >
        <path
          d="M7 10a13 13 0 1 1-2 11M7 4v7h7"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      <span>10</span>
    </span>
  );
}
