import {
  Component,
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { ErrorInfo, ReactNode } from "react";
import * as Menu from "@radix-ui/react-dropdown-menu";
import * as Dialog from "@radix-ui/react-dialog";
import * as Tabs from "@radix-ui/react-tabs";
import {
  ArrowRight,
  Check,
  ChevronDown,
  LoaderCircle,
  LogOut,
  Plus,
  Search,
  Settings2,
  X,
} from "lucide-react";
import { allPages, api, aborted, message } from "./api";
import { cancelOpening, playTitle, usePlayer, useSession } from "./store";
import { flushProgress } from "./player/engine";
import type { Preferences, Profile, Title, User } from "./types";
import { idPath } from "./types";
import { Player } from "./components/Player";
import { ContinueWatching, VirtualLibrary } from "./components/Library";
import { Empty, Loading } from "./components/shared";

import { captureOrigin } from "./components/useSpatialPresence";
import { Settings } from "./components/Settings";
import { Curated, CommandPalette } from "./components/Discovery";
import type { Hub } from "./components/Discovery";

const ShowPage = lazy(() => import("./components/ShowPage"));
const MovieSheet = lazy(() => import("./components/MovieSheet"));
async function enter() {
  const [user, profiles] = await Promise.all([
    api<User>("/api/v1/me"),
    api<{ items: Profile[] }>("/api/v1/profiles"),
  ]);
  let saved = "";
  try {
    saved = localStorage.getItem(`media-profile:${user.id}`) || "";
  } catch {
    /* private browsing */
  }
  const profile =
    profiles.items.find((p) => p.id === saved) ||
    profiles.items.find((p) => p.is_default) ||
    profiles.items[0];
  if (!profile)
    throw new Error("No viewing profile is available for this account.");
  useSession.getState().set({
    user,
    profiles: profiles.items,
    profileId: profile.id,
    ready: true,
  });
}
function Login() {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <main className="login-page">
      <div className="login-atmosphere" />
      <div className="login-brand">
        <span className="brand-icon">
          <PlayMark />
        </span>
        kodiac
      </div>
      <div className="login-card">
        <h1>Sign in to Kodiac</h1>
        <p>Use your server account to open your library.</p>
        <form
          onSubmit={async (event) => {
            event.preventDefault();
            setBusy(true);
            setError("");
            const data = new FormData(event.currentTarget);
            try {
              await api("/api/v1/login", {
                method: "POST",
                body: JSON.stringify({
                  username: data.get("username"),
                  password: data.get("password"),
                }),
              });
              await enter();
            } catch (error) {
              setError(message(error));
            } finally {
              setBusy(false);
            }
          }}
        >
          <label>
            Username
            <input
              autoComplete="username"
              name="username"
              placeholder="Your username"
              required
              autoFocus
            />
          </label>
          <label>
            Password
            <input
              type="password"
              autoComplete="current-password"
              name="password"
              placeholder="Your password"
              required
            />
          </label>
          {error && (
            <p role="alert" className="inline-error">
              {error}
            </p>
          )}
          <button className="primary-button" disabled={busy}>
            {busy ? (
              <LoaderCircle size={18} className="spin" />
            ) : (
              <>
                Sign in <ArrowRight size={18} />
              </>
            )}
          </button>
        </form>
      </div>
    </main>
  );
}
function PlayMark() {
  return (
    <svg width="19" height="20" viewBox="0 0 19 20" aria-hidden="true">
      <path
        d="M2 2v16M7 2v16L18 10z"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}
class Boundary extends Component<{ children: ReactNode }, { error: boolean }> {
  state = { error: false };
  static getDerivedStateFromError() {
    return { error: true };
  }
  componentDidCatch(_error: Error, _info: ErrorInfo) {
    /* Render a recoverable UI without logging user data. */
  }
  render() {
    return this.state.error ? (
      <Empty title="The library could not be displayed">
        Reload to try again.
        <button className="primary-button" onClick={() => location.reload()}>
          Reload
        </button>
      </Empty>
    ) : (
      this.props.children
    );
  }
}
export function App() {
  return (
    <Boundary>
      <Cinema />
    </Boundary>
  );
}
function Cinema() {
  const session = useSession();
  const active = usePlayer((state) => Boolean(state.item));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsDirty, setSettingsDirty] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const [hub, setHub] = useState<Hub | null>(null);
  const [homeError, setHomeError] = useState("");
  useEffect(() => {
    const scroll = () => setScrolled(window.scrollY > 50);
    window.addEventListener("scroll", scroll, { passive: true });
    return () => window.removeEventListener("scroll", scroll);
  }, []);
  const [kind, setKind] = useState("all");
  const [query, setQuery] = useState("");
  const [genre, setGenre] = useState("");
  const [sort, setSort] = useState("title");
  const [preferences, setPreferences] = useState<Preferences | null>(null);
  const [collectionView, setCollectionView] = useState<{
    id: string;
    title: string;
  } | null>(null);
  const [collectionItems, setCollectionItems] = useState<Title[]>([]);
  const [collectionLoading, setCollectionLoading] = useState(false);
  const [collectionError, setCollectionError] = useState("");
  const [movies, setMovies] = useState<Title[]>([]);
  const [shows, setShows] = useState<Title[]>([]);
  const [counts, setCounts] = useState({ movie: 0, show: 0 });
  const [continued, setContinued] = useState<Title[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [historyError, setHistoryError] = useState("");
  const [notice, setNotice] = useState("");
  const [revision, setRevision] = useState(0);
  const [historyRevision, setHistoryRevision] = useState(0);
  const [show, setShow] = useState<Title | null>(null);
  const [movie, setMovie] = useState<Title | null>(null);
  const lastMovie = useRef<Title | null>(null);
  if (movie) lastMovie.current = movie;
  const [addProfile, setAddProfile] = useState(false);
  const [profileError, setProfileError] = useState("");
  const [profileBusy, setProfileBusy] = useState(false);
  const [playBusy, setPlayBusy] = useState(false);
  const search = useRef<HTMLInputElement>(null);
  const reset = () => {
    cancelOpening();
    usePlayer.getState().close();
    useSession
      .getState()
      .set({ user: null, profiles: [], profileId: "", ready: true });
    setMovies([]);
    setShows([]);
    setContinued([]);
    setMovie(null);
    setShow(null);
    setQuery("");
    setPreferences(null);
    setCollectionView(null);
    setSettingsOpen(false);
    setHub(null);
    setNotice("");
  };
  useEffect(() => {
    const expired = () => reset();
    window.addEventListener("session-expired", expired);
    let current = true;
    enter().catch((error) => {
      if (current) {
        useSession.getState().set({ ready: true });
        if (!(
          error instanceof Error &&
          "status" in error &&
          error.status === 401
        ))
          setNotice(message(error));
      }
    });
    return () => {
      current = false;
      window.removeEventListener("session-expired", expired);
    };
  }, []);
  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "k") {
        event.preventDefault();
        usePlayer.getState().patch({ expanded: false });
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, []);
  useEffect(() => {
    if (!session.user) return;
    const controller = new AbortController();
    setLoading(true);
    setError("");
    setMovies([]);
    setShows([]);
    setCounts({ movie: 0, show: 0 });
    Promise.all([
      allPages<Title>("/api/v1/movies", controller.signal, (items, total) => {
        setMovies(items.map((item) => ({ ...item, kind: "movie" })));
        setCounts((c) => ({ ...c, movie: total }));
      }).then((items) =>
        setMovies(items.map((item) => ({ ...item, kind: "movie" }))),
      ),
      allPages<Title>("/api/v1/shows", controller.signal, (items, total) => {
        setShows(items.map((item) => ({ ...item, kind: "show" })));
        setCounts((c) => ({ ...c, show: total }));
      }).then((items) =>
        setShows(items.map((item) => ({ ...item, kind: "show" }))),
      ),
    ])
      .then(() => setLoading(false))
      .catch((error) => {
        if (!aborted(error)) {
          setError(message(error));
          setLoading(false);
          controller.abort();
        }
      });
    return () => controller.abort();
  }, [session.user?.id, revision]);
  useEffect(() => {
    if (!session.user || !session.profileId) return;
    const controller = new AbortController();
    setContinued([]);
    setHistoryError("");
    allPages<Title>(
      `/api/v1/continue-watching?profile_id=${idPath(session.profileId)}`,
      controller.signal,
    )
      .then(setContinued)
      .catch((error) => {
        if (!aborted(error)) setHistoryError(message(error));
      });
    return () => controller.abort();
  }, [session.user?.id, session.profileId, historyRevision]);
  useEffect(() => {
    const refresh = () => setHistoryRevision((n) => n + 1);
    window.addEventListener("history-changed", refresh);
    return () => window.removeEventListener("history-changed", refresh);
  }, []);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(""), 7000);
    return () => clearTimeout(timer);
  }, [notice]);
  useEffect(() => {
    const grid = document.getElementById("library");
    if (grid && window.scrollY > grid.offsetTop + 40)
      window.scrollTo({ top: grid.offsetTop - 100 });
  }, [query, kind]);
  const collection = useMemo(
    () =>
      [...(kind === "show" ? [] : movies), ...(kind === "movie" ? [] : shows)]
        .filter((item) => kind !== "anime" || item.is_anime)
        .sort(
          (a, b) => a.title.localeCompare(b.title) || a.id.localeCompare(b.id),
        ),
    [movies, shows, kind],
  );
  useEffect(() => {
    if (!session.user) return;
    const a = new AbortController();
    setPreferences(null);
    setHomeError("");
    api<Preferences>("/api/v1/preferences", { signal: a.signal })
      .then(setPreferences)
      .catch((e) => {
        if (!a.signal.aborted) setHomeError(message(e));
      });
    try {
      const saved = localStorage.getItem(`kodiac-sort:${session.user.id}`);
      setSort(
        ["title", "title-desc", "year", "rating"].includes(saved || "")
          ? saved!
          : "title",
      );
    } catch {
      setSort("title");
    }
    return () => a.abort();
  }, [session.user?.id, revision]);
  useEffect(() => {
    if (!collectionView) return;
    const a = new AbortController();
    setCollectionItems([]);
    setCollectionLoading(true);
    setCollectionError("");
    allPages<Title>(
      `/api/v1/collections/${idPath(collectionView.id)}`,
      a.signal,
    )
      .then((items) => {
        setCollectionItems(items);
        setCollectionLoading(false);
      })
      .catch((e) => {
        if (!a.signal.aborted) {
          setCollectionError(message(e));
          setCollectionLoading(false);
        }
      });
    return () => a.abort();
  }, [collectionView?.id, revision]);
  const libraryItems = collectionView ? collectionItems : collection;
  const genres = useMemo(
    () =>
      [
        ...new Map(
          libraryItems
            .flatMap((t) => t.genres || [])
            .map((g) => [String(g.id), g]),
        ).values(),
      ].sort((a, b) => a.name.localeCompare(b.name)),
    [libraryItems],
  );
  const filtered = useMemo(
    () =>
      libraryItems
        .filter(
          (item) =>
            item.title
              .toLocaleLowerCase()
              .includes(query.trim().toLocaleLowerCase()) &&
            (!genre || item.genres?.some((g) => String(g.id) === genre)),
        )
        .sort((a, b) => {
          const title =
            a.title.localeCompare(b.title, undefined, { numeric: true }) ||
            a.id.localeCompare(b.id);
          return sort === "title-desc"
            ? -title
            : sort === "year"
              ? (b.year ?? -1) - (a.year ?? -1) || title
              : sort === "rating"
                ? (b.rating ?? -1) - (a.rating ?? -1) || title
                : title;
        }),
    [libraryItems, query, genre, sort],
  );
  useEffect(() => {
    if (!session.user || !session.profileId) return;
    const a = new AbortController();
    api<Hub>(`/api/v1/hub/home?profile_id=${idPath(session.profileId)}`, {
      signal: a.signal,
    })
      .then(setHub)
      .catch((e) => {
        if (!a.signal.aborted) setHomeError(message(e));
      });
    return () => a.abort();
  }, [session.user?.id, session.profileId, revision, historyRevision]);
  const resumeById = useMemo(
    () => new Map(continued.map((item) => [item.id, item.position_ticks])),
    [continued],
  );
  const currentProfile = session.profiles.find(
    (profile) => profile.id === session.profileId,
  );
  const browse = () => {
    if (
      settingsOpen &&
      settingsDirty &&
      !window.confirm("Discard unsaved changes?")
    )
      return false;
    usePlayer.getState().patch({ expanded: false });
    setShow(null);
    setMovie(null);
    setSettingsOpen(false);
    setCollectionView(null);
    setCollectionError("");
    setQuery("");
    setGenre("");
    window.scrollTo({ top: 0 });
    return true;
  };
  const openCollection = (c: { id: string; title: string }) => {
    if (!browse()) return;
    setCollectionView(c);
  };
  const onOpen = (item: Title) => {
    if (settingsOpen && !browse()) return;
    if (item.kind === "episode") {
      const parent = shows.find((s) => s.id === item.show_id);
      if (parent) {
        setShow(parent);
        setMovie(null);
        window.scrollTo({ top: 0 });
      } else void onPlay(item);
      return;
    }
    usePlayer.getState().patch({ expanded: false });
    if (item.kind === "show") {
      setShow(item);
      setMovie(null);
      window.scrollTo({ top: 0 });
    } else {
      captureOrigin(document.activeElement as HTMLElement);
      setMovie(item);
    }
  };
  const onPlay = async (item: Title) => {
    if (item.kind === "show") {
      onOpen(item);
      return;
    }
    setPlayBusy(true);
    try {
      await playTitle(
        item,
        (item.position_ticks ?? resumeById.get(item.id)) == null
          ? undefined
          : (item.position_ticks ?? resumeById.get(item.id))! / 10_000_000,
      );
      setMovie(null);
    } catch (error) {
      setNotice(message(error));
    } finally {
      setPlayBusy(false);
    }
  };
  const switchProfile = async (id: string) => {
    if (id === session.profileId) return;
    cancelOpening();
    window.dispatchEvent(new Event("player-save"));
    usePlayer.getState().close();
    await flushProgress();
    try {
      localStorage.setItem(`media-profile:${session.user?.id}`, id);
    } catch {
      /* private browsing */
    }
    useSession.getState().set({ profileId: id });
    setMovie(null);
    setShow(null);
    setNotice("");
    setHistoryError("");
  };
  if (!session.ready)
    return (
      <div className="boot-screen">
        <Loading label="Loading Kodiac…" />
      </div>
    );
  return (
    <>
      <div id="browse-shell" className={active ? "has-player" : ""}>
        {!session.user ? (
          <Login />
        ) : (
          <>
            <header
              className={`navigation ${scrolled ? "is-scrolled glass" : ""}`}
            >
              <button
                className="brand"
                onClick={() => {
                  if (browse()) setKind("all");
                }}
                aria-label="Kodiac home"
              >
                <span className="brand-icon">
                  <PlayMark />
                </span>
                <span>kodiac</span>
              </button>
              <Tabs.Root
                value={settingsOpen ? "" : kind}
                onValueChange={(value) => {
                  if (browse()) setKind(value);
                }}
              >
                <Tabs.List className="segments" aria-label="Library view">
                  <Tabs.Trigger value="all">Home</Tabs.Trigger>
                  <Tabs.Trigger value="movie">Movies</Tabs.Trigger>
                  <Tabs.Trigger value="show">Series</Tabs.Trigger>
                  <Tabs.Trigger value="anime">Anime</Tabs.Trigger>
                </Tabs.List>
              </Tabs.Root>
              <div className="nav-right">
                <button
                  className="icon-button"
                  aria-label="Open search"
                  onClick={() => setPaletteOpen(true)}
                >
                  <Search size={19} />
                </button>
                <button
                  className="icon-button"
                  aria-label="Open settings"
                  onClick={(event) => {
                    captureOrigin(event.currentTarget);
                    if (browse()) setSettingsOpen(true);
                  }}
                >
                  <Settings2 size={19} />
                </button>
                <Menu.Root>
                  <Menu.Trigger asChild>
                    <button
                      className="profile-trigger"
                      aria-label={`Profile: ${currentProfile?.name || "Choose profile"}`}
                    >
                      <span className="avatar">
                        {currentProfile?.name.slice(0, 1).toUpperCase()}
                      </span>
                      <ChevronDown size={13} />
                    </button>
                  </Menu.Trigger>
                  <Menu.Portal>
                    <Menu.Content
                      className="menu glass"
                      sideOffset={15}
                      align="end"
                    >
                      <Menu.Label className="menu-label">Profiles</Menu.Label>
                      <Menu.RadioGroup
                        value={session.profileId}
                        onValueChange={(id) => void switchProfile(id)}
                      >
                        {session.profiles.map((profile) => (
                          <Menu.RadioItem
                            className="menu-item"
                            key={profile.id}
                            value={profile.id}
                          >
                            <span
                              className="avatar avatar-small"
                              aria-hidden="true"
                            >
                              {profile.name.slice(0, 1).toUpperCase()}
                            </span>
                            {profile.name}
                            <Menu.ItemIndicator>
                              <Check size={15} />
                            </Menu.ItemIndicator>
                          </Menu.RadioItem>
                        ))}
                      </Menu.RadioGroup>
                      <Menu.Item
                        className="menu-item"
                        onSelect={() => {
                          setProfileError("");
                          setAddProfile(true);
                        }}
                      >
                        <Plus size={16} /> Add profile
                      </Menu.Item>
                      <Menu.Separator className="menu-separator" />
                      <Menu.Item
                        className="menu-item"
                        onSelect={async () => {
                          cancelOpening();
                          window.dispatchEvent(new Event("player-save"));
                          usePlayer.getState().close();
                          await flushProgress();
                          try {
                            await api("/api/v1/logout", { method: "POST" });
                            reset();
                          } catch (error) {
                            setNotice(message(error));
                          }
                        }}
                      >
                        <LogOut size={16} /> Sign out
                      </Menu.Item>
                    </Menu.Content>
                  </Menu.Portal>
                </Menu.Root>
              </div>
            </header>
            <Suspense fallback={<Loading />}>
              {settingsOpen ? (
                <Settings
                  onClose={() => setSettingsOpen(false)}
                  onPreferences={setPreferences}
                  onDirtyChange={setSettingsDirty}
                />
              ) : show ? (
                <ShowPage
                  key={show.id}
                  show={show}
                  onBack={() => {
                    setShow(null);
                    window.scrollTo({ top: 0 });
                  }}
                  onPlay={(item) => void onPlay(item)}
                />
              ) : (
                <main className="browse-page">
                  {kind === "all" && !collectionView ? (
                    <>
                      <div className="page-heading content-width">
                        <div>
                          <h1>Home</h1>
                          <p>
                            {currentProfile?.name
                              ? `${currentProfile.name}’s library`
                              : "Your library"}
                          </p>
                        </div>
                      </div>
                      {historyError && (
                        <div
                          className="content-width inline-error"
                          role="alert"
                        >
                          Continue watching could not load: {historyError}
                          <button
                            onClick={() => setHistoryRevision((n) => n + 1)}
                          >
                            Try again
                          </button>
                        </div>
                      )}
                      {homeError ? (
                        <Empty title="Home could not load">
                          {homeError}
                          <button
                            className="secondary-button"
                            onClick={() => setRevision((n) => n + 1)}
                          >
                            Try again
                          </button>
                        </Empty>
                      ) : !hub || !preferences ? (
                        <Loading />
                      ) : preferences.home_sections.length === 0 ? (
                        <div className="content-width">
                          <Empty title="Make Home yours">
                            Choose which library rows appear here.
                            <button
                              className="secondary-button"
                              onClick={() => setSettingsOpen(true)}
                            >
                              Open settings
                            </button>
                          </Empty>
                        </div>
                      ) : (
                        preferences.home_sections.map((section) =>
                          section === "continue" ? (
                            <ContinueWatching
                              key={section}
                              items={continued}
                              onPlay={(item) => void onPlay(item)}
                            />
                          ) : (
                            <Curated
                              key={section}
                              hub={hub}
                              section={section}
                              onOpen={onOpen}
                              onCollection={openCollection}
                            />
                          ),
                        )
                      )}
                      {!loading && movies.length + shows.length === 0 && (
                        <Empty title="No media yet">
                          An administrator can add media folders and scan the
                          library in Settings.
                        </Empty>
                      )}
                      {error && (
                        <div
                          className="content-width inline-error"
                          role="alert"
                        >
                          {error}
                          <button onClick={() => setRevision((n) => n + 1)}>
                            Try again
                          </button>
                        </div>
                      )}
                    </>
                  ) : (
                    <section
                      id="library"
                      className="library-section content-width"
                      aria-busy={collectionView ? collectionLoading : loading}
                    >
                      {collectionView && (
                        <button
                          className="back-link"
                          onClick={() => {
                            setCollectionView(null);
                            setQuery("");
                            setGenre("");
                          }}
                        >
                          Back to library
                        </button>
                      )}
                      <div className="page-heading">
                        <div>
                          <h1>
                            {collectionView?.title ||
                              (kind === "movie"
                                ? "Movies"
                                : kind === "anime"
                                  ? "Anime"
                                  : "Series")}
                          </h1>
                          <p>
                            {(collectionView
                              ? collectionItems.length
                              : kind === "movie"
                                ? counts.movie
                                : kind === "anime"
                                  ? collection.length
                                  : counts.show
                            ).toLocaleString()}{" "}
                            titles
                          </p>
                        </div>
                      </div>
                      <div className="library-toolbar">
                        <label className="search-box">
                          <Search size={18} />
                          <input
                            ref={search}
                            aria-label="Search your library"
                            placeholder="Search titles"
                            value={query}
                            onChange={(e) => setQuery(e.target.value)}
                          />
                          {query && (
                            <button
                              aria-label="Clear search"
                              onClick={() => {
                                setQuery("");
                                search.current?.focus();
                              }}
                            >
                              <X size={16} />
                            </button>
                          )}
                        </label>
                        <label>
                          <span className="sr-only">Genre</span>
                          <select
                            aria-label="Genre"
                            value={genre}
                            onChange={(e) => setGenre(e.target.value)}
                          >
                            <option value="">All genres</option>
                            {genres.map((g) => (
                              <option key={g.id} value={String(g.id)}>
                                {g.name}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label>
                          <span className="sr-only">Sort by</span>
                          <select
                            aria-label="Sort by"
                            value={sort}
                            onChange={(e) => {
                              setSort(e.target.value);
                              try {
                                localStorage.setItem(
                                  `kodiac-sort:${session.user?.id}`,
                                  e.target.value,
                                );
                              } catch {
                                /* Storage may be disabled. */
                              }
                            }}
                          >
                            <option value="title">Title: A–Z</option>
                            <option value="title-desc">Title: Z–A</option>
                            <option value="year">Year: newest first</option>
                            <option value="rating">
                              Rating: highest first
                            </option>
                          </select>
                        </label>
                      </div>
                      {(error || collectionError) && (
                        <div className="inline-error" role="alert">
                          {collectionError || error}
                          <button onClick={() => setRevision((n) => n + 1)}>
                            Try again
                          </button>
                        </div>
                      )}
                      {(collectionView ? collectionLoading : loading) &&
                      !libraryItems.length ? (
                        <Loading />
                      ) : (
                        <VirtualLibrary items={filtered} onOpen={onOpen} />
                      )}
                      {loading && libraryItems.length > 0 && (
                        <div className="catalog-loading" role="status">
                          <LoaderCircle size={14} className="spin" /> Loading
                          remaining titles…
                        </div>
                      )}
                    </section>
                  )}
                </main>
              )}
              {lastMovie.current && (
                <MovieSheet
                  key={lastMovie.current.id}
                  open={Boolean(movie)}
                  movie={lastMovie.current}
                  onClose={() => setMovie(null)}
                  onPlay={(item) => void onPlay(item)}
                  collections={hub?.collections || []}
                  onCollectionChanged={() => setRevision((n) => n + 1)}
                />
              )}
            </Suspense>
          </>
        )}
      </div>
      {session.user && (
        <>
          <CommandPalette
            open={paletteOpen}
            onClose={() => setPaletteOpen(false)}
            onOpen={onOpen}
            hub={hub}
            onCollection={openCollection}
          />
        </>
      )}
      <Player />
      {playBusy && (
        <div className="toast" role="status">
          <LoaderCircle size={16} className="spin" /> Preparing playback…
        </div>
      )}
      {notice && (
        <div className="toast" role="alert">
          {notice}
          <button
            onClick={() => setNotice("")}
            aria-label="Dismiss notification"
          >
            <X size={16} />
          </button>
        </div>
      )}
      <Dialog.Root open={addProfile} onOpenChange={setAddProfile}>
        <Dialog.Portal>
          <Dialog.Overlay className="dialog-overlay" />
          <Dialog.Content className="profile-dialog glass">
            <Dialog.Close
              className="sheet-close icon-button"
              aria-label="Close profile dialog"
            >
              <X size={20} />
            </Dialog.Close>
            <span className="avatar profile-avatar">+</span>
            <Dialog.Title>Add profile</Dialog.Title>
            <Dialog.Description>
              Profiles keep watch progress separate within this account.
            </Dialog.Description>
            <form
              onSubmit={async (event) => {
                event.preventDefault();
                setProfileBusy(true);
                setProfileError("");
                const data = new FormData(event.currentTarget);
                try {
                  const profile = await api<Profile>("/api/v1/profiles", {
                    method: "POST",
                    body: JSON.stringify({ name: data.get("name") }),
                  });
                  useSession.getState().set({
                    profiles: [...useSession.getState().profiles, profile],
                  });
                  setAddProfile(false);
                  await switchProfile(profile.id);
                } catch (error) {
                  setProfileError(message(error));
                } finally {
                  setProfileBusy(false);
                }
              }}
            >
              <label>
                Profile name
                <input
                  name="name"
                  placeholder="e.g. Guests"
                  maxLength={80}
                  required
                  autoComplete="off"
                />
              </label>
              {profileError && (
                <p className="inline-error" role="alert">
                  {profileError}
                </p>
              )}
              <button className="primary-button" disabled={profileBusy}>
                {profileBusy ? "Creating…" : "Create profile"}
                <ArrowRight size={16} />
              </button>
            </form>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}
