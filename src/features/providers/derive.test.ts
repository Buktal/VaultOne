import { describe, expect, it } from "vitest"
import {
  configApiKey,
  configEndpoint,
  emptyProvider,
  providerApiKey,
  providerEndpoint,
  providerFromPreset,
  providerModel,
  withBasicFields,
  withBasicFieldsInText,
} from "@/features/providers/derive"
import { PROVIDER_PRESETS } from "@/features/providers/presets"
import type { Provider } from "@/types/generated/bindings"

/** A provider whose settingsConfig carries a full env block. */
function provider(config: string): Provider {
  return {
    ...emptyProvider(),
    settingsConfig: config,
  }
}

describe("providerEndpoint / providerApiKey / providerModel", () => {
  it("reads the basic fields out of the settingsConfig env block", () => {
    const p = provider(
      JSON.stringify({
        env: {
          ANTHROPIC_BASE_URL: "https://api.moonshot.cn/anthropic",
          ANTHROPIC_AUTH_TOKEN: "sk-abc",
          ANTHROPIC_MODEL: "kimi-k2.7-code",
        },
      }),
    )
    expect(providerEndpoint(p)).toBe("https://api.moonshot.cn/anthropic")
    expect(providerApiKey(p)).toBe("sk-abc")
    expect(providerModel(p)).toBe("kimi-k2.7-code")
  })

  it("reads the API key from the legacy ANTHROPIC_API_KEY spelling", () => {
    const p = provider(
      JSON.stringify({ env: { ANTHROPIC_API_KEY: "sk-legacy" } }),
    )
    expect(providerApiKey(p)).toBe("sk-legacy")
  })

  it("prefers AUTH_TOKEN over API_KEY when both are present", () => {
    const p = provider(
      JSON.stringify({
        env: { ANTHROPIC_AUTH_TOKEN: "sk-new", ANTHROPIC_API_KEY: "sk-old" },
      }),
    )
    expect(providerApiKey(p)).toBe("sk-new")
  })

  it("returns empty strings for a missing field / garbage / empty config", () => {
    expect(providerEndpoint(provider("{}"))).toBe("")
    expect(providerApiKey(provider("{}"))).toBe("")
    expect(providerModel(provider("not-json"))).toBe("")
    expect(providerEndpoint(provider(""))).toBe("")
    expect(providerApiKey(provider('"a bare string"'))).toBe("")
  })

  it("treats a non-object env (hand-edited garbage) as empty", () => {
    expect(providerEndpoint(provider(JSON.stringify({ env: "nope" })))).toBe("")
    expect(providerApiKey(provider(JSON.stringify({ env: [1, 2] })))).toBe("")
  })
})

describe("withBasicFields", () => {
  it("updates the endpoint/key and preserves the rest of the snapshot", () => {
    const p = provider(
      JSON.stringify({
        includeCoAuthoredBy: false,
        env: {
          ANTHROPIC_BASE_URL: "old-url",
          ANTHROPIC_AUTH_TOKEN: "old-key",
          ANTHROPIC_MODEL: "keep-me",
        },
      }),
    )
    const next = withBasicFields(p, {
      endpoint: "new-url",
      apiKey: "new-key",
    })
    expect(providerEndpoint(next)).toBe("new-url")
    expect(providerApiKey(next)).toBe("new-key")
    expect(providerModel(next)).toBe("keep-me")
    // Non-env settings survive untouched.
    expect(JSON.parse(next.settingsConfig)).toMatchObject({
      includeCoAuthoredBy: false,
    })
  })

  it("clears the endpoint/key when the form fields are empty", () => {
    const p = provider(
      JSON.stringify({
        env: {
          ANTHROPIC_BASE_URL: "old-url",
          ANTHROPIC_AUTH_TOKEN: "old-key",
          ANTHROPIC_API_KEY: "legacy-key",
        },
      }),
    )
    const next = withBasicFields(p, { endpoint: "", apiKey: "" })
    expect(providerEndpoint(next)).toBe("")
    expect(providerApiKey(next)).toBe("")
    // Both key spellings are gone.
    const env = (JSON.parse(next.settingsConfig) as { env: object }).env
    expect(env).toEqual({})
  })

  it("migrates a legacy API_KEY to AUTH_TOKEN and drops the old spelling", () => {
    // Editing a provider that only carried ANTHROPIC_API_KEY: the new key is
    // written as AUTH_TOKEN and the old spelling removed, so the snapshot ends
    // with one credential instead of two.
    const p = provider(
      JSON.stringify({ env: { ANTHROPIC_API_KEY: "sk-legacy" } }),
    )
    const next = withBasicFields(p, { endpoint: "", apiKey: "sk-new" })
    expect(providerApiKey(next)).toBe("sk-new")
    const env = (
      JSON.parse(next.settingsConfig) as {
        env: Record<string, string>
      }
    ).env
    expect(env).toEqual({ ANTHROPIC_AUTH_TOKEN: "sk-new" })
  })

  it("keeps the id / name / category of the original provider", () => {
    const p: Provider = {
      ...emptyProvider(),
      id: "p1",
      name: "Kimi",
      category: "cn_official",
    }
    const next = withBasicFields(p, { endpoint: "u", apiKey: "k" })
    expect(next.id).toBe("p1")
    expect(next.name).toBe("Kimi")
    expect(next.category).toBe("cn_official")
  })

  it("tolerates an empty settingsConfig", () => {
    const next = withBasicFields(emptyProvider(), {
      endpoint: "https://x.dev",
      apiKey: "sk-x",
    })
    expect(providerEndpoint(next)).toBe("https://x.dev")
    expect(providerApiKey(next)).toBe("sk-x")
  })
})

describe("emptyProvider", () => {
  it("is a blank custom provider with an empty env and no id", () => {
    const p = emptyProvider()
    expect(p.id).toBe("")
    expect(p.category).toBe("custom")
    expect(providerEndpoint(p)).toBe("")
    expect(providerApiKey(p)).toBe("")
  })
})

describe("configEndpoint / configApiKey", () => {
  it("reads the env-backed fields straight from a JSON text", () => {
    const text = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://api.x.dev",
        ANTHROPIC_AUTH_TOKEN: "sk-x",
      },
    })
    expect(configEndpoint(text)).toBe("https://api.x.dev")
    expect(configApiKey(text)).toBe("sk-x")
  })

  it("reads the legacy API_KEY spelling from a JSON text", () => {
    expect(
      configApiKey(JSON.stringify({ env: { ANTHROPIC_API_KEY: "sk-l" } })),
    ).toBe("sk-l")
  })

  it("returns empty strings for garbage / empty text", () => {
    expect(configEndpoint("not-json")).toBe("")
    expect(configApiKey("")).toBe("")
    expect(configEndpoint('"a bare string"')).toBe("")
  })
})

describe("withBasicFieldsInText", () => {
  it("is the text-level twin of withBasicFields", () => {
    const text = JSON.stringify({
      includeCoAuthoredBy: false,
      env: {
        ANTHROPIC_BASE_URL: "old-url",
        ANTHROPIC_AUTH_TOKEN: "old-key",
        ANTHROPIC_MODEL: "keep-me",
      },
    })
    const next = withBasicFieldsInText(text, {
      endpoint: "new-url",
      apiKey: "new-key",
    })
    expect(configEndpoint(next)).toBe("new-url")
    expect(configApiKey(next)).toBe("new-key")
    expect(JSON.parse(next)).toMatchObject({
      includeCoAuthoredBy: false,
      env: { ANTHROPIC_MODEL: "keep-me" },
    })
  })

  it("formats the result with 2-space indentation", () => {
    const next = withBasicFieldsInText('{"env":{"ANTHROPIC_BASE_URL":"u"}}', {
      endpoint: "",
      apiKey: "",
    })
    expect(next).toBe('{\n  "env": {}\n}')
  })
})

describe("providerFromPreset", () => {
  it("builds a custom-category draft that copies the preset snapshot verbatim", () => {
    const kimi = PROVIDER_PRESETS.find((p) => p.name === "Kimi")
    expect(kimi).toBeDefined()
    const draft = providerFromPreset(kimi!)
    // 预设是起点、定制是终点：落表单的草稿归 custom，id 留空让 save 分配。
    expect(draft.id).toBe("")
    expect(draft.category).toBe("custom")
    expect(draft.name).toBe("Kimi")
    expect(draft.settingsConfig).toBe(kimi!.settingsConfig)
    // derive 读函数直接回填表单字段，无需另起一套解析。
    expect(providerEndpoint(draft)).toBe("https://api.moonshot.cn/anthropic")
    expect(providerModel(draft)).toBe("kimi-k2.7-code")
    expect(providerApiKey(draft)).toBe("")
  })

  it("keeps the preset's model mapping after withBasicFields writes the form fields", () => {
    const glm = PROVIDER_PRESETS.find((p) => p.name === "Zhipu GLM")
    expect(glm).toBeDefined()
    const next = withBasicFields(providerFromPreset(glm!), {
      endpoint: "https://example.com/anthropic",
      apiKey: "sk-123",
    })
    expect(providerEndpoint(next)).toBe("https://example.com/anthropic")
    expect(providerApiKey(next)).toBe("sk-123")
    const env = (
      JSON.parse(next.settingsConfig) as {
        env: Record<string, string>
      }
    ).env
    // 模型映射（表单不拥有的字段）原样保留。
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.1")
    expect(env.ANTHROPIC_MODEL).toBe("glm-5.1")
  })

  it("keeps the Bedrock template-variable placeholders until the template step", () => {
    const bedrock = PROVIDER_PRESETS.find(
      (p) => p.name === "AWS Bedrock (AKSK)",
    )
    expect(bedrock).toBeDefined()
    const draft = providerFromPreset(bedrock!)
    const env = (
      JSON.parse(draft.settingsConfig) as {
        env: Record<string, string>
      }
    ).env
    expect(env.ANTHROPIC_BASE_URL).toBe(
      // biome-ignore lint/suspicious/noTemplateCurlyInString: 断言字面量占位符文本
      "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
    )
    expect(env.AWS_ACCESS_KEY_ID).toBe(
      // biome-ignore lint/suspicious/noTemplateCurlyInString: 断言字面量占位符文本
      "${AWS_ACCESS_KEY_ID}",
    )
    expect(env.CLAUDE_CODE_USE_BEDROCK).toBe("1")
  })

  it("never mutates the preset constant", () => {
    const before = JSON.stringify(PROVIDER_PRESETS)
    const draft = providerFromPreset(PROVIDER_PRESETS[0]!)
    expect(draft.settingsConfig).toBe(PROVIDER_PRESETS[0]!.settingsConfig)
    expect(JSON.stringify(PROVIDER_PRESETS)).toBe(before)
  })
})
