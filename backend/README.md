# FLWME authentication backend

Rust / Axum + PostgreSQL 16. Private bind `127.0.0.1:18081`.

- `POST /api/auth/register`: create user, Argon2id password hash, session cookie
- `POST /api/auth/login`: authenticate, issue new session
- `GET /api/auth/me`: verify active session
- `POST /api/auth/logout`: invalidate server-side session
- `GET /api/healthz`: health check

POST requests require Origin `https://flwme.kiyt.io`. Cookies are `Secure`, `HttpOnly`, `SameSite=Lax`, `__Host-` with a 12-hour TTL; only SHA-256 hashes of opaque session tokens are stored in PostgreSQL. Auth responses carry `Cache-Control: no-store`.

Migrations in `migrations/`; connection uses local PostgreSQL peer authentication as `flwme-dev` to dedicated `flwmenow` database. No `.env` or database password required. Backend starts as systemd unit `flwmenow-auth.service` under non-root `flwme-dev` user.

Development:
`cargo test && cargo build --release`

Production caveat: V1 limits invalid login attempts per email to five per 15 minutes. Before broad public signup, add registration/IP rate limits, email verification, password reset, and abuse monitoring.
