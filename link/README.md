# link

Rudy operator console frontend. Vite + React 19 + TypeScript, styled with
Tailwind v4 + shadcn/ui, state via TanStack Query, routing via TanStack
Router, telemetry streaming via WebTransport.

See [ADR-0004](../docs/decisions/0004-operator-console.md) for architecture.

## Develop

```bash
cd link
npm install

# Point at a locally-running rudyd (http://127.0.0.1:8443).
npm run dev
# -> http://localhost:5173
```

WebTransport requires HTTPS in the browser, so dev-mode Vite (plain HTTP) does
not get live WT streaming. The SPA falls back to REST polling in that case
(see `TelemetryGrid`).

## Build

```bash
npm run build       # -> link/dist/
```

`cargo build -p rudyd` runs the `link/dist/` -> `crates/rudyd/static/` copy in
`build.rs`, and `rust-embed` bakes it into the `rudyd` binary. Single-binary
Pi deploys therefore only need `rudyd` + the TOML config + the Tailscale cert.

## Add a shadcn/ui component

This project uses the shadcn/ui MCP server (`user-shadcn`). In Cursor, ask the
agent to add a component and it will call the MCP tool, which will install the
component into `src/components/ui/` using this repo's `components.json`.

Manual fallback:

```bash
npx shadcn@latest add button input dialog
```

## Regenerate wire types

The canonical wire types live in `crates/rudyd/src/types.rs` (and related
Rust modules) and are exported to `src/lib/types/` via `ts-rs` when you run
`npm run gen:types` (runs `cargo test -p rudyd export_bindings` in `crates/` and normalizes serde_json imports).

## Layout

```
src/
  main.tsx                 entry + router + query
  index.css                tailwind v4 + shadcn CSS vars
  routes/                  file-based TanStack Router routes
    __root.tsx
    index.tsx              /-> /telemetry (with auth gate)
    login.tsx              /login
    _authed.tsx            layout route: AppShell + auth guard
    _authed.telemetry.tsx  /telemetry
    _authed.params.tsx     /params
    _authed.jog.tsx        /jog    (Phase 2 stub)
    _authed.viz.tsx        /viz    (Phase 2 stub)
    _authed.logs.tsx       /logs   (Phase 2 stub)
  components/
    app-shell.tsx
    telemetry-grid.tsx
    coming-soon.tsx
    ui/                    shadcn components (installed on demand)
  lib/
    api.ts                 bearer-token fetch wrapper
    auth.ts                token store (localStorage)
    query.ts               TanStack QueryClient
    utils.ts               cn() helper (shadcn)
    wt.ts                  WebTransport client hook
  lib/types/               ts-rs-generated wire types (see above)
```
