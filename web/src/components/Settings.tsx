import { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  Check,
  ChevronRight,
  RefreshCw,
  LoaderCircle,
  Plus,
} from "lucide-react";
import { api, message } from "../api";
import { useSession } from "../store";
import type { Preferences, User } from "../types";

type Config = {
  values: Record<string, string>;
  tmdb_configured: boolean;
  metadata_path: string;
  library_paths: string[];
};
type Stats = {
  version: string;
  architecture: string;
  ffmpeg: string;
  uptime_seconds: number;
  db_bytes: number;
  artwork_bytes: number | null;
  workers?: { name: string; status: string; updated_at: number }[];
  media_count: number;
  metadata_pending: number;
  scan: { running: boolean; indexed: number; phase: string; failed?: number };
  active_streams: { title: string; profile: string }[];
  logs: string[];
};
const bytes = (n?: number | null) =>
  n == null
    ? "Unavailable"
    : n < 1048576
      ? `${(n / 1024).toFixed(1)} KB`
      : `${(n / 1048576).toFixed(1)} MB`;
export const homeLabels: Record<string, string> = {
  continue: "Continue watching",
  collections: "Collections",
  recommended: "Recommended for you",
  top: "Top rated",
  series: "Unwatched series",
  recent: "Recently added",
};
const languages = [
  ["eng", "English"],
  ["deu", "German"],
  ["fra", "French"],
  ["spa", "Spanish"],
  ["jpn", "Japanese"],
  ["ita", "Italian"],
];

export function Settings({
  onClose,
  onPreferences,
  onDirtyChange,
}: {
  onClose: () => void;
  onPreferences: (p: Preferences) => void;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const user = useSession((s) => s.user)!;
  const [tab, setTab] = useState("");
  const [statsRevision, setStatsRevision] = useState(0);
  const [statsError, setStatsError] = useState("");
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [usersLoaded, setUsersLoaded] = useState(false);
  const [p, setP] = useState<Preferences | null>(null);
  const [c, setC] = useState<Config | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [users, setUsers] = useState<User[]>([]);
  const [key, setKey] = useState("");
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    onDirtyChange(dirty);
    return () => onDirtyChange(false);
  }, [dirty, onDirtyChange]);
  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);
  useEffect(() => {
    const a = new AbortController();
    api<Preferences>("/api/v1/preferences", { signal: a.signal })
      .then(setP)
      .catch((e) => {
        if (!a.signal.aborted) setError(message(e));
      });
    if (user.is_admin)
      api<Config>("/api/v1/settings", { signal: a.signal })
        .then(setC)
        .catch((e) => {
          if (!a.signal.aborted) setError(message(e));
        });
    return () => a.abort();
  }, [user.id, user.is_admin]);
  useEffect(() => {
    if (
      !user.is_admin ||
      !["", "server", "system", "library", "users"].includes(tab)
    )
      return;
    const a = new AbortController();
    const refresh = () => {
      if (tab === "users")
        return api<{ items: User[] }>("/api/v1/users", {
          signal: a.signal,
        }).then((r) => {
          setUsers(r.items);
          setUsersLoaded(true);
          setStatsError("");
        });
      return api<Stats>("/api/v1/settings/stats", { signal: a.signal }).then(
        (value) => {
          setStats(value);
          setUpdatedAt(Date.now());
          setStatsError("");
        },
      );
    };
    const run = () =>
      void refresh().catch((e) => {
        if (!a.signal.aborted) setStatsError(message(e));
      });
    run();
    const timer = tab === "users" ? undefined : setInterval(run, 10000);
    return () => {
      a.abort();
      clearInterval(timer);
    };
  }, [tab, user.is_admin, statsRevision]);
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
  const updateP = (values: Partial<Preferences>) => {
    setP((p) => (p ? { ...p, ...values } : p));
    setDirty(true);
    setStatus("");
  };
  const update = (k: string, v: string) => {
    setC((c) => (c ? { ...c, values: { ...c.values, [k]: v } } : c));
    setDirty(true);
    setStatus("");
  };
  const save = () =>
    action(async () => {
      if (tab === "home" || tab === "playback") {
        if (!p) return;
        const saved = await api<Preferences>("/api/v1/preferences", {
          method: "PUT",
          body: JSON.stringify(p),
        });
        setP(saved);
        onPreferences(saved);
      } else {
        if (!c) return;
        setC(
          await api<Config>("/api/v1/settings", {
            method: "POST",
            body: JSON.stringify({
              ...c.values,
              ...(key ? { tmdb_api_key: key } : {}),
            }),
          }),
        );
        setKey("");
      }
      setDirty(false);
      setStatus("Changes saved.");
    });
  const go = (next: string) => {
    if (dirty && !window.confirm("Discard unsaved changes?")) return;
    // Reload discarded edits so switching back never resurrects unsaved values.
    if (dirty) {
      void api<Preferences>("/api/v1/preferences")
        .then(setP)
        .catch((e) => setError(message(e)));
      if (user.is_admin)
        void api<Config>("/api/v1/settings")
          .then(setC)
          .catch((e) => setError(message(e)));
    }
    setTab(next);
    setDirty(false);
    setError("");
    setStatus("");
    setKey("");
    window.scrollTo({ top: 0 });
  };
  const ordered = p
    ? [
        ...p.home_sections,
        ...Object.keys(homeLabels).filter((k) => !p.home_sections.includes(k)),
      ]
    : [];
  const move = (id: string, delta: number) => {
    if (!p) return;
    const sections = [...p.home_sections];
    const i = sections.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= sections.length) return;
    [sections[i], sections[j]] = [sections[j], sections[i]];
    updateP({ home_sections: sections });
  };
  const sections = [
    [
      "Personal",
      [
        ["home", "Home screen"],
        ["playback", "Playback"],
      ],
    ],
    ...(user.is_admin
      ? [
          [
            "Server",
            [
              ["server", "Server overview"],
              ["library", "Libraries"],
              ["metadata", "Metadata"],
              ["encoding", "Transcoding"],
              ["users", "Users"],
              ["system", "Diagnostics"],
            ],
          ],
        ]
      : []),
  ] as [string, string[][]][];
  const titles: Record<string, string> = {
    "": "Overview",
    server: "Server overview",
    home: "Home screen",
    playback: "Playback",
    library: "Libraries",
    metadata: "Metadata",
    encoding: "Transcoding",
    users: "Users",
    system: "Diagnostics",
  };
  const descriptions: Record<string, string> = {
    playback: p
      ? `${languages.find(([id]) => id === p.audio_language_preference)?.[1] || p.audio_language_preference} audio`
      : "Audio and subtitles",
    home: p ? `${p.home_sections.length} enabled rows` : "Rows and order",
    server: stats
      ? `${stats.active_streams.length} active streams`
      : "Status and activity",
    library: "Media folders and scans",
    metadata: c?.tmdb_configured ? "TMDB connected" : "Information and artwork",
    encoding: "Hardware and encoding",
    users: "Accounts and access",
    system: "Version and server events",
  };
  const workerState = (w: NonNullable<Stats["workers"]>[number]) =>
    !["paused", "stopped"].includes(w.status) &&
    Date.now() / 1000 - w.updated_at > 30
      ? "Not responding"
      : w.status.replaceAll("_", " ");
  return (
    <main
      className={`settings-page content-width ${tab ? "is-detail" : "is-overview"}`}
    >
      <header className="settings-page-header">
        <button
          className="back-link settings-exit"
          onClick={() => {
            if (!dirty || window.confirm("Discard unsaved changes?")) onClose();
          }}
        >
          <ArrowLeft size={17} /> Back to library
        </button>
        {tab && (
          <button
            className="back-link settings-mobile-back"
            onClick={() => go("")}
          >
            <ArrowLeft size={17} /> Settings
          </button>
        )}
        <h1>Settings</h1>
      </header>
      <div className="settings-layout">
        <nav className="settings-tabs" aria-label="Settings sections">
          <button
            aria-current={!tab ? "page" : undefined}
            onClick={() => go("")}
          >
            Overview
          </button>
          {sections.map(([group, tabs]) => (
            <div key={group}>
              <h2>{group}</h2>
              {tabs.map(([id, label]) => (
                <button
                  key={id}
                  aria-current={tab === id ? "page" : undefined}
                  onClick={() => go(id)}
                >
                  {label}
                </button>
              ))}
            </div>
          ))}
        </nav>
        <section className="settings-body" aria-label={titles[tab]}>
          {tab && <h2 tabIndex={-1}>{titles[tab]}</h2>}
          {statsError && (
            <div className="settings-alert" role="alert">
              <span>Server data could not refresh. {statsError}</span>
              <button
                className="text-button"
                onClick={() => setStatsRevision((n) => n + 1)}
              >
                Retry
              </button>
            </div>
          )}
          {!tab && (
            <div className="settings-overview">
              <div className="settings-account">
                <span className="avatar">
                  {user.username.slice(0, 1).toUpperCase()}
                </span>
                <div>
                  <strong>{user.username}</strong>
                  <p>
                    {user.is_admin
                      ? "Administrator account"
                      : "Personal account"}
                  </p>
                </div>
              </div>
              {sections.map(([group, tabs]) => (
                <section className="settings-category" key={group}>
                  <h2>
                    {group === "Server" ? "Server administration" : "Personal"}
                  </h2>
                  <div className="settings-group">
                    {tabs.map(([id, label]) => (
                      <button
                        className="settings-destination"
                        aria-label={`Open ${label.toLowerCase()}`}
                        key={id}
                        onClick={() => go(id)}
                      >
                        <span>{label}</span>
                        <span className="destination-description">
                          {descriptions[id]}
                        </span>
                        <ChevronRight size={17} />
                      </button>
                    ))}
                  </div>
                </section>
              ))}
            </div>
          )}
          {tab === "server" && (
            <>
              <div className="server-refresh">
                <span>
                  {updatedAt
                    ? `Updated ${new Date(updatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
                    : "Loading server status…"}
                </span>
                <button
                  className="icon-button"
                  aria-label="Refresh server stats"
                  onClick={() => setStatsRevision((n) => n + 1)}
                >
                  <RefreshCw size={17} />
                </button>
              </div>
              <dl className="server-facts settings-group">
                {[
                  [
                    "Uptime",
                    stats
                      ? `${Math.floor(stats.uptime_seconds / 86400)}d ${Math.floor((stats.uptime_seconds % 86400) / 3600)}h ${Math.floor((stats.uptime_seconds % 3600) / 60)}m`
                      : "—",
                  ],
                  ["Media files", stats?.media_count?.toLocaleString() ?? "—"],
                  ["Active streams", stats?.active_streams.length ?? "—"],
                  [
                    "Metadata queue",
                    stats?.metadata_pending?.toLocaleString() ?? "—",
                  ],
                  ["Database", stats ? bytes(stats.db_bytes) : "—"],
                  ["Artwork cache", stats ? bytes(stats.artwork_bytes) : "—"],
                ].map(([label, value]) => (
                  <div key={label}>
                    <dt>{label}</dt>
                    <dd>{value}</dd>
                  </div>
                ))}
              </dl>
              <h3>Background workers</h3>
              <div className="settings-group worker-list">
                {stats?.workers?.length ? (
                  stats.workers.map((w) => (
                    <div className="preference-row" key={w.name}>
                      <span>{w.name}</span>
                      <span
                        className={`worker-state ${workerState(w) === "Not responding" ? "is-warning" : ""}`}
                      >
                        {workerState(w)}
                      </span>
                    </div>
                  ))
                ) : (
                  <p className="settings-empty">
                    {stats ? "No worker status reported." : "Loading workers…"}
                  </p>
                )}
              </div>
              <h3>Streaming now</h3>
              <div className="settings-group">
                {stats?.active_streams.length ? (
                  stats.active_streams.map((stream, i) => (
                    <div className="stream-row" key={i}>
                      <strong>{stream.title}</strong>
                      <span>{stream.profile}</span>
                    </div>
                  ))
                ) : (
                  <p className="settings-empty">
                    {stats ? "No active streams." : "Loading streams…"}
                  </p>
                )}
              </div>
            </>
          )}
          {tab === "home" && (
            <>
              <p>Choose and reorder the rows shown on Home.</p>
              {!p ? (
                <p>Loading preferences…</p>
              ) : (
                <div className="home-order settings-group">
                  {ordered.map((id) => {
                    const i = p.home_sections.indexOf(id);
                    return (
                      <div className="preference-row" key={id}>
                        <label className="check-label">
                          <input
                            type="checkbox"
                            disabled={busy}
                            checked={i >= 0}
                            onChange={(e) =>
                              updateP({
                                home_sections: e.target.checked
                                  ? [...p.home_sections, id]
                                  : p.home_sections.filter((k) => k !== id),
                              })
                            }
                          />
                          {homeLabels[id]}
                        </label>
                        <div className="reorder-actions">
                          <button
                            className="icon-button"
                            disabled={busy || i <= 0}
                            aria-label={`Move ${homeLabels[id]} up`}
                            onClick={() => move(id, -1)}
                          >
                            <ArrowUp size={17} />
                          </button>
                          <button
                            className="icon-button"
                            disabled={
                              busy || i < 0 || i === p.home_sections.length - 1
                            }
                            aria-label={`Move ${homeLabels[id]} down`}
                            onClick={() => move(id, 1)}
                          >
                            <ArrowDown size={17} />
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </>
          )}
          {tab === "playback" && (
            <div className="settings-fields settings-group">
              <p>Your defaults apply when you start a video.</p>
              <label>
                Preferred audio language
                <select
                  disabled={!p || busy}
                  value={p?.audio_language_preference || "eng"}
                  onChange={(e) =>
                    updateP({ audio_language_preference: e.target.value })
                  }
                >
                  {languages.map(([id, label]) => (
                    <option key={id} value={id}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Subtitles
                <select
                  disabled={!p || busy}
                  value={p?.subtitle_mode || "off"}
                  onChange={(e) => updateP({ subtitle_mode: e.target.value })}
                >
                  <option value="off">Off</option>
                  <option value="forced">Forced only</option>
                  <option value="always">Always on</option>
                </select>
              </label>
              <p className="field-hint">
                Audio and subtitle tracks can also be changed in the player.
              </p>
            </div>
          )}
          {tab === "metadata" && (
            <div className="settings-fields settings-group">
              <p>
                Connect TMDB for movie and series information, artwork and
                collections.
              </p>
              <label>
                TMDB API key
                <span className="field-hint">
                  {c?.tmdb_configured
                    ? "A key is saved. Enter a key to replace it."
                    : "API key or read access token."}
                </span>
                <div className="input-action">
                  <input
                    type="password"
                    autoComplete="off"
                    value={key}
                    onChange={(e) => {
                      setKey(e.target.value);
                      setDirty(true);
                    }}
                    placeholder={c?.tmdb_configured ? "••••••••" : "Enter key"}
                  />
                  <button
                    className="secondary-button"
                    disabled={busy || !c}
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
                            "Key could not be verified. Check the key and connection.",
                          );
                        setStatus("Key verified. Save to apply it.");
                      })
                    }
                  >
                    Verify key
                  </button>
                </div>
              </label>
              <label className="toggle-row">
                <span>
                  <strong>Generate missing artwork</strong>
                  <span className="field-hint">
                    Use a video frame when no poster or backdrop is available.
                  </span>
                </span>
                <input
                  type="checkbox"
                  role="switch"
                  checked={c?.values.thumbnail_fallback !== "false"}
                  disabled={!c || busy}
                  onChange={(e) =>
                    update("thumbnail_fallback", String(e.target.checked))
                  }
                />
              </label>
            </div>
          )}
          {tab === "encoding" && (
            <div className="settings-fields settings-group">
              <p>
                Server encoding options. Original files play directly whenever
                supported.
              </p>
              <label>
                Hardware acceleration
                <select
                  disabled={!c || busy}
                  value={c?.values.hw_accel || "auto"}
                  onChange={(e) => update("hw_accel", e.target.value)}
                >
                  {[
                    "auto",
                    "software",
                    "vaapi",
                    "nvenc",
                    "qsv",
                    "amf",
                    "videotoolbox",
                  ].map((v) => (
                    <option key={v} value={v}>
                      {v === "auto"
                        ? "Automatic"
                        : v === "software"
                          ? "Software"
                          : v.toUpperCase()}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Software encoding preset
                <select
                  disabled={!c || busy}
                  value={c?.values.transcode_preset || "ultrafast"}
                  onChange={(e) => update("transcode_preset", e.target.value)}
                >
                  {["ultrafast", "superfast", "veryfast", "fast", "medium"].map(
                    (v) => (
                      <option key={v}>{v}</option>
                    ),
                  )}
                </select>
                <span className="field-hint">
                  Slower presets use more CPU and can reduce bandwidth.
                </span>
              </label>
            </div>
          )}
          {tab === "library" && (
            <>
              <p>
                Media folders are configured through the server's mounted
                volumes.
              </p>
              <div className="path-list settings-group">
                {c?.library_paths.map((path) => (
                  <div key={path}>
                    <strong>
                      {path.split(/[\\/]/).filter(Boolean).at(-1)}
                    </strong>
                    <code>{path}</code>
                  </div>
                ))}
              </div>
              <button
                className="secondary-button"
                disabled={busy || !stats || stats.scan.running}
                onClick={() =>
                  void action(async () => {
                    await api("/api/v1/settings/rescan", { method: "POST" });
                    setStatus(
                      "Scan requested. Background workers will update the library.",
                    );
                  })
                }
              >
                {stats?.scan.running ? "Scanning…" : "Scan libraries"}
              </button>
              <div className="scan-status">
                <strong>{stats?.scan.phase || "Loading scan status…"}</strong>
                <p>
                  {stats?.scan.indexed ?? "—"} indexed ·{" "}
                  {stats?.scan.failed ?? "—"} failed ·{" "}
                  {stats?.metadata_pending ?? "—"} metadata jobs pending
                </p>
              </div>
              <details>
                <summary>Storage details</summary>
                <p>Artwork path</p>
                <code>{c?.metadata_path}</code>
              </details>
            </>
          )}
          {tab === "users" && (
            <>
              <p>
                Each account has its own profiles, progress and preferences.
              </p>
              <div className="user-list settings-group">
                {users.map((account) => (
                  <div className="preference-row" key={account.id}>
                    <span>
                      <strong>{account.username}</strong>
                      {account.id === user.id && <small> You</small>}
                    </span>
                    <label className="sr-only" htmlFor={`role-${account.id}`}>
                      Role for {account.username}
                    </label>
                    <select
                      id={`role-${account.id}`}
                      disabled={busy || account.id === user.id}
                      value={String(account.is_admin)}
                      onChange={(e) => {
                        const is_admin = e.target.value === "true";
                        void action(async () => {
                          await api(
                            `/api/v1/users/${encodeURIComponent(account.id)}/role`,
                            {
                              method: "PUT",
                              body: JSON.stringify({ is_admin }),
                            },
                          );
                          setUsers((rows) =>
                            rows.map((row) =>
                              row.id === account.id
                                ? { ...row, is_admin }
                                : row,
                            ),
                          );
                          setStatus("Role updated.");
                        });
                      }}
                    >
                      <option value="false">User</option>
                      <option value="true">Administrator</option>
                    </select>
                  </div>
                ))}
              </div>
              {!usersLoaded && !statsError && (
                <p className="settings-empty" role="status">
                  Loading accounts…
                </p>
              )}
              <details className="create-account">
                <summary>
                  <Plus size={17} /> Create account
                </summary>
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    const form = e.currentTarget;
                    const data = new FormData(form);
                    void action(async () => {
                      const created = await api<User>("/api/v1/users", {
                        method: "POST",
                        body: JSON.stringify({
                          username: data.get("username"),
                          password: data.get("password"),
                          is_admin: data.get("role") === "admin",
                        }),
                      });
                      setUsers((rows) => [...rows, created]);
                      form.reset();
                      setStatus(`Account created for ${created.username}.`);
                    });
                  }}
                >
                  <label>
                    Username
                    <input
                      name="username"
                      autoComplete="off"
                      maxLength={80}
                      required
                      disabled={busy}
                    />
                  </label>
                  <label>
                    Initial password
                    <input
                      aria-label="Initial password"
                      name="password"
                      type="password"
                      autoComplete="new-password"
                      minLength={12}
                      maxLength={1024}
                      required
                      disabled={busy}
                    />
                    <span className="field-hint">
                      At least 12 characters. Share it privately with the
                      account owner.
                    </span>
                  </label>
                  <label>
                    Account role
                    <select name="role" disabled={busy}>
                      <option value="user">User</option>
                      <option value="admin">Administrator</option>
                    </select>
                  </label>
                  <button className="primary-button" disabled={busy}>
                    <Plus size={17} /> Create account
                  </button>
                </form>
              </details>
            </>
          )}
          {tab === "system" && (
            <>
              <dl className="server-facts settings-group">
                <div>
                  <dt>Version</dt>
                  <dd>{stats?.version ?? "—"}</dd>
                </div>
                <div>
                  <dt>Architecture</dt>
                  <dd>{stats?.architecture ?? "—"}</dd>
                </div>
              </dl>
              <details className="settings-diagnostics">
                <summary>FFmpeg</summary>
                <p>{stats?.ffmpeg || "Loading encoder information…"}</p>
              </details>
              <h3>Recent server events</h3>
              <pre className="server-logs">
                {stats
                  ? stats.logs.join("\n") || "No recent events."
                  : "Loading events…"}
              </pre>
            </>
          )}
          {["home", "playback", "metadata", "encoding"].includes(tab) &&
            dirty && (
              <div className="settings-save">
                <button
                  className="primary-button"
                  disabled={busy || !dirty || !p || (user.is_admin && !c)}
                  onClick={() => void save()}
                >
                  Save changes
                </button>
                <span>{dirty ? "Unsaved changes" : "All changes saved"}</span>
              </div>
            )}
          <div className="settings-feedback" aria-live="polite">
            {busy ? (
              <span>
                <LoaderCircle size={16} className="spin" /> Working…
              </span>
            ) : error ? (
              <p role="alert" className="inline-error">
                {error}
              </p>
            ) : status ? (
              <span>
                <Check size={16} /> {status}
              </span>
            ) : null}
          </div>
        </section>
      </div>
    </main>
  );
}
