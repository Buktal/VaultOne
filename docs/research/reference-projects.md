# 参考项目调研

> 调研日期：2026-07-15。目的：为 VaultOne（基于 GitHub 仓库的 LLM Usage 多设备同步看板）确定可借鉴的采集 / 存储 / 看板范式，并定位 VaultOne 的真正差异点。
> 数据来源：`E:\Project\AI\GitHub\CodeBurn`、`E:\Project\AI\GitHub\CC-Switch`。

## 一句话定位

- **CodeBurn**：纯本地、CLI 形态的 Token 用量 / 成本监控。采集范式 = 为每个 AI 工具写一个 provider（discover + parse），解析本地 session 日志。零 API key、零代理、零自有数据库。
- **CC-Switch**：Tauri 2 + React + Rust + SQLite 的桌面应用，核心是**多供应商配置切换**，附带本地代理拦截 + session 日志双源 usage 跟踪、WebDAV/S3 多设备同步。
- **VaultOne**：Tauri 桌面，采集借鉴 CodeBurn（session 日志解析），独特性 = **以 GitHub 仓库（文本 JSONL）做多设备同步后端**——两者都没做。

## 对比矩阵

| 维度 | CodeBurn | CC-Switch | VaultOne（目标） |
|---|---|---|---|
| 形态 | CLI（Ink/React 终端 + Vite/React Web） | Tauri 2 + React + Rust + SQLite | Tauri + React |
| 采集 | 纯 session 日志解析，~32 工具各一个 provider | 本地代理拦截 + session 日志双源，`data_source` 区分 | session 日志解析（见 ADR-0001） |
| 本地存储 | 无自有库，多级缓存，每次重解析 | SQLite：`proxy_request_logs` / `usage_daily_rollups` / `model_pricing`(200+) | 待定 |
| 同步 / 多设备 | OTLP 推自建后端（opt-in） | WebDAV / S3（同步二进制 `db.sql` + `skills.zip`） | GitHub 仓库（文本 JSONL） |
| 看板 | Recharts（Web） | Recharts + 实时事件推送 | 待定 |
| 定价 | LiteLLM 上游 + 本地缓存 + 快照兜底 + 用户覆盖（USD/1M token） | 内置 `model_pricing` 表（200+ 模型）+ 用户配置 | 待定（BLUEPRINT 已设计用户配置 UI） |

## CodeBurn 可借鉴点

- **provider 范式**：每个工具一个 provider，统一 discover（发现日志）+ parse（解析）接口。Claude Code provider 读 `~/.claude/projects/<sanitized-path>/<session-id>.jsonl`。
- **定价链**：LiteLLM 上游（TTL 24h）+ 打包快照 + 用户 `priceOverrides`（USD/1M token）。缺省 cache 启发式：cache write = 1.25× input，cache read = 0.1× input。
- 关键文件（在 `E:\Project\AI\GitHub\CodeBurn`）：`src/providers/index.ts`（provider 注册）、`src/providers/claude.ts`（Claude 路径）、`src/parser.ts`（通用解析）、`src/models.ts`（定价）、`src/types.ts`（`ParsedApiCall` / usage 结构）。

## CC-Switch 可借鉴点

- **Rust + SQLite 核心架构**，配置写入 CLI 原生路径（`~/.claude/`、`~/.codex/`、`~/.gemini/`）。应用数据在 `~/.cc-switch/`。
- **usage 数据模型**（最值得借鉴）：`proxy_request_logs`（按请求：token 四件套 + 成本 + 延迟 + TTFT + 状态 + model + `data_source`）、`usage_daily_rollups`（按日聚合）、`model_pricing`（200+ 模型预置价）。schema 在 `src-tauri/src/database/schema.rs`。
- **双源采集**：本地代理（`127.0.0.1:15721`）+ session 日志解析（`session_usage*.rs`），`data_source` 字段区分来源。
- **GitHub 交互**：仅用于 Copilot OAuth Device Flow（client_id `Iv1.b507a08c87ecfe98`），**无任何 git 操作**（grep `git2` / `Command::new("git")` 零命中）。token 存 `~/.cc-switch/copilot_auth.json`，原子写入。
- **云同步**：基于 manifest 同步 `db.sql` + `skills.zip`，SHA-256 校验 + 协议版本 + 同步锁。另有「自定义配置目录」指向 Dropbox / iCloud / NAS。
- 关键文件（在 `E:\Project\AI\GitHub\CC-Switch`）：`src-tauri/src/database/schema.rs`（全表结构）、`src-tauri/src/proxy/`（代理 + usage 捕获）、`src-tauri/src/services/session_usage*.rs`（session 日志解析）、`src-tauri/src/services/webdav_sync.rs` / `s3_sync.rs`（同步）。

## Provider 借鉴来源映射（多工具扩展时）

MVP 只接 Claude Code；扩展其他工具时，session 解析逻辑直接从两份源码移植，不必从零写。两份互补：

| 工具 | CodeBurn（TS） | CC-Switch（Rust） | 扩展时首选 |
|---|---|---|---|
| Claude Code | `src/providers/claude.ts` | `services/session_usage*.rs` | 两份都有 |
| Codex | `src/providers/codex.ts` | `services/session_usage*.rs` + `codex_history_migration.rs` | 两份都有 |
| Gemini CLI | `src/providers/gemini.ts` | `services/session_usage*.rs` | 两份都有 |
| OpenCode | `src/providers/opencode.ts`（SQLite） | `services/session_usage*.rs`（SQLite） | 两份都有 |
| Cursor | `src/providers/cursor.ts`（SQLite 只读，`sqlite.ts`） | — | CodeBurn |
| Grok / Kimi / Copilot / Cline / Kiro 等长尾 | 各有 `src/providers/*.ts` | — | 仅 CodeBurn |

> **关键前提**：借鉴方式取决于采集层语言。VaultOne 后端是 Rust（Tauri），所以 **Claude / Codex / Gemini / OpenCode 优先移植 CC-Switch 的 Rust 实现**（同语言，数据模型也接近 `proxy_request_logs`）；CodeBurn 的 TS 版作「字段映射 / 解析算法」参考，或后续在 Rust 侧用 JS 沙盒复用（CC-Switch 已用 `rquickjs`）。这一点在第 3 问（运行时 / 存储形态）定。

## ⚠️ 注意

CC-Switch 仓库的 README / CHANGELOG 引用了 2026 年的模型与定价（"Claude Fable 5"、"GPT-5.6"、"GLM-5.2" 等）。**架构可参考，但预置的 `model_pricing` / 模型清单当数据看，移植时需替换 / 校验。**
