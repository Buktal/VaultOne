// 预设清单的守门测试：18 个内置预设必须与需求清单一字不差（数量、名称、顺序、
// category 映射），排除项（OAuth 类 / gemini_native / openai_chat 格式）一个都不许
// 泄漏进清单，每项 settingsConfig 都是合法 JSON 且含 env 块。清单文件是单一事实
// 来源——这里把「需求」固化成语义断言，任何增删改都会先在这里红掉。

import { describe, expect, it } from "vitest"
import {
  providerEndpoint,
  providerFromPreset,
} from "@/features/providers/derive"
import { PROVIDER_PRESETS } from "@/features/providers/presets"

import type { ProviderCategory } from "@/types/generated/bindings"

/** 权威清单：名称与顺序不得增删改（官方/云 3 + 国内大厂 11 + 热门聚合 4）。 */
const EXPECTED_NAMES = [
  "Claude Official",
  "AWS Bedrock (AKSK)",
  "AWS Bedrock (API Key)",
  "Kimi",
  "Kimi For Coding",
  "DeepSeek",
  "Zhipu GLM",
  "火山 Agentplan",
  "DouBaoSeed",
  "百度千帆",
  "阿里百炼 For Coding",
  "StepFun",
  "MiniMax",
  "小米 MiMo",
  "SiliconFlow",
  "OpenRouter",
  "ModelScope",
  "Novita AI",
]

/** 每个预设的 category 归属（名称 → 分类）。 */
const NAME_CATEGORY: Record<string, ProviderCategory> = {
  "Claude Official": "official",
  "AWS Bedrock (AKSK)": "cloud_provider",
  "AWS Bedrock (API Key)": "cloud_provider",
  Kimi: "cn_official",
  "Kimi For Coding": "cn_official",
  DeepSeek: "cn_official",
  "Zhipu GLM": "cn_official",
  "火山 Agentplan": "cn_official",
  DouBaoSeed: "cn_official",
  百度千帆: "cn_official",
  "阿里百炼 For Coding": "cn_official",
  StepFun: "cn_official",
  MiniMax: "cn_official",
  "小米 MiMo": "cn_official",
  SiliconFlow: "aggregator",
  OpenRouter: "aggregator",
  ModelScope: "aggregator",
  "Novita AI": "aggregator",
}

/** 排除项黑名单（正则）：OAuth 类（GitHub Copilot / Codex / xAI）与 gemini_native /
 *  openai_chat 格式，断言其不出现在任何预设的名称或 settingsConfig 里。`xai` 用
 *  词边界匹配——SiliconFlow 的模型名 MiniMaxAI 合法含 "xai" 子串，只有独立的
 *  xAI 供应商名才算泄漏。 */
const BLACKLIST_PATTERNS = [
  /oauth/,
  /github/,
  /copilot/,
  /codex/,
  /grok/,
  /\bxai\b/,
  /gemini/,
  /openai/,
  /generativelanguage/,
  /gpt-5/,
]

describe("PROVIDER_PRESETS", () => {
  it("总数 18 且名称与顺序与需求清单完全一致", () => {
    expect(PROVIDER_PRESETS).toHaveLength(18)
    expect(PROVIDER_PRESETS.map((p) => p.name)).toEqual(EXPECTED_NAMES)
  })

  it("名称唯一", () => {
    const names = PROVIDER_PRESETS.map((p) => p.name)
    expect(new Set(names).size).toBe(names.length)
  })

  it("category 映射正确且分组合计为 1/2/11/4", () => {
    const counts: Record<ProviderCategory, number> = {
      official: 0,
      cloud_provider: 0,
      cn_official: 0,
      aggregator: 0,
      custom: 0,
    }
    for (const preset of PROVIDER_PRESETS) {
      expect(preset.category).toBe(NAME_CATEGORY[preset.name])
      counts[preset.category] += 1
    }
    expect(counts).toEqual({
      official: 1,
      cloud_provider: 2,
      cn_official: 11,
      aggregator: 4,
      custom: 0,
    })
  })

  it("无排除项泄漏（OAuth / gemini_native / openai_chat）", () => {
    const text = PROVIDER_PRESETS.map((p) => `${p.name} ${p.settingsConfig}`)
      .join("\n")
      .toLowerCase()
    for (const pattern of BLACKLIST_PATTERNS) {
      expect(text).not.toMatch(pattern)
    }
  })

  it("每项 settingsConfig 是合法 JSON 且含 env 对象", () => {
    for (const preset of PROVIDER_PRESETS) {
      const parsed: unknown = JSON.parse(preset.settingsConfig)
      expect(
        parsed !== null && typeof parsed === "object" && !Array.isArray(parsed),
      ).toBe(true)
      const env = (parsed as { env?: unknown }).env
      expect(
        env !== null && typeof env === "object" && !Array.isArray(env),
      ).toBe(true)
    }
  })

  it("每个预设都带分类的必填元数据（websiteUrl / icon / iconColor）", () => {
    for (const preset of PROVIDER_PRESETS) {
      expect(preset.websiteUrl).toBeTruthy()
      expect(preset.icon).toBeTruthy()
      expect(preset.iconColor).toBeTruthy()
    }
  })

  it("预设可被表单读取：providerFromPreset 回填端点与模型", () => {
    const openrouter = PROVIDER_PRESETS.find((p) => p.name === "OpenRouter")
    expect(openrouter).toBeDefined()
    const draft = providerFromPreset(openrouter!)
    expect(providerEndpoint(draft)).toBe("https://openrouter.ai/api")
  })
})
