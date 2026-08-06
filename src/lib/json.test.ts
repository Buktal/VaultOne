// Tests for the generic JSON helpers shared by the JSON editor and the
// provider form sheet's settingsConfig sync (lib/json.ts).

import { describe, expect, it } from "vitest"
import { formatJson, parseJsonObject } from "@/lib/json"

describe("parseJsonObject", () => {
  it("parses a plain object", () => {
    const result = parseJsonObject('{"env": {"ANTHROPIC_MODEL": "m"}}')
    expect(result).toEqual({
      ok: true,
      value: { env: { ANTHROPIC_MODEL: "m" } },
    })
  })

  it("treats empty text as an empty object (a blank snapshot)", () => {
    expect(parseJsonObject("")).toEqual({ ok: true, value: {} })
    expect(parseJsonObject("   ")).toEqual({ ok: true, value: {} })
  })

  it("flags a syntax error without throwing", () => {
    const result = parseJsonObject('{"env": ')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.length).toBeGreaterThan(0)
  })

  it("flags a non-object top level (array / string / number)", () => {
    expect(parseJsonObject("[1, 2]").ok).toBe(false)
    expect(parseJsonObject('"a bare string"').ok).toBe(false)
    expect(parseJsonObject("42").ok).toBe(false)
    expect(parseJsonObject("null").ok).toBe(false)
  })
})

describe("formatJson", () => {
  it("trims, parses and stringifies with 2-space indentation", () => {
    expect(formatJson('  {"b":1,"a":[1,2]}  ')).toBe(
      '{\n  "b": 1,\n  "a": [\n    1,\n    2\n  ]\n}',
    )
  })

  it("throws on invalid JSON", () => {
    expect(() => formatJson("{ nope")).toThrow()
  })

  it("leaves already-formatted JSON unchanged", () => {
    const text = '{\n  "env": {}\n}'
    expect(formatJson(text)).toBe(text)
  })
})
