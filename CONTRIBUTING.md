# Contributing

Use issues for reproducible bugs and concrete feature proposals. Keep changes
focused, explain the user-visible problem, and include validation appropriate to
the changed path. Discuss schema, authentication or streaming contract changes
before large implementations. Do not include personal libraries, artwork,
production paths, databases, binaries, credentials or machine-specific deployments.

## Development setup

Use Rust 1.99.1, Node.js 22.12+ or 24+, npm, Python 3 and FFmpeg/ffprobe.
The Rust package is `kodiac`; build its embedded SPA first:

```sh
npm ci --prefix web
npm --prefix web run build
cargo check --locked
```

Vite's development server proxies API traffic to `127.0.0.1:8096`:

```sh
npm --prefix web run dev
```

Start the backend with the native instructions in README, a temporary library and
a private initial password file. Do not point tests at a production database.

## Checks

```sh
npm --prefix web run format:check
npm --prefix web run build
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
# Real FFmpeg integration checks require ffmpeg and ffprobe on PATH:
cargo test --locked --test http -- --ignored
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo build --release --locked
python3 scripts/verify-relational-boot.py target/release/kodiac
cd web
npx playwright install chromium
npm run test:e2e
```

Browser tests launch an isolated real server with generated media and an artificial
catalog, then remove temporary files. Environment-dependent tests are explicitly
ignored in the default Rust run; run them separately and report actual skips.

## Code and review conventions

Document public Rust items with `///` and modules with `//!`. Describe units,
containment boundaries, cancellation, concurrency limits and meaningful errors.
Do not add performance claims without a reproducible benchmark and raw results.
Use rustfmt and Prettier. Keep comments, documentation and commit messages free
of emojis. Write imperative commit subjects describing the actual diff, optionally
with a scope such as `fix(streaming):` or `docs(api):`.

Use additive SQL migrations; never edit an already released migration. Preserve
media IDs, account ownership and playback history. Test migrations with synthetic
fixtures. Streaming changes must preserve bounds, child cleanup, permit lifetime
and the distinction between audio ordinals and absolute stream indices.

Include a clear change description, relevant test results and known limitations
in pull requests. New contributions are submitted under GPL-3.0-only. Retain
required dependency and inherited notices. Release history provenance is described
in [docs/RELEASE.md](docs/RELEASE.md); future work uses actual author/commit times.
