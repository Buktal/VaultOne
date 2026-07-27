import type { UsageLogRow } from "@/types/generated/bindings"

/** Sum of the four token buckets: input + output + cache_creation + cache_read. */
export function tokenTotal(r: UsageLogRow): number {
  return (
    r.tokens.input +
    r.tokens.output +
    r.tokens.cache_creation +
    r.tokens.cache_read
  )
}
