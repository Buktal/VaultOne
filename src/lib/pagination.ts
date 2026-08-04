// Offset → page stats for client-side paginated tables. Generic table math —
// not feature-specific — shared by the pricing table and the usage log table so
// the offset-clamp rule lives in one place. (A divergent copy once omitted the
// clamp and produced an out-of-range page number in the log table when rows
// shrank beneath the current offset.)

export interface PageStats {
  totalPages: number
  page: number
}

/** Offset → page stats. `totalPages` is at least 1 so a single-page control
 *  never disappears, and `page` is clamped into range — important when the row
 *  count shrinks beneath the current offset (e.g. forgetting a device removes
 *  rows) before the offset has reset. */
export function paginate(
  total: number,
  offset: number,
  pageSize: number,
): PageStats {
  const totalPages = Math.max(1, Math.ceil(total / pageSize))
  const page = Math.min(Math.floor(offset / pageSize) + 1, totalPages)
  return { totalPages, page }
}
