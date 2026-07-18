# VaultOne 功能实现进度（交接文档）

> 交接日期：2026-07-16。`/mattpocock-skills:implement` 串行实现 issue #1–#8 的中断交接。
> **约束：全程不 commit / 不 push**（用户明令），所有改动留在工作树，待用户复核后再决定提交粒度。
> 关联 ADR：见 `docs/adr/`；领域术语见 `CONTEXT.md`；总目标见 `BLUEPRINT.md`。

## 一句话现状

**Standalone 闭环 + #5 git2 多设备同步 + #6 云配置手动同步（冲突提示）均已打通且可构建**（采集 → SQLite → cost → 看板；Synced 模式下 usage JSONL clone/pull/commit/push + 启动 pull / 退出 flush / 定时 push / 手动「立即同步」，SQLite↔JSONL uuid 去重互转，写库后 emit `usage_changed`；config/{app,user,pricing}.json 手动同步 + 双向冲突检测/逐文件解决）。剩余：终验清理（dead-code + db.rs/ingest.rs 既有 clippy lint）+ commit 决策。前端 `tsc`/`biome`/`vite build` 全绿；Rust `cargo check` 通过、`cargo test` **48 例全绿**、bindings 已重生成（含 `syncNow`/`syncConfig`/`resolveConfigConflict`）。

---

## 已完成且验证通过

| Issue | 状态 | 说明 |
|---|---|---|
| #1 骨架 | ✅（Rust+前端） | 见下「#1 落地明细」 |
| #2 schema | ✅ | `db_schema.sql` + `model.rs` |
| #3 Provider | ✅ | `providers.rs` ClaudeCodeProvider + trait |
| #4 Local Store + 去重 + cost | ✅ | `db.rs` + `ingest.rs` |
| #7 定价（核心） | ✅ | `pricing.rs`（seed + CostCalculator + LiteLLM 拉取） |
| #8 看板 | ✅ | dashboard / pricing / settings 三视图 |
| #5 git2 同步 | ✅ S2 完成 | 仅 Synced；S2a 原语（clone/pull/commit/push+PAT 回调）+ S2b 接入（启动 pull/退出 flush/定时 push/手动 sync_now + SQLite↔JSONL 互转 + emit 事件 + 前端 listen/UI）全落地 |
| #6 云配置同步+冲突 | ✅ S3 完成 | 仅 Synced；config 手动 push/pull + 冲突检测/逐文件解决（绝不 last-write-wins） |

### 验证结果

- `cargo check`：通过（仅 dead-code 警告，多为 #5 待用函数）。
- `cargo test`：**48 例全绿**（S1 35 + S2a 6 + S2b 3 + S3 4）。S1 覆盖 pricing（calc / normalize_key / resolve prefix-fallback + cache 启发式 / litellm parse / doc round-trip）、model（day / total / cache_hit_rate）、providers（assistant 解析 + 噪声跳过 + zero-token 丢弃 + 缺目录）、config（device_id 校验/生成/碰撞、Paths、mode、masked_token）、ingest（recordify cost / JSONL 往返 / 集成去重）、db（seed / ingest 去重 / stats / trend / logs 分页 / models / rollup / rebill / pricing CRUD）。db 用 `:memory:`，config/ingest 用 `tempfile` tempdir。S2a 新增 sync（6 例）：PAT credential 构造、require_synced 守卫、open_or_clone 幂等、ensure_repo clone+open、Standalone 拒绝、**两设备 push/pull 闭环**（本地 bare repo：fast-forward + 自有 artifact 在 pull 后存活）。**S2b 新增 sync 高层（3 例）**：pull_and_import 导入远端 artifact 到 Store（uuid 去重，re-pull no-op）、commit_and_push 无改动时 no-op、**sync_now 跨设备 roundtrip**（A 写+push，B sync_now 拉取见记录）。**S3 新增 sync 云配置（4 例）**：`sync_config` 双方都改 pricing 时检出冲突（不覆盖）、本地干净时拉取远端 pricing、`resolve_config_conflict` keep_remote 取远端版本、keep_local 推送本地版本（两端 bare repo 互验）。
- `VAULTONE_GEN_BINDINGS=1 cargo run --manifest-path E:/Project/D_VaultOne/src-tauri/Cargo.toml`：bindings.ts 重生成成功（含全部命令）。
- `./node_modules/.bin/tsc --noEmit`：0 错。
- `yarn lint`（biome check）：0 错（生成文件 bindings.ts 已加入 biome 忽略）。
- `yarn build`（tsc + vite）：成功（bundle ~856kB，recharts 偏大，桌面端可接受）。
- **GUI 未实跑**（无法在此环境开窗口）。boot 路径（ConfigStore::load + Store::open）经 `VAULTONE_GEN_BINDINGS` 早返回，**尚未在运行时验证**——靠 Rust 测试补。

### #1 落地明细

- Rust `config.rs`：`~/.config/vaultone/` 全目录初始化（config.json/vaultone.db/repo/{config,data/<deviceId>}/logs/）；deviceId 持久 12-hex 生成 + 冲突检测（查 repo/data 已有目录）+ 显示名映射；`ConfigStore`（Mutex，原子读写）；`mode()` 判定（repo_url+token 都有→Synced，否则 Standalone，ADR-0011）；token 脱敏。
- 前端骨架（ADR-0007）：RTK Query base api（no-op baseQuery + injectEndpoints + tagTypes Usage/Pricing/Device/App/Sync）；feature-based 目录（usage/pricing/settings）；viewSlice（dashboard/pricing/settings 三视图，无 react-router）；shell 侧栏 + 模式徽标；AppProviders（Redux Provider + Toaster）。

---

## 关键实现决策（**不要重新争论，照此继续**）

1. **命名全程 snake_case**（Rust ↔ SQLite ↔ JSONL ↔ TS DTO 字段），零跨边界 rename。
   - 注意：specta-typescript 把**命令函数名**转成 camelCase（`getAppInfo`、`queryUsageStats`），但 **DTO 字段是 snake_case**（`total_tokens`、`from_day`）——两端一致，无 mismatch。
2. **跨边界类型**：时间戳用 ISO8601 **String**（规避 specta 禁 BigInt 的 i64），SQLite 存 TEXT（UTC，可字典序排序）；**token 用 u32**；**cost 跨边界用 f64**（DB 内部 `rust_decimal::Decimal` 存 TEXT）。
3. **specta-typescript 把 f64 渲染成 `number | null`**（怪癖），前端一律 `?? 0` 兜。
4. **bindings 导出路径**：`lib.rs::export_bindings` 用 `env!("CARGO_MANIFEST_DIR")` 拼绝对路径（`../src/types/generated/bindings.ts`）。原因：`cargo run --manifest-path` 的 CWD 是项目根，相对路径 `../src/...` 会写到错位置。改 Rust 命令/DTO 后**必须重生成 bindings**（见下命令）。
5. **AppState 用 `Arc<Store>` / `Arc<ConfigStore>`**：`collect_now`/`fetch_litellm` 用 `spawn_blocking`，需 `'static`，不能 move 请求作用域的 `State<'_>`。
6. **RTK Query 避免 import 循环**：baseSplitApi 在 `app/store/api.ts`，feature 用 `injectEndpoints`；feature api 的 import 放 **`store.ts`** 触发注入（**不**放 api.ts——ES import 提升会导致 baseSplitApi 在注入前未定义）。
7. **LiteLLM 拉取用 `ureq`（rustls）**：纯 Rust、Windows 友好，放 `spawn_blocking`；per-token → per-million（×1e6）；离线兜底用 builtin seed。
8. **执行顺序**：先 Standalone 闭环（默认模式），#5/#6 git 层最后。
9. **去重**：`usage_records.uuid` PRIMARY KEY + 独立 `ledger` 表（uuid 批量 IN 查重 + provenance）；ingest 返回**新插入**记录供 JSONL 只追加新行。
10. **rollups**：ingest 后按受影响 (day,model,device) 重算 `daily_rollups`；stats/trend 查询直接对 usage_records 做 SUM（cost 列 `CAST AS REAL`，展示级精度）。
11. **git2 同步（ADR-0010，S2a）**：`git2 = "0.19" { default-features=false, features=["https"] }`——不带 ssh 减依赖，Windows TLS 走 libgit2 内置 Schannel（**本机落地验证：无需 cmake、无需 openssl**，文档原「vendored openssl」警告未发生）。PAT 经 `RemoteCallbacks::credentials` 进程内注入（`Cred::userpass_plaintext("x-access-token", token)`）+ 一次性 attempts 守卫防循环；git2 0.19 的 `RemoteCallbacks`/`Cred` **无生命周期参数**（持 `'static` 闭包），故 token clone 入闭包（rustc `mismatched_lifetime_syntaxes` 在此误报，已 `#[allow]`）。clone 后强制 `core.autocrlf=false`——否则 Windows/POSIX 间 CRLF 转换会破坏 JSONL 行式确定性（ADR-0004，测试已复现并修复）。pull 仅 fast-forward，diverge 拒绝（冲突留 S3）；checkout 用 `git2::build::CheckoutBuilder`（0.19 无 `CheckoutOptions`）。
12. **git2 接入与时机（ADR-0005，S2b）**：高层 API 组合 S2a 原语——`pull_and_import`（pull → `read_all_artifacts` → `store.ingest` uuid 去重）、`commit_and_push`（`has_changes` 守门，无改动 no-op）、`sync_now`（手动全流程）。时机（仅 Synced）：`lib.rs` setup 里 `std::thread::spawn` 启动 pull + ~10min 周期 push（用原生线程而非 tauri async sleep，git2 阻塞操作）；`RunEvent::ExitRequested` 同步 flush。`AppState` **不持 `Repository`**——每次 sync 操作 `open_or_clone`（open 轻量），规避 git2 `Repository` 跨线程 Send/Sync 复杂性。事件用原生 `app_handle.emit("usage_changed", ())`（非 tauri-specta 类型化 event——RC 阶段务实，ADR-0008 只预留缝，类型化待稳定后升级）；前端 `AppProviders` useEffect `listen` → `api.util.invalidateTags(['Usage'])`。clone 后 `autocrlf=false` + **force checkout**（修 CRLF：设 false 后旧 checkout 的工作树会误判 WT_MODIFIED，S2b 测试复现）。
13. **云配置同步（ADR-0005 / #6，S3，仅 Synced）**：`config/{app,user,pricing}.json` 走**手动** push/pull（非自动——共享文件自动同步必冲突，ADR-0005）。冲突 = 工作树 dirty 文件 ∩ 远端相对 HEAD 改动文件（`repo.diff_tree_to_tree(HEAD_tree, origin_tree)`）；命中则返回 `ConfigConflict` 列表给前端逐文件选 keep_local/keep_remote，**绝不 last-write-wins**。关键坑：git2 `checkout_head(SAFE)` 会把「远端已改、工作树仍是旧 HEAD 版本」的文件误判为本地修改而拒绝更新（incoming 静默丢失），force checkout 又会覆盖真正本地非冲突编辑——故用 `pull_preserving_dirty`：快照全部 WT_MODIFIED/WT_NEW → force fast-forward → 写回快照（冲突已预检排除，写回不覆盖 incoming）。解决冲突后**总是 reload pricing** 入 Store（幂等）。pricing.json 位于 `repo/config/`（ADR-0006）随云配置同步；app/user.json 目前仅路径占位（机制通用，未来加读写命令即覆盖）。Standalone 模式两命令返回 `Err`（UI 不渲染入口）。DTO 用具名 `ConfigConflictResolution{file,choice}` 而非元组（specta 友好）。

---

## 常用命令（新对话直接复用）

```bash
# Rust 检查 / 测试 / 格式
cargo check  --manifest-path E:/Project/D_VaultOne/src-tauri/Cargo.toml
cargo test   --manifest-path E:/Project/D_VaultOne/src-tauri/Cargo.toml
cargo fmt    --manifest-path E:/Project/D_VaultOne/src-tauri/Cargo.toml --check
cargo clippy --manifest-path E:/Project/D_VaultOne/src-tauri/Cargo.toml -- -D warnings

# 重生成类型化绑定（改了 Rust 命令/DTO 后必跑）
VAULTONE_GEN_BINDINGS=1 cargo run --manifest-path E:/Project/D_VaultOne/src-tauri/Cargo.toml

# 前端
yarn lint            # biome check
yarn lint:fix        # biome check --write
./node_modules/.bin/tsc --noEmit
yarn build           # tsc + vite build
```

> 注：`cargo` 命令在 bash 里若报 "No such file or directory"，说明 CWD 已在 src-tauri 内（持久），用 `--manifest-path` 即可。

---

## 文件清单

**Rust（`src-tauri/src/`）**：`lib.rs`（装配+启动钩子）、`error.rs`（AppError specta 类型化）、`model.rs`（领域类型+DTO）、`config.rs`（目录/deviceId/config.json）、`pricing.rs`（定价+CostCalculator+LiteLLM）、`db.rs`（SQLite store+查询）、`db_schema.sql`（DDL）、`providers.rs`（ClaudeCodeProvider）、`ingest.rs`（入库+JSONL）、`commands.rs`（23 个 tauri 命令）、`sync.rs`（git2 同步原语：PAT 回调 + clone/pull/commit/push + Standalone 守卫，ADR-0010；S3 增云配置同步 `sync_config`/`resolve_config_conflict` + 冲突检测 + `pull_preserving_dirty`，ADR-0005/#6）。

**前端（`src/`）**：`app/{App,providers,shell/shell,store/{api,store,hooks,slices/viewSlice}}`、`components/{query-state, ui/*}`、`features/{usage,pricing,settings}/{api,components/*}`、`lib/format.ts`、`main.tsx`、`types/generated/bindings.ts`。

**依赖新增**：Rust `rusqlite(bundled)`/`rust_decimal`/`chrono`/`rand`/`dirs`/`walkdir`/`ureq`；前端 `@reduxjs/toolkit`/`react-redux`/`recharts`/`react-is` + 11 个 shadcn 组件。

**删除**：旧 scaffold `src/App.tsx`、`src/App.css`（新 App 在 `src/app/App.tsx`）。

---

## 待办（按优先级）

1. ~~**写 Rust 测试**~~ ✅ **S1 完成（2026-07-16，35 例全绿）**。覆盖见上「验证结果」。新增 `tempfile` dev-dep；测试为内联 `#[cfg(test)] mod tests`（可触私有函数）。

   _原计划留档_：pricing（calc/normalize_key/resolve cache 启发式/litellm parse）、providers（解析样例 assistant 行→RawUsage，用 `with_dir`）、db（temp store：ingest 去重+stats/trend/logs 查询+rollup+rebill+pricing CRUD）、config（is_valid_device_id/生成）、ingest（recordify+JSONL 往返，用 read_jsonl_file）。
2. ~~**#5 git2 同步层**~~ ✅ **完成（S2a + S2b，2026-07-16，9 测试）**：
   - **S2a**（原语）：`git2 = "0.19" { default-features=false, features=["https"] }`（不带 ssh；Windows Schannel，**无需 cmake/openssl**）；`sync.rs` pat_credential/build_callbacks（PAT 进程内回调 + 防循环）/ open_or_clone（clone 后强制 `core.autocrlf=false` + force checkout，保 JSONL 跨平台 LF）/ pull（fetch+fast-forward，diverge 拒绝）/ commit_all（add -A + unborn HEAD）/ push / require_synced+ensure_repo（Standalone 守卫）。
   - **S2b**（接入）：高层 API `pull_and_import`（pull → `read_all_artifacts` → `store.ingest` uuid 去重）/ `commit_and_push`（有改动才 commit+push）/ `sync_now`（手动）；`commands.rs` 加 `sync_now` 命令（Standalone no-op）+ `collect_now` 写库后 best-effort commit+push + emit `usage_changed`；`lib.rs` setup 启动 pull（`std::thread::spawn`）+ ~10min 定时 push + `RunEvent::ExitRequested` flush（全 Synced 守卫）；前端 AppProviders listen `usage_changed` → `invalidateTags(['Usage'])` + settings「立即同步」按钮。AppState **不持 Repository**（每次 open_or_clone，规避 git2 Send/Sync）；emit 用原生 `app_handle.emit`（tauri-specta 类型化 event 待 RC 稳定后升级，ADR-0008 预留缝）。
3. ~~**#6 云配置手动同步+冲突**~~ ✅ **完成（S3，2026-07-18，4 测试）**：`sync_config`（fetch → 冲突=dirty∩远端改动 → 无冲突则 pull/commit/push + reload pricing，有则返回 `ConfigConflict` 列表）/ `resolve_config_conflict`（逐文件 keep_local/keep_remote → pull_preserving_dirty + 写回本地缓存 → 总 reload pricing）；前端 `conflict-resolver.tsx`（双方 preview + 逐文件选择）+ settings「同步云配置」按钮（Standalone 隐藏）。决策见 #13。
4. **终验**：清掉 dead-code 警告（见下）、`cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`、regen bindings、`yarn lint`、`yarn build`、`/code-review`。

### 当前 dead-code 警告（S3 已清自身；以下留 S4 统一处理）

`cargo clippy` 共 10 warning，**S3 新增代码（`sync_config`/`resolve_config_conflict`/`pull_preserving_dirty` 等及 DTO）零警告**——全部被 commands.rs 引用。剩余 dead-code（非 test 构建报，多仅在 `#[cfg(test)]` 用）：

- `config.rs`：`cloud_app_json`/`cloud_user_json`（**S3 实际未用**——仅 pricing.json 走通；app/user 待加读写命令）。
- `ingest.rs`：`ensure_artifact_dir`；另 `ingest.rs:112` clippy 建议改 `std::io::Error::other(_)`。
- `model.rs`：`CostBreakdown::total_f64`（仅 test）。
- `pricing.rs`：`PricingBook::{insert,len,is_empty}`。
- `providers.rs`：`with_dir`（仅 test）、`looks_like_projects_dir`。
- `sync.rs`：`ensure_repo`（高层改用 open_or_clone，仅 test 引用）。
- `db.rs`：`db.rs:450` clamp-like 模式建议用 `clamp`；`db.rs:451` `q.offset` 恒 ≥0 无效（既有 lint）。

> S3 更新：`sync.rs` 新增 `sync_config`/`resolve_config_conflict`/`pull_preserving_dirty`/`fetch_origin`/`origin_tip_oid`/`dirty_config_files`/`remote_changed_config_files`/`read_blob`/`preview`/`pricing_fingerprint`/`reload_pricing_into_store` 均已被 commands.rs 引用，**非 dead-code**。统一 S4 对上述列表 `#[allow(dead_code)]` 或删除 + 修 db.rs/ingest.rs clippy。

---

## Session 切分与推进纪律（2026-07-16 起）

为避免单 session 上下文膨胀，剩余工作切成小单元：**一个 session 做一个单元，跑绿验收门即停**，靠本文档（而非对话历史）跨 session 交接。

**方法论**：
1. 一个 session = 一个可独立验收单元，做完即停。
2. 读多写少的工作（读 ADR / 大文件）委托 subagent（Explore / general-purpose），只回传结论，不回传文件内容。
3. 本文档是唯一锚点；新 session 启动只读本节 + 目标 issue + 对应 ADR，不翻全部历史。
4. 存档（更新本表）后主动 `/clear` 开新单元。
5. issue tracker 与本文档状态保持一致。

**单元清单与进度**：

| 单元 | 内容 | 验收门 | 状态 |
|---|---|---|---|
| S0 | issue tracker 同步（已实现 6 issue 标注，不 close / 不 commit） | issue 状态诚实 | ✅ 2026-07-16 |
| S1 | Rust 测试（pricing / providers / db / config / ingest） | `cargo test` 全绿 | ✅ 2026-07-16（35 例） |
| S2a | #5 git2 核心（依赖 + clone/pull/commit/push + PAT 回调） | `cargo check` + 测试 | ✅ 2026-07-16（6 测试，41 全绿） |
| S2b | #5 git2 接入（SQLite↔JSONL 互转 + 同步时机 + emit 事件 + 前端） | 闭环测试 + lint/build | ✅ 2026-07-16（3 闭环测试，44 全绿） |
| S3 | #6 云配置手动同步 + 冲突提示 | UI + 测试 | ✅ 2026-07-18（4 测试，48 全绿） |
| S4 | 终验 + commit 决策 | clippy/fmt/test/lint/build 全绿 + diff 复核 | ⬜ |

**新 session 启动协议**：读本节 → 读目标单元对应 issue + ADR → 用 subagent 摸清要改的文件/签名 → 实现 → 跑验收门 → 更新本表状态 → 停。

> 注：本节方法论（小单元 + 验收门 + 外部锚点）属标准工程实践；subagent 隔离与 `/clear` 属 LLM agent 特有。建议日后固化进 `docs/agents/` 作为项目级工作流约定。

---

## 提交粒度（已决策，2026-07-18）

- 已落地：**单个内聚 commit** `feat: implement VaultOne core (closes #1-#8)`，body 按 issue 组织，push 到 `origin/main`。理由：改动跨文件深度交叉（`lib.rs` 一次性装配、`commands.rs`/`bindings.ts`/`sync.rs` 跨 issue），per-issue 或分层会让中间 commit 不可编译；单 commit 可编译、最可靠、最诚实。
- S4 清理（dead-code + db.rs/ingest.rs clippy）后续单独 commit（`chore:`/`refactor:`）。
- 历史约束（持续）：commit subject+body 英文 + Conventional Commits；本地工程文档（本文件、BLUEPRINT/CONTEXT/docs）中文。用户原话：「不许commit push，你尽管做，吧issues属于我的，都做完」「实现方案都认可你，接受你，按你推荐方案进行下去」——实现层面决策预授权；push 授权于 2026-07-18 解除。
