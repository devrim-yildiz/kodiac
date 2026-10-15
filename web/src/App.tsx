import MovieSheet from "./components/MovieSheet";
import { Player } from "./components/Player";
import * as inlineSettingsDependency0 from "react";
import * as inlineSettingsDependency1 from "@radix-ui/react-dialog";
import * as inlineSettingsDependency2 from "@radix-ui/react-tabs";
import * as inlineSettingsDependency3 from "lucide-react";
import * as inlineSettingsDependency4 from "./components/useSpatialPresence";
import * as inlineSettingsDependency5 from "./api";
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

const Settings = (() => {
const useEffect = inlineSettingsDependency0.useEffect;
const useState = inlineSettingsDependency0.useState;
const Dialog = inlineSettingsDependency1;
const Tabs = inlineSettingsDependency2;
const X = inlineSettingsDependency3.X;
const SlidersHorizontal = inlineSettingsDependency3.SlidersHorizontal;
const Database = inlineSettingsDependency3.Database;
const Clapperboard = inlineSettingsDependency3.Clapperboard;
const Activity = inlineSettingsDependency3.Activity;
const Check = inlineSettingsDependency3.Check;
const LoaderCircle = inlineSettingsDependency3.LoaderCircle;
const useSpatialPresence = inlineSettingsDependency4.useSpatialPresence;
const api = inlineSettingsDependency5.api;
const message = inlineSettingsDependency5.message;

type Config = {
  values: Record<string, string>;
  tmdb_configured: boolean;
  metadata_path: string;
  library_paths: string[];
};
type Stats = {
  version: string;
  architecture: string;
  rust_edition: string;
  ffmpeg: string;
  uptime_seconds: number;
  db_bytes: number;
  wal_bytes: number;
  artwork_bytes: number;
  media_count: number;
  extracted_count: number;
  metadata_pending: number;
  scan: { running: boolean; indexed: number; phase: string; failed?: number };
  active_streams: { title: string; profile: string }[];
  logs: string[];
};
const bytes = (n = 0) =>
  n < 1048576
    ? `${(n / 1024).toFixed(1)} KB`
    : `${(n / 1048576).toFixed(1)} MB`;
function Settings({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const spatial = useSpatialPresence(open);
  const [c, setC] = useState<Config | null>(null),
    [stats, setStats] = useState<Stats | null>(null),
    [key, setKey] = useState(""),
    [status, setStatus] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!open) return;
    const a = new AbortController();
    api<Config>("/api/v1/settings", { signal: a.signal })
      .then(setC)
      .catch((e) => {
        if (!a.signal.aborted) setError(message(e));
      });
    const refresh = () =>
      api<Stats>("/api/v1/settings/stats", { signal: a.signal })
        .then(setStats)
        .catch((e) => {
          if (!a.signal.aborted) setError(message(e));
        });
    void refresh();
    const timer = setInterval(refresh, 3000);
    return () => {
      a.abort();
      clearInterval(timer);
    };
  }, [open]);
  const update = (k: string, v: string) =>
    setC((c) => (c ? { ...c, values: { ...c.values, [k]: v } } : c));
  const action = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError("");
    setStatus("");
    try {
      await fn();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };
  const save = async (enrich = false) => {
    if (!c) return;
    const values = { ...c.values, ...(key ? { tmdb_api_key: key } : {}) };
    const saved = await api<Config>("/api/v1/settings", {
      method: "POST",
      body: JSON.stringify(values),
    });
    setC(saved);
    setKey("");
    if (enrich) await api("/api/v1/settings/rescan", { method: "POST" });
    setStatus(
      enrich
        ? "Saved. Library enrichment queued."
        : "Your preferences are saved.",
    );
  };
  return (
    <Dialog.Root
      open={spatial.present}
      onOpenChange={(v) => {
        if (!v) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay ref={spatial.overlay} className="dialog-overlay" />
        <Dialog.Content ref={spatial.panel} className="settings-panel glass">
          <header className="settings-header">
            <div>
              <span className="eyebrow">MAKE YOURSELF AT HOME</span>
              <Dialog.Title>Your cinema, your way.</Dialog.Title>
              <Dialog.Description>
                Fine-tune your library and every moment of playback.
              </Dialog.Description>
            </div>
            <Dialog.Close className="icon-button" aria-label="Close settings">
              <X />
            </Dialog.Close>
          </header>
          <Tabs.Root defaultValue="general" className="settings-layout">
            <Tabs.List className="settings-tabs" aria-label="Settings sections">
              {(
                [
                  ["general", "General & Metadata", SlidersHorizontal],
                  ["library", "Library & Storage", Database],
                  ["playback", "Streaming & Playback", Clapperboard],
                  ["system", "System & Diagnostics", Activity],
                ] as const
              ).map(([id, label, Icon]) => (
                <Tabs.Trigger key={String(id)} value={String(id)}>
                  <Icon size={17} />
                  <span>{String(label)}</span>
                </Tabs.Trigger>
              ))}
            </Tabs.List>
            <div className="settings-body">
              <Tabs.Content value="general">
                <span className="eyebrow">A LIBRARY WITH A STORY</span>
                <h2>Bring every title to life.</h2>
                <p>
                  Rich artwork, the people behind the story, and the details
                  that make choosing effortless.
                </p>
                <label>
                  TMDB API key
                  <span className="field-hint">
                    {c?.tmdb_configured
                      ? "A key is securely stored. Enter a replacement to update it."
                      : "Connect your TMDB account for posters, ratings and cast."}
                  </span>
                  <div className="input-action">
                    <input
                      type="password"
                      autoComplete="off"
                      value={key}
                      onChange={(e) => setKey(e.target.value)}
                      placeholder={
                        c?.tmdb_configured
                          ? "••••••••"
                          : "Enter API key or read access token"
                      }
                    />
                    <button
                      className="secondary-button"
                      disabled={busy}
                      onClick={() =>
                        void action(async () => {
                          const r = await api<{ valid: boolean }>(
                            "/api/v1/settings/verify",
                            {
                              method: "POST",
                              body: JSON.stringify({ tmdb_api_key: key }),
                            },
                          );
                          if (!r.valid)
                            throw new Error(
                              "The key could not be verified. Check the key and server connection.",
                            );
                          setStatus("TMDB key verified.");
                        })
                      }
                    >
                      Verify key
                    </button>
                  </div>
                </label>
                <label className="toggle-row">
                  <div>
                    <strong>Always find the scene</strong>
                    <span className="field-hint">
                      Extract a cinematic frame with FFmpeg when artwork is
                      missing.
                    </span>
                  </div>
                  <input
                    type="checkbox"
                    role="switch"
                    checked={c?.values.thumbnail_fallback !== "false"}
                    onChange={(e) =>
                      update("thumbnail_fallback", String(e.target.checked))
                    }
                  />
                </label>
                <div className="settings-note">
                  Artwork cache <code>{c?.metadata_path}</code>
                </div>
                <button
                  className="primary-button"
                  disabled={busy || !c}
                  onClick={() => void action(() => save(true))}
                >
                  Save & Enrich Library Now
                </button>
              </Tabs.Content>
              <Tabs.Content value="library">
                <span className="eyebrow">ALL YOUR STORIES, TOGETHER</span>
                <h2>Room for your collection.</h2>
                <p>Your monitored folders are mounted by the server.</p>
                <div className="path-list">
                  {c?.library_paths.map((p) => (
                    <div key={p}>
                      <Database size={16} />
                      <code>{p}</code>
                      <span>MONITORED</span>
                    </div>
                  ))}
                </div>
                <div className="storage-grid">
                  <div>
                    <strong>{bytes(stats?.db_bytes)}</strong>
                    <span>SQLite database</span>
                  </div>
                  <div>
                    <strong>{bytes(stats?.artwork_bytes)}</strong>
                    <span>Artwork cache</span>
                  </div>
                  <div>
                    <strong>{stats?.extracted_count ?? "—"}</strong>
                    <span>Extracted scenes</span>
                  </div>
                </div>
                <button
                  className="primary-button"
                  disabled={busy || stats?.scan.running}
                  onClick={() =>
                    void action(async () => {
                      await api("/api/v1/settings/rescan", { method: "POST" });
                      setStatus("Library scan started.");
                    })
                  }
                >
                  {stats?.scan.running ? (
                    <LoaderCircle className="spin" size={17} />
                  ) : (
                    <Database size={17} />
                  )}
                  Scan Libraries
                </button>
                <div className="scan-status" role="status">
                  <strong>{stats?.scan.phase || "Ready to scan"}</strong>
                  <p>
                    {stats?.scan.indexed ?? 0} indexed ·{" "}
                    {stats?.scan.failed ?? 0} failed ·{" "}
                    {stats?.metadata_pending ?? 0} metadata jobs pending
                  </p>
                  {stats?.scan.running && (
                    <progress aria-label="Scanning library" />
                  )}
                </div>
              </Tabs.Content>
              <Tabs.Content value="playback">
                <span className="eyebrow">PRESS PLAY. SETTLE IN.</span>
                <h2>A seamless first frame.</h2>
                <p>Defaults apply to the next playback session.</p>
                <label>
                  Hardware acceleration
                  <select
                    value={c?.values.hw_accel || "auto"}
                    onChange={(e) => update("hw_accel", e.target.value)}
                  >
                    <option value="auto">Auto</option>
                    <option value="software">Software</option>
                    <option value="vaapi">VAAPI</option>
                    <option value="videotoolbox">VideoToolbox</option>
                  </select>
                </label>
                <label>
                  Software encoding preset
                  <select
                    value={c?.values.transcode_preset || "ultrafast"}
                    onChange={(e) => update("transcode_preset", e.target.value)}
                  >
                    {[
                      "ultrafast",
                      "superfast",
                      "veryfast",
                      "fast",
                      "medium",
                    ].map((p) => (
                      <option key={p}>{p}</option>
                    ))}
                  </select>
                </label>
                <label>
                  Preferred audio language
                  <select
                    value={c?.values.audio_language_preference || "eng"}
                    onChange={(e) =>
                      update("audio_language_preference", e.target.value)
                    }
                  >
                    {[
                      ["deu", "German"],
                      ["eng", "English"],
                      ["fra", "French"],
                      ["spa", "Spanish"],
                      ["jpn", "Japanese"],
                      ["ita", "Italian"],
                    ].map(([id, label]) => (
                      <option value={id} key={id}>
                        {label}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Subtitles
                  <select
                    value={c?.values.subtitle_mode || "off"}
                    onChange={(e) => update("subtitle_mode", e.target.value)}
                  >
                    <option value="off">Off</option>
                    <option value="forced">Forced only</option>
                    <option value="always">Always on</option>
                  </select>
                </label>
                <button
                  className="primary-button"
                  disabled={busy || !c}
                  onClick={() => void action(() => save())}
                >
                  Save playback preferences
                </button>
              </Tabs.Content>
              <Tabs.Content value="system">
                <span className="eyebrow">BEHIND THE SCREEN</span>
                <h2>Everything in harmony.</h2>
                <div className="system-grid">
                  {[
                    ["Release", `v${stats?.version || "1.1.0"}`],
                    ["Rust edition", stats?.rust_edition],
                    ["Architecture", stats?.architecture],
                    [
                      "Uptime",
                      `${Math.floor((stats?.uptime_seconds || 0) / 60)} minutes`,
                    ],
                    ["Media files", stats?.media_count?.toLocaleString()],
                    ["SQLite WAL", bytes(stats?.wal_bytes)],
                  ].map(([k, v]) => (
                    <div key={k}>
                      <span>{k}</span>
                      <strong>{v || "—"}</strong>
                    </div>
                  ))}
                </div>
                <p className="ffmpeg-version">{stats?.ffmpeg}</p>
                <h3>Active streaming sessions</h3>
                {stats?.active_streams.length ? (
                  stats.active_streams.map((s, i) => (
                    <p key={i}>
                      {s.title} · {s.profile}
                    </p>
                  ))
                ) : (
                  <p>No streams playing right now.</p>
                )}
                <h3>Recent server events</h3>
                <pre className="server-logs">
                  {stats?.logs.join("\n") || "No recent events."}
                </pre>
              </Tabs.Content>
            </div>
          </Tabs.Root>
          <footer className="settings-footer" aria-live="polite">
            {busy ? (
              <>
                <LoaderCircle size={15} className="spin" />
                Working…
              </>
            ) : error ? (
              <span role="alert" className="inline-error">
                {error}
              </span>
            ) : status ? (
              <>
                <Check size={15} />
                {status}
              </>
            ) : (
              <span>KODIAC · CINEMATIC LUXURY · v1.1.0</span>
            )}
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

return Settings;
})();
