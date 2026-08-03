import type { TFunction } from "i18next"
import { describe, expect, it } from "vitest"

import { describeError } from "@/lib/error"

// A stand-in for i18next's `t`: echoes the key, appending the interpolated
// `data` so tests can assert both the chosen key and the payload. Cast to
// `TFunction` (a branded type) so the signature matches production callers.
const t = ((key: string, opts?: Record<string, unknown>) =>
  opts && "data" in opts ? `${key}:${String(opts.data)}` : key) as TFunction

describe("describeError", () => {
  it("maps a structured AppError to errors.<type> with data interpolation", () => {
    expect(describeError({ type: "Config", data: "bad token" }, t)).toBe(
      "errors.Config:bad token",
    )
    expect(describeError({ type: "Sync", data: "offline" }, t)).toBe(
      "errors.Sync:offline",
    )
    expect(describeError({ type: "Internal", data: "serde: eof" }, t)).toBe(
      "errors.Internal:serde: eof",
    )
  })

  it("covers every backend AppError variant", () => {
    const variants = ["Config", "Db", "Provider", "Pricing", "Sync", "Internal"]
    for (const type of variants) {
      expect(describeError({ type, data: "x" }, t)).toBe(`errors.${type}:x`)
    }
  })

  it("extracts the message from a thrown Error (non-API path, e.g. updater)", () => {
    expect(describeError(new Error("boom"), t)).toBe("boom")
  })

  it("extracts .message from a plain object (RTK Query-serialised shape)", () => {
    expect(describeError({ message: "network down" }, t)).toBe("network down")
  })

  it("extracts a string .data / .error field when there is no .message", () => {
    expect(describeError({ data: "rate limited" }, t)).toBe("rate limited")
    expect(describeError({ error: "denied" }, t)).toBe("denied")
  })

  it("returns a bare string verbatim", () => {
    expect(describeError("plain string", t)).toBe("plain string")
  })

  it("returns empty when nothing recognisable (caller adds fallback)", () => {
    expect(describeError(null, t)).toBe("")
    expect(describeError(undefined, t)).toBe("")
    expect(describeError({}, t)).toBe("")
    expect(describeError(42, t)).toBe("")
  })
})
