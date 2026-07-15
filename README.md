# VaultOne

**English** | [简体中文](./README.zh-CN.md)

> ⚠️ Early work in progress — not usable yet.

VaultOne is a desktop app that turns your local AI assistant session logs into a clear usage dashboard — tokens, cost, cache efficiency, trends — and syncs the data across your devices through a GitHub repository.

- **Read-only collection** from local session logs (Claude Code today; more providers planned).
- **Multi-device sync** using a GitHub repo as a lightweight, text-based backend.
- **Local dashboard** for querying usage across all your devices.

## Tech stack

- [Tauri](https://tauri.app/) (Rust backend) + [React 19](https://react.dev/) + [TypeScript](https://www.typescriptlang.org/)
- [Vite](https://vite.dev/) + [Tailwind CSS v4](https://tailwindcss.com/) + [shadcn/ui](https://ui.shadcn.com/)

## Prerequisites

- [Node.js](https://nodejs.org/) LTS and [Yarn](https://yarnpkg.com/)
- [Rust](https://www.rust-lang.org/) (stable) — follow the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS

## Getting started

```bash
yarn install        # install frontend dependencies
yarn tauri dev      # run the desktop app in development
```

## Scripts

| Command | Description |
| --- | --- |
| `yarn dev` | Run the web UI only (Vite). |
| `yarn tauri dev` | Run the full Tauri desktop app in development. |
| `yarn build` | Type-check and build the web bundle. |
| `yarn tauri build` | Build a release desktop binary. |
| `yarn lint` | Lint and check formatting (Biome). |
| `yarn format` | Auto-format (Biome). |

## License

[MIT](./LICENSE)
