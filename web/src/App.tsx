
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
  const visible = items.filter(item => item.title.toLowerCase().includes(query.toLowerCase()));
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

