# FLWME frontend
React 19 + TypeScript + Vite. Pages: `/`, `/register`, `/dashboard`.

Authentication calls same-origin `/api/auth/*`; Rust backend runs on private port 18081 and Nginx serves the production Vite dist on private port 18080.

`npm ci && npm run build && npm run lint`

Registration requires a valid email and password of 10–128 characters. No tokens or passwords are stored in browser storage; sessions use Secure, HttpOnly cookies.
