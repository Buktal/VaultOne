import { describe, expect, it } from "vitest"
import {
  emptyProvider,
  providerApiKey,
  providerEndpoint,
  providerModel,
  withBasicFields,
} from "@/features/providers/derive"
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
