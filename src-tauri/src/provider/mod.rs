//! Provider（供应商）写盘逻辑模块。
//!
//! 后端 `provider` module（spec §4.9）：`store`（DB CRUD）与 `sync` 分别在
//! `db/store_providers.rs` 与后续 ticket；本模块目前只承载 `live`——把供应商的
//! settingsConfig 合并写进用户本机 `~/.claude/settings.json`（写盘语义见
//! ADR-0005）。`model_fetch` / `snippet` 是后续 ticket 的模块。

pub mod live;
