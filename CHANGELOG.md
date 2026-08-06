# Changelog

All notable changes to VaultOne are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.7.1] - 2026-08-06

### Added

- **Update checks on launch** — VaultOne probes GitHub Releases for a newer version on every launch and re-checks every 6 hours while it stays open; the footer shows when one is available.

- **Drag to reorder groups** — grab any custom group in the sessions sidebar and drag it to a new spot; the list keeps your order, and on the Favorites tab that order follows the group to every other device. Order lives per track — a `position` column in the local database for local groups, a `position` field in the synced-groups artifact for the Favorites tab — and new groups always land at the end. The All / Ungrouped rows stay pinned.

### Changed

- **Roomier minimum window** — the main window's floor rises to 840×600 so the session list never cramps against the group sidebar; the session card title now ellipsizes on narrow windows.

- **Cleaner group sidebar** — group rows drop their folder icons and the sidebar narrows, giving the session table more room.

- **Session detail sheet polish** — the title's rename trigger is just the title plus a pencil icon (blank space no longer starts editing), the source renders as a tag chip, the favorite button matches the group picker's height, and chat bubbles cap at 80% width on narrow windows while keeping the 72ch line-length cap wide.

## [1.7.0] - 2026-08-06

### Added

- **Copy buttons on every message** — hover (or keyboard-focus) a message row in a session's transcript and a copy button appears; clicking it puts the raw text on your clipboard with a momentary checkmark.
- **Window-tracking detail sheet** — the session detail panel now spans ~70% of the window (`100vw - 32rem`), leaving the sidebar and the title column of the list visible behind it, so you always know which session is open.

### Changed

- **Three-voice transcript layout** — assistant messages float left, user messages float right as a mirrored bubble (corner cut toward the edge), tool and system rows span the full width in the middle as the workbench. Position alone tells you who spoke.
- **Tighter message headers** — icon, time, and model badge share one line; on user messages the group mirrors to the bubble's right edge so the time sits flush against it, aligned with the edge of the sheet.
- **Collapsible messages, quiet tool rows** — every message collapses on click (expanded by default); tool rows collapse to their tool name by default, and tool output is styled as a monospace code panel.
- **Aligned group counts** — the group sidebar's counts always sit flush right, matching the plain All / Ungrouped rows; the edit menu slides the count aside on hover instead of occupying space at rest.

### Fixed

- **Tool rows cleaned up** — the duplicated icon pair is gone, and a tool without a name truncates its content's first line instead of stretching the row.

## [1.6.0] - 2026-08-06

### Added

- **Sessions browser** — a new side-navigation entry that turns the raw session logs your AI CLIs write into a browsable, searchable history. Every session sits under its project directory with its full transcript, per-session token breakdown, and cost (computed live from the usage records — nothing is double-stored). Filter by time range, source, model, and device; search titles and paths; rename a session in place; open any session in a detail panel with a color-coded transcript (assistant model badges, distinct user turns, collapsible tool calls).
- **Two tabs, two ways to organize** — a **Local** tab lists every session collected on this machine, sorted into private groups that never leave it; a **Favorites** tab lists the sessions you favorited across all devices, sorted into synced groups shared everywhere, each entry marked with its source device. The same session can sit in different groups in each tab.
- **Favorites sync across devices** — starring a session publishes its transcript and synced-group placement through your sync repo; unstarring removes it everywhere. Only favorited sessions ever leave your machine — everything else stays local.
- **Transcripts for every session, instantly** — all conversation text is stored in the local database at collect time, so any session — favorited or not — opens its full transcript without re-reading a log file that may still be mid-write.
- **Faster collection** — a 5-second collect interval joins the scheduler presets for near-real-time dashboards.

### Changed

- **Sync rebuilt on dirty-day tracking** — collect now writes the local store only and marks each affected day dirty in the same transaction; push regenerates that day's artifact deterministically from the store (byte-stable, not append) and clears the dirty marks only after the push lands. The old JSONL-first write order, the with-own-data snapshot/restore protection, and the artifact-gap reconciler are gone — one write path, so two devices can never disagree on a file's content.
- **Session snapshots are derived, not written** — a favorited session's synced snapshot is recomputed from the store on push; a session whose source log has vanished is reclaimed automatically on every device (local rows and synced snapshot both removed).
- **Sync scope narrowed** — the optional sync repo now carries usage, favorited sessions, and library files only. The Sync-config feature (syncing app settings through the repo) shipped in 1.5.x is removed; settings are per-device again.
- **Smarter project grouping** — a session's project directory is derived from the most frequent working directory across its events (the mode, not the first entry), so a session that starts inside a subdirectory is grouped under the real project root.

### Fixed

- **Session collection robustness** — a log file that ends mid-character (a session still being written, e.g. in Chinese) is read lossily instead of dropped, so an in-progress session no longer loses its whole file; the scan cursor advances correctly, titles follow renames, and missing sessions are recovered.
- **Unfavorited sessions can now be viewed** — previously opening one asked you to favorite it first, because its transcript lived only in the favorited snapshot; the transcript now comes from the local database, so every session opens, favorited or not.
- **Favorite state no longer flickers** — a refetch no longer falls back to a stale snapshot, so a session you just favorited stays favorited, and the star icon matches the table row.
- **Session layout** — the "New group" button stays visible and the group sidebar no longer overflows its area.

### Internal

- **The largest architecture batch yet** — the db god-module was split into domain modules (`store_*`), 22 commands migrated out of domain modules into `commands.rs`, and single sources of truth were consolidated across the board (model normalization, price matching, device registry, provider parsing, UI formatting, date-range chips); the sync god-module was split into git primitives + flow orchestration, and a review pass cleaned up remaining drift. No user-visible change — the dashboard's code is measurably simpler to extend.

## [1.5.1] - 2026-07-31

### Fixed

- **Sync self-heals when a local commit duplicates a remote patch** — the 1.5.0 rebase self-heal aborted if a local commit duplicated a patch already on the remote (e.g. the same device-cleanup run on two machines), surfacing `rebase onto remote tip would conflict ... this patch has already been applied` and leaving the device stuck diverged again. `pull` now drops already-applied commits during the rebase and continues, so the divergence self-heals instead of stalling.
- **Usage rows no longer silently drop out of sync** — ingest wrote SQLite before the JSONL Artifact and treated the Artifact as a mere backup, swallowing append errors. A row that hit the DB but missed the Artifact (a transient append failure, or residue from ≤1.3.x) was then locked out forever — the ledger dedup silenced every later collect, so peers pulling the Artifact never saw it (one device showed ~24M tokens while a peer showed ~30M under the same filter). Ingest now appends the JSONL Artifact first and idempotently, and propagates append errors, so a failed append leaves the scan cursor untouched and the next collect re-parses the same source lines from the AI CLI logs. A new pre-collect reconcile also clears the cursors when the store holds rows the Artifact is missing, so a single rescan backfills pre-existing gaps — devices converge without manual repair.

## [1.5.0] - 2026-07-31

### Added

- **Grok CLI** — reads token usage from Grok Build's session logs, making it the fifth supported AI CLI (overlooked in this release's notes; documented retroactively).

### Changed

- **Settings layout** — the standalone Cloud-config section merges into Sync: the "Sync config" button and conflict resolver now sit beside "Sync now" under a single Sync card. Section order is now General / This machine / Devices / Sync / Maintenance, and the "Sync cloud config" button reads "Sync config".

### Fixed

- **Sync self-heals after a diverged push** — when a device lost a push race (a peer pushed between its own last pull and push), every "Sync now" / "Sync config" failed with `pull would diverge on 'main'; refusing to auto-merge` and could never recover on its own, leaving the dashboard on stale pulled data. `pull` now rebases the device's local-only commits onto the remote tip and pushes, auto-healing the divergence. Device isolation (`data/<deviceId>/`) keeps the rebase conflict-free, so both devices' data survive on the remote — a soft/reset-only fix would have replayed the local tree verbatim and clobbered the peer's data.
- **Trend chart for a single past day** — selecting a single past day (e.g. 2026-07-30 → 2026-07-30) collapsed the usage trend to a flat zero line: the chart zero-filled *today's* hours instead of the selected day, so the real records never matched. It now fills the selected day's full 24h axis (00:00 → 23:00; the current day stops at the current hour).

## [1.4.0] - 2026-07-30

### Added

- **Library — per-device file relay** — drag files or directories onto the window to upload (= a push into the device's subtree of the sync repo), drill into nested directories (upload, export, and single-file download all work at every depth), preview a file in-app (images fit-to-width with ctrl+wheel zoom; everything else in a sandboxed iframe), and export to a path of your choice. Upload is the only automatic direction — export stays manual and never writes into an AI tool's own config dir. Same-name same-kind overwrites (git history is the safety net); same-name different-kind is rejected. Forgetting a peer offers to migrate its files into yours under `from-<peer>/`, or delete them.

### Changed

- **Picker labels** — the logs and dashboard device / source / model dropdowns drop the dim `.`-prefixed placeholder; the "all" option now reads its full label (All devices / All sources / All models), and the date-range chip collapses same-day ranges and no longer wraps in the narrow dashboard column.

### Fixed

- **Usage filter** — the dashboard filter now persists the time-range preset (today / 7d / 30d / all / custom) instead of concrete dates, so a "today" selection no longer reads back as "yesterday" after midnight. Legacy rows without a preset fall back to "custom" with their literal dates.
- **Sync dedup** — `usage_records` / `turn_durations` / `ledger` used `uuid` as a global primary key, so the same source event replayed under two device ids collapsed into one row and could attribute one device's data to another. Dedup is now keyed on `(uuid, device_id)` (existing single-column PK migrated to a composite key, row counts preserved), and binding a sync repo pulls immediately so peer devices appear without a restart.
- **Window minimum size** — the morphing main window could restore or snapshot at the glance card's small size. The full dashboard now enforces a 720×520 minimum (the lightweight dock clears it; full restore re-applies it), and a stale sub-minimum rect self-heals on the next restore.

## [1.3.1] - 2026-07-28

### Changed

- **Source filter visibility** — the Source dropdown now renders whenever any source data exists, so a single-source user still sees the filter (previously it required ≥2 collected sources).
- **Filter chip sizing** — the logs control bar now sizes its filter chips by typical content (model `w-48`, source `w-40`, device `w-36`) instead of a uniform width; the dashboard card column stays uniform at `w-36`.

### Fixed

- **Release completeness** — `v1.3.0` was tagged two commits early, so the Source-filter and chip-sizing changes above never shipped in the 1.3.0 installers. `v1.3.1` tags the current `main` to include them.

## [1.3.0] - 2026-07-28

### Added

- **Multi-CLI usage collection** — usage collection now spans four AI CLIs that write to local logs: Claude Code (`~/.claude`), Codex (`~/.codex`), Gemini CLI (`~/.gemini`), and OpenCode (SQLite). Each source's token semantics are normalized into the same four-bucket model: Codex's cache-inclusive input becomes fresh input, Gemini's `thoughts` fold into output, OpenCode's `cache.write` maps to cache creation. Claude Code dedup now picks the best message-id snapshot (one with `stop_reason` set, else the largest output), so message_start snapshots no longer freeze and undercount output. Seed pricing added for gpt-5.5 / 5.4 / 5.2 (prefix-fallback covers `-codex` variants) and DeepSeek v3.x.
- **Device-scoped usage tracking** — filter the dashboard, the expanded lightweight card, and the tucked mini-bar by device. A unified device picker drives all three windows; the tucked bar's hover drawer opens on hover and lists devices on click.
- **Device lifecycle** — forget a peer device locally (clears its rows and artifact dir); stale peers with no git presence auto-clear within ~30s on both sync pull and the collect path, while a still-active peer self-heals on the next sync. Recent requests and the logs' Device column now show device names.
- **Persistent usage filter** — the time-range / model / device-scope filter survives app restarts.
- **Per-shape window geometry** — full, expanded, and tucked each remember their own placement and state, so switching shapes restores the last position instead of resetting.
- **System-proxy sync** — libgit2 push/fetch/clone/connect now follows the OS system proxy (env vars, then the Windows registry), so Synced-mode clients behind Clash/Mihomo or a corporate gateway no longer time out silently. Proxy changes apply on the next sync — nothing is cached.

### Changed

- **UI polish** — title bar raised to 36px; mid-window device selector tightened. Token columns carry a language-neutral `tok` unit in the header. Select popovers auto-fit content (never narrower than the trigger) and open top-aligned, fixing the jumping model dropdown.

### Fixed

- **Linux tray** — added a "Show" entry so the window can be restored from the tray on Linux, where the libappindicator/SNI backend never emits tray click events and left-clicking was a no-op. Windows/macOS left-click restore is unchanged.
- **Pricing seed** — a malformed seed literal now panics at startup instead of silently returning 0 and skewing every cost calculation.
- **Today trend** — the "today" trend now spans the full day instead of stopping at the current hour.
- **Request log** — switching the filter resets the log page, so you don't land on a stale, out-of-range page.

## [1.2.0] - 2026-07-24

### Added

- **Lightweight glance mode** — the main window morphs into a small, always-on-top "today" snapshot docked to the right screen edge. Two shapes reachable from one another: a tucked mini-bar that always shows today's token total, and an expanded card mirroring the dashboard's anchor. Switch full ⇄ expanded ⇄ tucked from any shape.
- **Multi-skin theming** — recolor the accent and chart palette across five skins (Neutral, Sage, Azure, Crimson, Mauve); Neutral (greyscale chrome) is the new default. Per-device, never synced.

### Changed

- **Usage trend** — the trend chart is now multi-line with data points instead of a single line, so each metric reads on its own.

### Fixed

- **Lightweight mode** — the entire tucked bar is draggable now (not just a tiny corner grip), and a press still distinguishes click-to-expand from drag-to-move.

## [1.1.0] - 2026-07-23

### Added

- **Auto-update** — check for new versions on launch (throttled to once per 24h) or manually from Settings; download and install signed installers straight from GitHub Releases, with Ed25519 signature verification and one-click relaunch. Distributed entirely through GitHub — no self-hosted server. On updater failure, a manual fallback opens the Releases page.
- **Display language** — switch the UI between English, 简体中文, and 日本語.

### Fixed

- **Lightweight mode** — edge-flush the tucked peek icon and smooth out the diagonal reveal animation.

## [1.0.0] - 2026-07-23

First public, open-source release.

### Added

- **Dashboard** — four-bucket token consumption (input / output / cache creation / cache read), cache-hit rate (`cache_read / (input + cache_creation + cache_read)`), total requests and total cost (USD, frozen at collection), dual-axis token-vs-cost usage trends, per-call request log (model, token breakdown, cost, turn duration, `stop_reason` / `service_tier` chips), and per-turn cost and wall-clock views.
- **Collection** — read-only parsing of Claude Code session logs (source logs are never modified), cursor-based incremental scan, tray-resident background scheduler. Pluggable provider architecture (Claude Code today, more planned).
- **Sync (optional)** — Standalone mode (full dashboard, zero network) and Synced mode (align usage across devices through a GitHub repository you own); plain-text artifacts partitioned by device and date (`data/<device>/usage-YYYY-MM-DD.jsonl`).
- **Cost & pricing** — editable per-model pricing overrides; rebill for records that had no price when collected, without re-costing existing history.
- **Experience** — lightweight glance mode (edge-tuck + hover-to-peek today's usage), custom title bar, light / dark theme, local-first and private by default.
- **Packaging** — cross-platform installers for Windows, macOS (Apple Silicon), and Linux, built automatically on tag push via GitHub Actions.

### Known limitations

- **macOS**: Apple Silicon (arm64) only; builds are unsigned — right-click → **Open** on first launch (or `xattr -dr com.apple.quarantine /Applications/VaultOne.app`). Intel Mac users can build from source.
- **Providers**: Claude Code only; additional providers (Codex, Cursor, …) are planned.

[Unreleased]: https://github.com/Buktal/VaultOne/compare/v1.6.0...HEAD
[1.6.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.6.0
[1.5.1]: https://github.com/Buktal/VaultOne/releases/tag/v1.5.1
[1.5.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.5.0
[1.4.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.4.0
[1.3.1]: https://github.com/Buktal/VaultOne/releases/tag/v1.3.1
[1.3.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.3.0
[1.2.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.2.0
[1.1.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.1.0
[1.0.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.0.0
