/**
 * The new order after a drag-move: `activeId` moves to `overId`'s slot —
 * everything between shifts by one (dnd-kit's arrayMove semantics, so the
 * result matches the live visual). Returns null when the move landed where it
 * started, so callers can skip the backend round trip. Ids not in `ids` make
 * the call a no-op (null) — a stale source/target (deleted mid-drag) must not
 * corrupt the order.
 *
 * Shared by the sessions group sidebar and the providers list (both drag to
 * reorder a local list); each call site feeds the result to its own
 * `reorder_*_cmd`.
 */
export function reorderIds(
  ids: readonly string[],
  activeId: string,
  overId: string,
): string[] | null {
  const from = ids.indexOf(activeId)
  const to = ids.indexOf(overId)
  if (from < 0 || to < 0 || from === to) return null
  const next = ids.slice()
  const [moved] = next.splice(from, 1)
  next.splice(to, 0, moved)
  return next
}
