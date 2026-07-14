# VaultOne

VaultOne 是一个 Tauri 桌面应用，解析本地 AI 工具的会话日志得到 LLM 用量数据，以文本 JSONL 为介质经 GitHub 仓库在多设备间同步，并在本地看板查询。它和参考项目（CodeBurn、CC-Switch）的根本差异在于：**以 GitHub 仓库（文本 JSONL）作为多设备同步后端**。

## Language

### 数据分层

**Source（源日志）**:
AI 工具（MVP 为 Claude Code）原生产生的会话日志，VaultOne 只读取、不修改。
_Avoid_: 原始日志、raw log、session 文件

**Provider（采集器）**:
针对某一 AI 工具的解析插件，负责发现并解析该工具的 Source，产出用量数据。MVP 仅含 Claude Code provider，架构按 provider 插件化预留多工具。
_Avoid_: adapter、reader、解析器

**Artifact（同步产物）**:
Provider 从 Source 解析后、导出并上传到 GitHub 仓库的用量数据，按设备与日期切分为文本 JSONL。是多设备同步的介质。
_Avoid_: 上报文件、导出文件、同步文件

**Local Store（本地库）**:
本地的用量查询库，看板从其读取。与 Artifact 之间通过确定性转换同步（见 ADR-0001 及后续同步 ADR）。
_Avoid_: 数据库、cache、缓存库

### 多设备

**Device（设备）**:
一台运行 VaultOne 的机器。首次启动生成持久 12 位十六进制短 ID（`deviceId`）作为唯一键，用作 Artifact 目录名与多设备合并的分组维度；显示名在 config 中单独映射，不参与唯一性（见 ADR-0002）。
_Avoid_: 机器、host、客户端、节点

### 用量语义

**Turn（回合）**:
用户一次输入到该回合结束的完整交互。其墙钟耗时由 Source 记录，是 turn 级耗时——非单次 API 请求耗时，也非 TTFT。
_Avoid_: round、轮次

**Usage Record（用量记录）**:
一条用量数据单元，承载 token（input / output / cache_creation / cache_read）、计费模型、时间戳等。粒度（按 API 请求还是按 Turn）尚未定。
_Avoid_: usage 条目、日志条目

## 待定术语

以下核心术语尚未与用户对齐，待后续 grilling 落定后补入：

- **Sync（同步）**：本地与 GitHub 之间的对齐机制，见第 6/7 问。
