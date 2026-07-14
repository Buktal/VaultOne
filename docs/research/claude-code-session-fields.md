# Claude Code Session 日志字段分析

> 调研日期：2026-07-15。样本：`~/.claude/projects/E--Project-AI/*.jsonl`。
> 目的：确认 session 日志解析（见 ADR-0001）能填出 BLUEPRINT 请求日志的哪些字段，避免靠猜测设计 schema。

## 日志形态

每个 `<session-id>.jsonl`，**每行一条 JSON 事件**。顶层字段：`type`（`user` / `assistant` / `system`）、`subtype`（system 事件用，如 `turn_duration`）、`timestamp`（ISO8601 UTC，毫秒）、`uuid`、`parentUuid`、`isSidechain`。

## assistant 消息结构（usage 来源）

```json
{
  "type": "assistant",
  "timestamp": "2026-07-13T16:55:22.467Z",
  "message": {
    "id": "msg_...",
    "model": "glm-5.2",
    "stop_reason": "tool_use",
    "usage": {
      "input_tokens": 3785,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 20224,
      "output_tokens": 93,
      "server_tool_use": { "web_search_requests": 0, "web_fetch_requests": 0 }
    },
    "content": ["..."]
  }
}
```

## system / turn_duration 事件（延迟来源）

```json
{ "type": "system", "subtype": "turn_duration", "durationMs": 92847, "messageC..." }
```

`durationMs` = 一个 turn（用户输入 → 回合结束）的墙钟耗时，**不是单次 API 请求耗时**，也**不是 TTFT**。样本量级：69s / 93s / 383s。

## BLUEPRINT 请求日志字段可行性

| BLUEPRINT 字段 | 来源 | 可行性 |
|---|---|---|
| Time | 顶层 `timestamp` | ✅ |
| Billed Model | `message.model` | ✅ |
| 输入 / 输出 Token | `usage.input_tokens` / `output_tokens` | ✅ |
| 缓存创建 / 缓存命中 | `usage.cache_creation_input_tokens` / `cache_read_input_tokens` | ✅ |
| Server tool 调用（额外） | `usage.server_tool_use.web_search / web_fetch` | ✅（BLUEPRINT 未列，白送） |
| Latency | `turn_duration.durationMs` | ⚠️ turn 级，非单请求级 |
| 单次 Cost | 无结构化字段 | ❌ 需应用层 token × 定价自算 |
| TTFT | 无 | ❌ |
| Status Code | 无 HTTP 码 | ❌ 可用 `stop_reason` 推断成败 |
| Provider | 日志无此字段 | ⚠️ 应用层按来源标注 |

## 由此产生的三个待决问题（留待 schema 设计阶段）

1. **「按请求维度记录」的粒度**：`usage` 是 per-API-request（每条 assistant 消息），`turn_duration` 是 per-turn（一回合可能含多次 API 请求）。请求日志每行对齐到哪一级？
2. **TTFT / Status Code**：砍掉？还是 TTFT 留空、Status 用 `stop_reason`（`end_turn` / `tool_use` → 200，`max_tokens` → 超时，error → 失败）映射成伪状态码？
3. **Cost** 完全靠应用层算 → 定价表是 hard dependency（接 BLUEPRINT 成本定价模块）。

## 勘误

此前判断「session 日志通常没有延迟」**有误**：存在 `turn_duration.durationMs`。真正缺失的只有结构化 Cost、TTFT、HTTP Status Code。
