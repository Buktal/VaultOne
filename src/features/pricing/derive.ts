// Pure read-model derivations for the pricing table: client-side search +
// single-column sort, and offset → page stats. The full list is already loaded;
// these keep rendering snappy past a few hundred entries.

import type { PricingEntry } from "@/types/generated/bindings"

export type PricingSortKey = keyof PricingEntry

/**
 * Case-insensitive search over model key + display name, then an optional
 * single-column sort (numeric when both sides are numbers, else localeCompare).
 * The input is never mutated.
 */
export function filterAndSortPricing(
  entries: PricingEntry[],
  search: string,
  sortKey: PricingSortKey | null,
  sortDir: "asc" | "desc",
): PricingEntry[] {
  const q = search.trim().toLowerCase()
  let list = q
    ? entries.filter(
        (e) =>
          e.model_key.toLowerCase().includes(q) ||
          e.display_name.toLowerCase().includes(q),
      )
    : entries
  if (sortKey) {
    list = [...list].sort((a, b) => {
      const av = a[sortKey] ?? 0
      const bv = b[sortKey] ?? 0
      const cmp =
        typeof av === "number" && typeof bv === "number"
          ? av - bv
          : String(av).localeCompare(String(bv))
      return sortDir === "asc" ? cmp : -cmp
    })
  }
  return list
}

export interface PageStats {
  totalPages: number
  page: number
}

/** Offset → page stats. `totalPages` is at least 1 so a single-page control
 *  never disappears, and `page` is clamped into range. */
export function paginate(
  total: number,
  offset: number,
  pageSize: number,
): PageStats {
  const totalPages = Math.max(1, Math.ceil(total / pageSize))
  const page = Math.min(Math.floor(offset / pageSize) + 1, totalPages)
  return { totalPages, page }
}
