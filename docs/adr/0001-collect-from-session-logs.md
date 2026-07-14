# 0001 — 从 session 日志解析采集 Usage 数据

**Status**: accepted（2026-07-15）

VaultOne 的 LLM Usage 数据采集，采用**解析本地 session 日志**为主线（如 Claude Code 的 `~/.claude/projects/**/*.jsonl`），**不**在 MVP 中引入本地代理拦截或厂商 Usage API。

## 为什么

session 日志解析零侵入、零配置、零 API key，且经字段验证（见 `docs/research/claude-code-session-fields.md`）已覆盖核心看板指标：token 四件套（input / output / cache_creation / cache_read）、缓存、server tool 调用、turn 级延迟（`turn_duration`）、模型、时间戳。真正缺失的 Cost / TTFT / Status Code 中，Cost 本就要靠应用层用 token × 定价计算（与 CodeBurn、CC-Switch 一致）。

## 考虑过的替代方案

- **本地代理拦截**（CC-Switch 范式）：字段最全（含 TTFT、HTTP 状态码），但要把 CLI 出站流量改走本地代理，侵入式 + 重基础设施，使 VaultOne 偏离「轻量只读看板」定位。作为**未来可选增强**（opt-in）保留，不进 MVP。
- **厂商 Usage API**：多数厂商无 per-request 细粒度接口、账号级无法区分设备、需各家 key。基本不可行。

## 后果

- 采集层按 provider 插件化设计（借鉴 CodeBurn），每新增一个 AI 工具 = 加一个 provider。
- 请求日志中 TTFT 列无来源、Status Code 需用 `stop_reason` 推断——具体取舍在 schema 设计阶段定。
- Cost 是应用层计算，定价表（BLUEPRINT 成本定价模块）成为 hard dependency。
