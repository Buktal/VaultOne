import { describe, expect, it } from "vitest"

import { reorderIds } from "@/lib/reorder"

describe("reorderIds", () => {
  it("moves activeId into overId's slot, shifting the rest by one", () => {
    expect(reorderIds(["a", "b", "c"], "a", "c")).toEqual(["b", "c", "a"])
    expect(reorderIds(["a", "b", "c"], "c", "a")).toEqual(["c", "a", "b"])
  })

  it("returns null when the move landed where it started", () => {
    expect(reorderIds(["a", "b", "c"], "a", "a")).toBeNull()
  })

  it("returns null on a stale source or target id", () => {
    expect(reorderIds(["a", "b"], "zz", "b")).toBeNull()
    expect(reorderIds(["a", "b"], "a", "zz")).toBeNull()
  })

  it("does not mutate the input list", () => {
    const ids = ["a", "b", "c"]
    reorderIds(ids, "a", "c")
    expect(ids).toEqual(["a", "b", "c"])
  })
})
