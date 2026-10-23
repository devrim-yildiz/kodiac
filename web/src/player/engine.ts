// Codex, 2026-10-21. The media engine's lifetime follows its source, never the page.
import { api, message } from "../api";
import type { Title, Source } from "../types";
import { idPath } from "../types";
import { usePlayer } from "../store";
import type { StreamMode } from "../store";

let progressQueue: Promise<unknown> = Promise.resolve();
export const flushProgress = () => progressQueue.catch(() => {});
const MediaSourceClass =
  window.MediaSource ||
  (window as unknown as { ManagedMediaSource?: typeof MediaSource })
    .ManagedMediaSource;
const abortError = () => new DOMException("Cancelled", "AbortError");
function sleep(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    if (signal.aborted) return reject(abortError());
    const abort = () => {
      clearTimeout(timer);
      reject(abortError());
    };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, ms);
    signal.addEventListener("abort", abort, { once: true });
  });
}
function operation(
  buffer: SourceBuffer,
  action: () => void,
  signal: AbortSignal,
) {
  return new Promise<void>((resolve, reject) => {
    if (signal.aborted) return reject(abortError());
    const cleanup = () => {
      clearTimeout(timer);
      buffer.removeEventListener("updateend", done);
      buffer.removeEventListener("error", failed);
      signal.removeEventListener("abort", cancelled);
    };
    const done = () => {
      cleanup();
      resolve();
    };
    const failed = () => {
      cleanup();
      reject(new Error("The browser could not decode this stream."));
    };
    const cancelled = () => {
      cleanup();
      reject(abortError());
    };
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error("Media buffer timed out."));
    }, 15_000);
    buffer.addEventListener("updateend", done, { once: true });
    buffer.addEventListener("error", failed, { once: true });
    signal.addEventListener("abort", cancelled, { once: true });
    try {
      action();
    } catch (error) {
      cleanup();
      reject(error);
    }
  });
}
export function mimeFor(mode: StreamMode, item: Title, audioCodec?: string) {
  if (mode === "transcode") return 'video/mp4; codecs="avc1.640028,mp4a.40.2"';
  if (
    mode === "remux" &&
    item.video_codec === "h264" &&
    (!audioCodec || audioCodec === "aac")
  )
    return `video/mp4; codecs="avc1.640028${audioCodec ? ",mp4a.40.2" : ""}"`;
  return "";
}
export const canBuffer = (mode: StreamMode, item: Title, audio?: string) =>
  Boolean(
    MediaSourceClass &&
    mimeFor(mode, item, audio) &&
    MediaSourceClass.isTypeSupported(mimeFor(mode, item, audio)),
  );

export class PlaybackEngine {
  private controller = new AbortController();
  private url?: string;
  private source?: MediaSource;
  private ready = false;
  private initializing = true;
  private target = 0;
  private retry = false;
  private disposed = false;
  private ended = false;
  private pauseTimer?: ReturnType<typeof setTimeout>;
  private lastSave = Date.now();
  private listeners: (() => void)[] = [];
  constructor(
    private video: HTMLVideoElement,
    readonly item: Title,
    readonly info: Source,
    readonly profileId: string,
    readonly mode: StreamMode,
    readonly audioIndex: number | null,
    start: number,
  ) {
    const on = (name: keyof HTMLMediaElementEventMap, callback: () => void) => {
      video.addEventListener(name, callback);
      this.listeners.push(() => video.removeEventListener(name, callback));
    };
    on("loadedmetadata", () => {
      if (this.mode === "direct") {
        if (this.target > 0) video.currentTime = this.target;
        this.initializing = false;
        this.ready = true;
      }
      if (Number.isFinite(video.duration))
        usePlayer.getState().patch({ duration: video.duration });
    });
    on("playing", () =>
      usePlayer.getState().patch({ playing: true, loading: false, error: "" }),
    );
    on("waiting", () => usePlayer.getState().patch({ loading: true }));
    on("timeupdate", () => {
      if (this.ready && !this.initializing)
        usePlayer.getState().patch({ position: video.currentTime });
      if (Date.now() - this.lastSave > 10_000) {
        this.lastSave = Date.now();
        this.save();
      }
    });
    on("pause", () => {
      usePlayer.getState().patch({ playing: false });
      this.save();
      this.releasePaused();
    });
    on("play", () => {
      clearTimeout(this.pauseTimer);
      if (this.retry) this.start(video.currentTime, true);
    });
    on("seeked", () => this.save());
    on("seeking", () => {
      if (
        this.ready &&
        !this.initializing &&
        this.mode !== "direct" &&
        !this.buffered(video.currentTime)
      )
        this.start(video.currentTime, !video.paused);
    });
    on("ended", () => {
      this.ended = true;
      usePlayer.getState().patch({ playing: false });
      this.save(true, true);
      window.dispatchEvent(new Event("history-changed"));
    });
    on("error", () => {
      if (this.disposed) return;
      // Retry format failures once per delivery mode; network errors need a real error.
      if ([3, 4].includes(video.error?.code ?? 0)) {
        const audio =
          this.info.MediaStreams.filter((s) => s.Type === "Audio")[
            this.audioIndex ?? 0
          ]?.Codec ||
          this.item.audio_codec ||
          undefined;
        const next =
          this.mode === "direct" && canBuffer("remux", this.item, audio)
            ? "remux"
            : this.mode !== "transcode" &&
                canBuffer("transcode", this.item, audio)
              ? "transcode"
              : null;
        if (next) {
          usePlayer.getState().patch({ mode: next, error: "" });
          return;
        }
      }
      if (!this.disposed)
        usePlayer.getState().patch({
          loading: false,
          playing: false,
          error:
            "This format could not play. Choose Transcode in playback settings.",
        });
    });
    const visibility = () => {
      if (document.hidden) this.save();
    };
    const pagehide = () => this.save(false, false, true);
    document.addEventListener("visibilitychange", visibility);
    window.addEventListener("pagehide", pagehide);
    this.listeners.push(
      () => document.removeEventListener("visibilitychange", visibility),
      () => window.removeEventListener("pagehide", pagehide),
    );
    this.start(start, true);
  }
  private fail(error: unknown, signal: AbortSignal) {
    if (!signal.aborted && !this.disposed)
      usePlayer.getState().patch({ loading: false, error: message(error) });
  }
  private stopSource() {
    clearTimeout(this.pauseTimer);
    this.controller.abort();
    if (this.url) URL.revokeObjectURL(this.url);
    this.url = undefined;
    this.source = undefined;
  }
  start(seconds: number, autoplay: boolean) {
    this.stopSource();
    this.controller = new AbortController();
    this.retry = false;
    this.ended = false;
    this.initializing = true;
    this.ready = false;
    this.target = Math.min(
      Math.max(0, seconds),
      this.item.runtime_seconds
        ? Math.max(0, this.item.runtime_seconds - 0.1)
        : 604800,
    );
    usePlayer.getState().patch({ loading: true, error: "" });
    const query = new URLSearchParams({
      mode: this.mode,
      start_seconds: String(this.mode === "direct" ? 0 : this.target),
    });
    if (this.audioIndex != null && this.mode !== "direct")
      query.set("audio_index", String(this.audioIndex));
    const url = `/api/v1/items/${idPath(this.item.id)}/stream?${query}`;
    if (this.mode === "direct") this.video.src = url;
    else {
      const audio =
        this.info.MediaStreams.filter((stream) => stream.Type === "Audio")[
          this.audioIndex ?? 0
        ]?.Codec ||
        this.item.audio_codec ||
        undefined;
      if (!canBuffer(this.mode, this.item, audio) || !MediaSourceClass) {
        usePlayer.getState().patch({
          loading: false,
          error:
            "Timeline conversion is unavailable in this browser. Try Direct play.",
        });
        return;
      }
      const source = new MediaSourceClass();
      this.source = source;
      this.url = URL.createObjectURL(source);
      this.video.src = this.url;
      if (!window.MediaSource) this.video.disableRemotePlayback = true;
      const signal = this.controller.signal;
      source.addEventListener(
        "sourceopen",
        () => {
          this.pump(
            source,
            url,
            mimeFor(this.mode, this.item, audio),
            signal,
          ).catch((error) => this.fail(error, signal));
        },
        { once: true },
      );
    }
    if (autoplay)
      this.video.play().catch((error) => {
        if (!this.disposed && error.name === "NotAllowedError")
          usePlayer.getState().patch({
            playing: false,
            loading: false,
            error: "Press play to begin.",
          });
      });
  }
  private async pump(
    source: MediaSource,
    url: string,
    mime: string,
    signal: AbortSignal,
  ) {
    const buffer = source.addSourceBuffer(mime);
    buffer.timestampOffset = this.target;
    if (this.item.runtime_seconds) source.duration = this.item.runtime_seconds;
    let response: Response | undefined;
    for (let attempt = 0; attempt < 4; attempt++) {
      response = await fetch(url, { signal, credentials: "same-origin" });
      if (response.status !== 429 || attempt === 3) break;
      await response.body?.cancel();
      await sleep(200 * (attempt + 1), signal);
    }
    if (!response?.ok || !response.body)
      throw new Error(
        (await response?.json().catch(() => ({})))?.error ||
          "Playback could not start. Please try again.",
      );
    if (this.mode === "remux") {
      const header = response.headers.get("x-media-start-seconds");
      if (header == null || !Number.isFinite(Number(header)))
        throw new Error("The remux timeline is unavailable.");
      buffer.timestampOffset = Number(header);
    }
    const reader = response.body.getReader();
    try {
      while (!signal.aborted) {
        while (
          buffer.buffered.length &&
          buffer.buffered.end(buffer.buffered.length - 1) -
            (this.initializing ? this.target : this.video.currentTime) >
            24
        )
          await sleep(120, signal);
        const { done, value } = await reader.read();
        if (done) break;
        const before = this.video.currentTime - 20;
        if (
          before > 0 &&
          buffer.buffered.length &&
          buffer.buffered.start(0) < before
        )
          await operation(buffer, () => buffer.remove(0, before), signal);
        await operation(buffer, () => buffer.appendBuffer(value), signal);
        if (
          this.initializing &&
          buffer.buffered.length &&
          (this.buffered(this.target) || buffer.buffered.start(0) > this.target)
        ) {
          this.video.currentTime = Math.max(
            this.target,
            buffer.buffered.start(0),
          );
          this.ready = true;
          this.initializing = false;
          if (this.video.paused) this.releasePaused();
        }
      }
      if (!signal.aborted && source.readyState === "open" && !buffer.updating)
        source.endOfStream();
    } finally {
      await reader.cancel().catch(() => {});
      reader.releaseLock();
    }
  }
  private buffered(position: number) {
    for (let n = 0; n < this.video.buffered.length; n++)
      if (
        position >= this.video.buffered.start(n) &&
        position < this.video.buffered.end(n) - 0.005
      )
        return true;
    return false;
  }
  seek(position: number) {
    const target = Math.max(
      0,
      Math.min(position, usePlayer.getState().duration || 604800),
    );
    if (this.mode !== "direct" && !this.buffered(target))
      this.start(target, !this.video.paused);
    else this.video.currentTime = target;
    usePlayer.getState().patch({ position: target });
  }
  save(stopped = false, completed = this.ended, keepalive = false) {
    if (
      !this.ready ||
      this.initializing ||
      !Number.isFinite(this.video.currentTime)
    )
      return;
    const record = {
      item_id: this.item.id,
      profile_id: this.profileId,
      position_ticks: Math.round(this.video.currentTime * 10_000_000),
      is_paused: this.video.paused,
      stopped,
      completed,
    };
    progressQueue = progressQueue
      .catch(() => {})
      .then(() =>
        api("/api/v1/playback", {
          method: "POST",
          body: JSON.stringify(record),
          keepalive,
        }),
      );
    progressQueue.catch((error) => {
      if (!this.disposed)
        usePlayer
          .getState()
          .patch({ error: `Progress could not be saved: ${message(error)}` });
    });
  }
  subtitle(index: number | null) {
    this.video.querySelectorAll("track").forEach((track) => track.remove());
    if (index == null) return;
    const data = this.info.MediaStreams.find(
      (stream) => stream.Type === "Subtitle" && stream.Index === index,
    );
    const track = document.createElement("track");
    track.kind = "subtitles";
    track.default = true;
    track.label = data?.Language || "Subtitles";
    track.src = `/api/v1/videos/${idPath(this.item.id)}/subtitles/${index}.vtt`;
    track.onload = () => {
      track.track.mode = "showing";
    };
    track.onerror = () =>
      usePlayer
        .getState()
        .patch({ error: "Subtitles could not load. Try again." });
    this.video.append(track);
    track.track.mode = "showing";
  }
  private releasePaused() {
    if (!this.ready || !this.source || this.source.readyState !== "open")
      return;
    clearTimeout(this.pauseTimer);
    this.pauseTimer = setTimeout(() => {
      if (this.video.paused && this.source?.readyState === "open") {
        this.retry = true;
        this.controller.abort();
      }
    }, 2500);
  }
  destroy() {
    this.save(true);
    this.disposed = true;
    this.listeners.forEach((remove) => remove());
    this.listeners = [];
    this.stopSource();
    this.video.pause();
    this.video.removeAttribute("src");
    this.video.load();
  }
}
