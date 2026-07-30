import { describe, expect, it } from "vitest"

import { filterAndSortPricing, paginate } from "@/features/pricing/derive"

import type { PricingEntry } from "@/types/generated/bindings"

function entry(
  key: string,
  name: string,
  inputPerMillion: number,
): PricingEntry {
  return {
    model_key: key,
    display_name: name,
    input_per_million: inputPerMillion,
    output_per_million: 0,
    cache_read_per_million: 0,
    cache_creation_per_million: 0,
    is_builtin: false,
  }
}

describe("filterAndSortPricing", () => {
  const rows = [
    entry("alpha", "Alpha Model", 3),
    entry("beta", "Beta", 1),
    entry("gamma-pro", "Gamma Pro", 2),
  ]

  it("filters case-insensitively across key and display name", () => {
    const out = filterAndSortPricing(rows, "ALPHA", null, "asc")
    expect(out.map((e) => e.model_key)).toEqual(["alpha"])
    const pro = filterAndSortPricing(rows, "pro", null, "asc")
    expect(pro.map((e) => e.model_key)).toEqual(["gamma-pro"])
  })

  it("returns all (unsorted) when search and sortKey are empty", () => {
    expect(filterAndSortPricing(rows, "", null, "asc")).toHaveLength(3)
  })

  it("sorts numeric columns asc and desc", () => {
    const asc = filterAndSortPricing(rows, "", "input_per_million", "asc")
    expect(asc.map((e) => e.model_key)).toEqual(["beta", "gamma-pro", "alpha"])
    const desc = filterAndSortPricing(rows, "", "input_per_million", "desc")
    expect(desc.map((e) => e.model_key)).toEqual(["alpha", "gamma-pro", "beta"])
  })

  it("does not mutate the input", () => {
    const copy = [...rows]
    filterAndSortPricing(rows, "", "input_per_million", "desc")
    expect(rows.map((e) => e.model_key)).toEqual(copy.map((e) => e.model_key))
  })
})

describe("paginate", () => {
  it("totalPages is at least 1 even when empty", () => {
    expect(paginate(0, 0, 50)).toEqual({ totalPages: 1, page: 1 })
  })

  it("computes page from offset and clamps into range", () => {
    expect(paginate(100, 0, 50)).toEqual({ totalPages: 2, page: 1 })
    expect(paginate(100, 50, 50)).toEqual({ totalPages: 2, page: 2 })
    // offset past the end clamps to the last page
    expect(paginate(100, 999, 50)).toEqual({ totalPages: 2, page: 2 })
  })
})
