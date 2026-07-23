# VaultOne

> A local-first desktop dashboard for your Claude Code token usage and cost — read straight from the session logs Claude Code already writes, with optional multi-device sync through a GitHub repo you control.

[![Version](https://img.shields.io/github/v/release/Buktal/VaultOne?color=blue&label=version)](https://github.com/Buktal/VaultOne/releases)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/Buktal/VaultOne/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](./LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

**English** | [简体中文](./README.zh-CN.md) | [Changelog](./CHANGELOG.md)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./docs/images/dashboard-dark.png">
  <img src="./docs/images/dashboard-light.png" alt="VaultOne dashboard">
</picture>

*Lightweight glance mode — tuck to the screen edge, hover to peek today's usage:*

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./docs/images/lightweight-dark.png">
  <img src="./docs/images/lightweight-light.png" alt="Lightweight glance mode">
</picture>

---

## Why VaultOne?

Every time Claude Code runs, it writes session logs to disk. VaultOne turns those logs into a clear usage picture — **tokens, cost, cache efficiency, trends** — without you having to wire up a proxy, hand over an API key, or send anything anywhere.

Two things shape the whole product:

- **Local-first.** The dashboard works with zero network. Reading your own logs is all it needs.
- **Read-only.** VaultOne only ever *reads* the session logs; it never modifies them, and it never touches Claude Code's behavior. Claude Code keeps running exactly as before.

Multi-device sync exists, but it's a purely **opt-in** layer on top — never a precondition for using the app.

## Highlights

- **Reads logs your tools already produce** — parses Claude Code session logs straight off disk. No proxy, no API key, no network required to use the dashboard.
- **Multi-device sync through your own GitHub repo** — usage data is exported as plain text, partitioned by device and date, into a repository you own. No third-party service in the middle.
- **Token economics that match how you're actually billed** — four-bucket consumption (input / output / cache creation / cache read), cache-hit rate, and cost, captured and frozen at collection time.
- **Lightweight glance mode** — park it at the screen edge as a half-icon; hover for a peek at today's usage without opening the full dashboard.
- **Tray-resident background collection** — an incremental scanner keeps the dashboard fresh behind the scenes, no window required.
- **Per-call and per-turn views** — drill into every model call (cost, duration, `stop_reason` semantics) and roll up to whole-turn cost and wall-clock time.

## Download

Grab the installer for your OS from the **[Releases](https://github.com/Buktal/VaultOne/releases)** page.

| OS | Installer |
| --- | --- |
| **Windows** | `.msi` or `.exe` (NSIS) setup |
| **macOS** | `.dmg` (Apple Silicon, arm64) |
| **Linux** | `.deb`, `.AppImage` (`.rpm` where available) |

**First run:** launch VaultOne — it scans your local Claude Code session logs and the dashboard fills in. No account, no sign-in, no network. To see usage across multiple machines, enable sync in **Settings** and point VaultOne at a GitHub repository you control.

> **macOS note:** builds are currently unsigned. On first launch, right-click the app → **Open**, or strip the quarantine attribute:
> ```bash
> xattr -dr com.apple.quarantine /Applications/VaultOne.app
> ```

## Features

### Dashboard

- **Four-bucket token consumption** — input, output, cache creation, cache read.
- **Cache-hit rate** — `cache_read / (input + cache_creation + cache_read)`, aligned with how upstream usage is counted.
- **Requests & cost** — total request count and total cost (USD), frozen at collection time.
- **Usage trends** — dual-axis token-vs-cost chart over time, with toggleable series.
- **Per-call request log** — model, token breakdown, cost, turn duration, and `stop_reason` / `service_tier` chips.
- **Per-turn view** — whole-turn cost and wall-clock duration, separate from single-call timing.

### Collection

- **Read-only source** — parses the session logs Claude Code already writes; never modifies them.
- **Incremental scan** — a cursor-based scanner picks up only what changed.
- **Tray-resident background scheduler** — collects on a timer without keeping a window open.
- **Claude Code today, more providers planned** — the collector is built as a pluggable provider.

### Sync (optional)

- **Standalone mode** — full dashboard, zero network.
- **Synced mode** — align usage across devices through a GitHub repo you own.
- **Plain-text artifacts** — partitioned by device and date (`data/<device>/usage-YYYY-MM-DD.jsonl`), so diffs stay readable and reviewable.

### Cost & pricing

- **Editable per-model pricing** — override the seed prices; VaultOne uses your numbers.
- **Rebill** — backfill records that had no price when they were collected, without re-costing existing history.

### Experience

- **Lightweight glance mode** — edge-tuck + hover-to-peek today's usage.
- **Custom title bar, light / dark theme.**
- **Private by default** — usage data stays on your machines unless you opt into sync.

## How it works

```
  Claude Code session logs
          │  (read-only)
          ▼
       Collect ──────▶ Local store ──────▶ Dashboard
          │
          │  (optional · Synced mode)
          ▼
   Artifact (plain text, per device + date)
          │
    push / pull via your GitHub repo
          │
          ▼
     Other devices
```

- **Collect** reads local source logs, parses them, and writes the local store (and produces a sync artifact).
- **Local store** is the query layer the dashboard reads from.
- **Sync** (opt-in) pushes/pulls artifacts between devices through a GitHub repository.

## Build from source

**Prerequisites:** [Node.js](https://nodejs.org/) LTS + [Yarn](https://yarnpkg.com/), and [Rust](https://www.rust-lang.org/) stable with the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
yarn install        # install frontend dependencies
yarn dev            # run the desktop app in development
yarn dist           # build a release desktop binary
```

| Command | Description |
| --- | --- |
| `yarn dev` | Run the full Tauri desktop app in development (Vite + Rust). |
| `yarn web:dev` | Run the web UI only (Vite), for frontend-only iteration. |
| `yarn check` | All static checks — frontend (Biome + tsc) and Rust (fmt + clippy). Same gates as CI. |
| `yarn web:fix` | Auto-fix frontend lint and formatting (Biome). |
| `yarn web:build` | Type-check and build the web bundle. |
| `yarn test` | Run all tests (the Rust suite). |
| `yarn dist` | Build a release desktop binary. |

**Tech stack:** [Tauri 2](https://tauri.app/) (Rust) · [React 19](https://react.dev/) · [TypeScript](https://www.typescriptlang.org/) · [Vite](https://vite.dev/) · [Tailwind CSS v4](https://tailwindcss.com/) · [shadcn/ui](https://ui.shadcn.com/) · [Redux Toolkit](https://redux-toolkit.js.org/) · [Recharts](https://recharts.org/)

## Architecture

A [Tauri 2](https://tauri.app/) app: a Rust backend handles collection, the local store, and optional Git-repo sync, while a React frontend renders the dashboard through generated, type-safe IPC bindings. The collector is a pluggable provider model (Claude Code today), the local store is the dashboard's single read source, and sync is an opt-in projection of that store into plain-text artifacts partitioned by device and date.

## Contributing

Issues and suggestions are welcome. Before opening a PR, please run `yarn check` and `yarn test` so CI gates pass locally. For larger features, open an issue to discuss the approach first.

## License

[MIT](./LICENSE) © VaultOne Contributors
