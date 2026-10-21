# Security policy

Security fixes target the latest released version. Older versions have no
maintenance guarantee. This policy creates no response-time commitment.

## Private reporting

On the official GitHub repository, use **Security > Advisories > Report a
vulnerability** if private vulnerability reporting is enabled. If it is unavailable,
open an issue requesting a private contact without disclosing the vulnerability,
then wait for a maintainer-provided channel. No unverified security email address
is published here.

Include the affected version, platform, prerequisites, impact and a minimal
reproduction using synthetic media. Remove access tokens, passwords, TMDB keys,
private paths, personal metadata and database files. Coordinate disclosure with
the maintainer before publishing exploit details.

## Deployment model

Kodiac is intended for a private household network. Authentication is required
for catalogs and playback; server identity, health, login and web assets are public.
Use an HTTPS reverse proxy for untrusted networks and set `SECURE_COOKIE=true`
when the external service is HTTPS. Configure exact browser origins when needed.
The application does not terminate TLS or implement a multi-tenant isolation model.

Mount media read-only and keep the writable database/cache private. Initial
passwords are read from a file (preferred) or `ADMIN_PASSWORD`; the file only
bootstraps an empty account database. Password hashes use Argon2, session tokens
are stored as SHA-256 digests, and tokens expire after seven days. SQLite may also
contain plaintext TMDB credentials from administrator settings; protect backups
accordingly. No application-level database encryption is provided.

The server checks canonical media/cache containment, skips discovery symlinks,
rejects NFO declarations and bounds external process output, duration and
concurrency. These controls do not make arbitrary hostile media safe: keep FFmpeg,
OS libraries and drivers updated, and run the container as its unprivileged UID.
Do not use a privileged container to enable hardware access.

HTTP query tokens are supported for native media clients. Reverse proxies and
other infrastructure must avoid recording query strings. Check logs and screenshots
before sharing them. Keep private production artifacts outside the source tree.
