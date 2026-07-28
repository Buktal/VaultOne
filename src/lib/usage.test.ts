import { describe, expect, it } from "vitest"
import { tokenTotal } from "@/lib/usage"
import type { UsageLogRow } from "@/types/generated/bindings"

function row(tokens: {
  input: number
  output: number
  cache_creation: number
  cache_read: number
}): UsageLogRow {
  return { tokens } as unknown as UsageLogRow
}

describe("tokenTotal", () => {
  it("sums the four buckets", () => {
    expect(
      tokenTotal(
        row({ input: 100, output: 200, cache_creation: 300, cache_read: 400 }),
      ),
    ).toBe(1000)
  })

  it("defaults every bucket to 0", () => {
    expect(
      tokenTotal(
        row({ input: 0, output: 0, cache_creation: 0, cache_read: 0 }),
      ),
    ).toBe(0)
  })

  it("counts cache reads toward the total", () => {
    expect(
      tokenTotal(
        row({ input: 10, output: 0, cache_creation: 0, cache_read: 990 }),
      ),
    ).toBe(1000)
  })
})
