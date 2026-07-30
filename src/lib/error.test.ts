import { describe, expect, it } from "vitest"

import { describeError } from "@/lib/error"

describe("describeError", () => {
  it("extracts the message from a thrown Error", () => {
    expect(describeError(new Error("boom"))).toBe("boom")
  })

  it("extracts .message from a plain object (RTK Query-serialised shape)", () => {
    // run() throws `new Error("Type: detail")`; RTK Query serialises that to
    // { name, message } — message is the path that must win.
    expect(describeError({ message: "Config: bad token" })).toBe(
      "Config: bad token",
    )
  })

  it("extracts a string .data field", () => {
    expect(describeError({ data: "rate limited" })).toBe("rate limited")
  })

  it("extracts a string .error field", () => {
    expect(describeError({ error: "denied" })).toBe("denied")
  })

  it("prefers message over data and error", () => {
    expect(describeError({ message: "m", data: "d", error: "e" })).toBe("m")
  })

  it("returns a bare string verbatim", () => {
    expect(describeError("plain string")).toBe("plain string")
  })

  it("returns empty when nothing recognisable (caller adds fallback)", () => {
    expect(describeError(null)).toBe("")
    expect(describeError(undefined)).toBe("")
    expect(describeError({})).toBe("")
    expect(describeError(42)).toBe("")
  })
})
