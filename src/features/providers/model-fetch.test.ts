import { describe, expect, it } from "vitest"
import {
  bucketFetchModelsError,
  presetModelsUrl,
} from "@/features/providers/model-fetch"
import { PROVIDER_PRESETS } from "@/features/providers/presets"

describe("presetModelsUrl", () => {
  it("returns the preset's modelsUrl when the endpoint equals its default", () => {
    // DouBaoSeed 预设：/api/compatible 不在剥离清单里，必须精确指路。
    const doubao = PROVIDER_PRESETS.find((p) => p.name === "DouBaoSeed")
    expect(doubao?.modelsUrl).toBe(
      "https://ark.cn-beijing.volces.com/api/v3/models",
    )
    expect(
      presetModelsUrl(
        "https://ark.cn-beijing.volces.com/api/compatible",
        PROVIDER_PRESETS,
      ),
    ).toBe("https://ark.cn-beijing.volces.com/api/v3/models")
  })

  it("matches with whitespace and trailing slash normalized", () => {
    expect(
      presetModelsUrl(
        " https://ark.cn-beijing.volces.com/api/compatible/ ",
        PROVIDER_PRESETS,
      ),
    ).toBe("https://ark.cn-beijing.volces.com/api/v3/models")
  })

  it("returns null when the endpoint matches a preset without modelsUrl", () => {
    expect(
      presetModelsUrl("https://api.moonshot.cn/anthropic", PROVIDER_PRESETS),
    ).toBeNull()
  })

  it("returns null when no preset matches", () => {
    expect(
      presetModelsUrl("https://api.example.com", PROVIDER_PRESETS),
    ).toBeNull()
  })

  it("returns null for an empty endpoint", () => {
    expect(presetModelsUrl("", PROVIDER_PRESETS)).toBeNull()
    expect(presetModelsUrl("   ", PROVIDER_PRESETS)).toBeNull()
  })
})

describe("bucketFetchModelsError", () => {
  it("buckets the auth tag (401/403 认证失败)", () => {
    expect(
      bucketFetchModelsError("AUTH_FAILED: HTTP 401: invalid key"),
    ).toEqual({
      kind: "auth",
      detail: "HTTP 401: invalid key",
    })
  })

  it("buckets the endpoint tag (404/405/全失败 端点未开放)", () => {
    expect(
      bucketFetchModelsError(
        "ENDPOINT_CLOSED: all candidates failed: HTTP 404: not found",
      ),
    ).toEqual({
      kind: "endpoint",
      detail: "all candidates failed: HTTP 404: not found",
    })
  })

  it("buckets the timeout tag", () => {
    expect(
      bucketFetchModelsError(
        "TIMEOUT: http: timeout connecting to https://x.com",
      ),
    ).toEqual({
      kind: "timeout",
      detail: "http: timeout connecting to https://x.com",
    })
  })

  it("buckets the format tag (parse 失败 格式不支持)", () => {
    expect(bucketFetchModelsError("BAD_FORMAT: missing field `data`")).toEqual({
      kind: "format",
      detail: "missing field `data`",
    })
  })

  it("buckets the network tag", () => {
    expect(bucketFetchModelsError("NETWORK: HTTP 500: boom")).toEqual({
      kind: "network",
      detail: "HTTP 500: boom",
    })
  })

  it("falls back to network for untagged or unknown-tagged strings", () => {
    expect(bucketFetchModelsError("HTTP 500: boom")).toEqual({
      kind: "network",
      detail: "HTTP 500: boom",
    })
    expect(bucketFetchModelsError("SOMETHING_ELSE: hi")).toEqual({
      kind: "network",
      detail: "SOMETHING_ELSE: hi",
    })
    expect(bucketFetchModelsError("")).toEqual({ kind: "network", detail: "" })
  })

  it("keeps the rest of a tagged string intact after the first tag", () => {
    expect(bucketFetchModelsError("TIMEOUT: foo: bar: baz")).toEqual({
      kind: "timeout",
      detail: "foo: bar: baz",
    })
  })
})
