//! Provider（供应商）写盘逻辑模块。
//!
//! 后端 `provider` module：`store`（DB CRUD，`db/store_providers.rs`）、
//! `sync`（per-device `providers.json` 结构同步，本目录 `sync.rs`）、`live`
//! （把供应商的 settingsConfig 合并写进用户本机 `~/.claude/settings.json`——
//! 写盘语义：只合并受控字段、非受控字段原地保留、备份 + 原子写），以及
//! `export_import`——全部供应商导出 / 导入一份 JSON 文档（手动迁移，不走
//! git 同步）。`model_fetch` / `snippet` 是后续 ticket 的模块。

pub mod export_import;
pub mod live;
pub mod sync;
