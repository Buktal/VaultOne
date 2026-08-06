// Providers browser controller: the list query plus drag-reorder wiring.
// `reorderIds` (lib/reorder.ts) owns the move math — this hook just feeds it
// the live order and ships the new one to `reorder_providers_cmd`. Save/delete
// stay in the view (they go through useMutateWithToast, like pricing), so the
// hook stays a thin data source + sort glue.

import {
  useListProvidersQuery,
  useReorderProvidersMutation,
} from "@/app/store/api"
import { reorderIds } from "@/lib/reorder"

export function useProvidersBrowser() {
  const { data: providers = [], isLoading } = useListProvidersQuery()
  const [reorder] = useReorderProvidersMutation()

  /** Apply a drag move: recompute the order, skip the round trip when the
   *  move landed where it started. */
  function onReorder(activeId: string, overId: string): void {
    const next = reorderIds(
      providers.map((p) => p.id),
      activeId,
      overId,
    )
    if (next) void reorder(next)
  }

  return { providers, isLoading, onReorder }
}
