//! Provider（供应商）写盘逻辑模块。
//!
//! Provider（供应商）写盘逻辑模块。
//!
//! 后端 `provider` module：`store`（DB CRUD）与 `sync` 分别在
//! `db/store_providers.rs` 与后续 ticket；本模块目前只承载 `live`——把供应商的
//! settingsConfig 合并写进用户本机 `~/.claude/settings.json`（写盘语义：
//! 只合并受控字段、非受控字段原地保留、备份 + 原子写）。`model_fetch` /
//! `snippet` 是后续 ticket 的模块。

pub mod live;
