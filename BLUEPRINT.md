# BLUEPRINT

- 项目名：VaultOne
- 架构：Tauri、Vite、React、UI 等



## 参考项目

- E:\Project\AI\GitHub\CC-Switch（主参考）
- E:\Project\AI\GitHub\CodeBurn

## 目标

1. 基于 GitHub 仓库实现云储存
2. VaultOne 基于 GitHub 实现 LLM Usage 数据上报/同步
3. 数据上报：多设备隔离，按请求维度记录，以日期 yyyy-mm-dd 为单位
4. 数据同步：对比本地同步记录（或者对比 Git 变更记录），同步到本地库
5. 数据看板
6. 未来
   - Prompt、Skill、Hook、SubAgent、Scripts、等文档的存储跟同步（基于 GitHub 仓库）
   - Claude Code、Codex、Cursor 等配置的存储跟同步（基于 GitHub 仓库）

### GitHub

- 授权登录
- 指定仓库

### 云储存

```yaml
├── config/                         # 配置文件目录
│   ├── app.json                    # 全局应用与系统配置
│   └── user.json                   # 用户信息与画像配置
└── data/                           # 设备运行与日志数据目录
    ├── deviceA/                    # 设备 A 数据集
    │   ├── usage-2026-07-11.jsonl
    │   └── usage-2026-07-12.jsonl
    └── deviceB/                    # 设备 B 数据集
        ├── usage-2026-07-11.jsonl
        └── usage-2026-07-12.jsonl
```

### 数据看板

#### 检索控制

- **全局条件筛选器**
  - **来源筛选：** 按数据/调用来源过滤（如 `全部来源`）。
  - **模型筛选：** 按模型类型进行针对性筛选（如 `全部模型`、`glm-5.2` 等）。
  - **时间范围选择器：** 支持快捷切换时间粒度（如 `当天`、按日期范围自定义等）。
- **控制与刷新设置**
  - **自动刷新频率控制：** 设置数据定时刷新间隔（如 `30s` 自动刷新）。
  - **快捷看板/图标导航：** 支持切换不同的视角或厂商视角（如 Claude、OpenAI、Gemini 等不同厂商图标入口）。

#### 使用统计

- **实时指标统计栏**
  - **真实消耗 Tokens：** 汇总并展示总 Token 消耗（支持大数字化简展示，如 `360.86万`）。
  - **细分 Token 指标：**
    - 输入（Prompt Tokens）
    - 输出（Completion Tokens）
    - 缓存创建（Cache Creation Tokens）
    - 缓存命中（Cache Read/Hit Tokens）
  - **缓存效率分析：** 缓存命中率百分比及可视化进度条（如 `90.2%`）。
  - **请求与成本汇总：** 总请求次数（如 `45`）与累计总成本（美元/人民币等币种计价，如 `$1.7564`）。
- **使用趋势图表 (Usage Trends Chart)**
  - **多维度折线/面积复合图表：** 实时绘制不同时间段内的使用波峰与波谷。
  - **双 Y 轴/多指标对照：** 支持同时对 Token 数值（左轴，如 `0 ~ 3000k`）与花费金额（右轴，如 `$0 ~ $1.8`）进行趋势拟合。
  - **多图例交互切换：** 成本、缓存创建、缓存命中、新增输入、输出等图例开关与对比。

#### 请求日志

- **数据明细列表**
  - **时间戳 (Time)：** 记录具体调用的精确时间（如 `07/14 13:00`）。
  - **供应商 (Provider)：** 标注渠道/供应商信息（如 `Claude (Session)`）。
  - **计费/映射模型 (Billed Model)：** 记录实际触发计费的模型标识（如 `glm-5.2`）。
  - **输入/输出 Token 明细：**
    - **输入：** 展示原始输入 Token 及关联的 Context/Cache 信息（如 `864 / R53,696`）。
    - **输出：** 展示模型生成产出的 Token 数（如 `184`）。
  - **单次成本 (Cost)：** 精确计算单次调用的费用（如 `$0.0160`）。
  - **性能耗时 (Latency/TTFT)：** 展示整体响应时间及首字延迟（如 `0.0s`）。
  - **状态响应码 (Status Code)：** 状态码可视化呈现（如 `200` 绿色成功标识）。
  - **来源追溯 (Source)：** 标注调用的具体日志类型或端点（如 `session_log`）。

#### 成本定价

- **应用维度倍率设置 (Application Multiplier)**
  - 支持针对不同的集成应用/供应商（如 Claude, Codex, Gemini）分别配置**默认倍率（Default Ratio/Multiplier）**，用于快捷调整总体计费权重。
- **计费模式来源选择 (Billing Mode Strategy)**
  - 支持针对各应用独立下拉选择**计费模式**（如：`返回模型`模式），用于控制计费依据是按照请求映射模型还是实际返回响应的模型。

- **模型成本配置列表（单位：每百万 Token / Per Million Tokens）**

  - **模型标识 (Model Key)：** 系统的内部模型唯一 ID（如 `claude-3-5-haiku-20241022`）。
  - **显示名称 (Display Name)：** 面向前端展示的友好别名（如 `Claude 3.5 Haiku`）。
  - **输入成本 (Input Cost)：** 每 1M Input Tokens 的计费单价（如 `$0.80`）。
  - **输出成本 (Output Cost)：** 每 1M Output Tokens 的计费单价（如 `$4`）。
  - **缓存命中成本 (Cache Hit Cost)：** 每 1M Cache Read/Hit Tokens 的优惠计费单价（如 `$0.08`）。
  - **缓存创建成本 (Cache Creation Cost)：** 每 1M Cache Creation/Write Tokens 的创建单价（如 `$1`）。

- **操作模块** 

  - **新增模型定价 (Add Model Pricing)**

  - **行级数据管理 (Row-level Action)**
    - **编辑 (Edit)：** 针对单条模型的各项 Token 成本单价及显示名称进行实时修改。
    - **删除 (Delete)：** 一键移除指定模型的计费配置项。