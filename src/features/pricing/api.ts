// Pricing feature RTK Query endpoints (ADR-0006 / 0008).

import { api } from "@/app/store/api"
import type { PricingEntry } from "@/types/generated/bindings"
import { commands } from "@/types/generated/bindings"

type SpectaResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: unknown }

async function unwrap<T>(
  p: Promise<SpectaResult<T>>,
): Promise<{ data: T } | { error: unknown }> {
  const res = await p
  return res.status === "ok" ? { data: res.data } : { error: res.error }
}

export const pricingApi = api.injectEndpoints({
  endpoints: (build) => ({
    listPricing: build.query<PricingEntry[], void>({
      queryFn: () => unwrap(commands.listPricing()),
      providesTags: ["Pricing"],
    }),
    savePricingEntry: build.mutation<
      null,
      { entry: PricingEntry; is_builtin?: boolean }
    >({
      queryFn: ({ entry, is_builtin }) =>
        unwrap(commands.savePricingEntry(entry, is_builtin ?? false)),
      invalidatesTags: ["Pricing"],
    }),
    deletePricingEntry: build.mutation<null, string>({
      queryFn: (modelKey) => unwrap(commands.deletePricingEntry(modelKey)),
      invalidatesTags: ["Pricing"],
    }),
    fetchLitellmPricing: build.mutation<number, void>({
      queryFn: () => unwrap(commands.fetchLitellmPricing()),
      invalidatesTags: ["Pricing"],
    }),
    reloadPricingFromFile: build.mutation<number, void>({
      queryFn: () => unwrap(commands.reloadPricingFromFile()),
      invalidatesTags: ["Pricing"],
    }),
    savePricingToFile: build.mutation<null, void>({
      queryFn: () => unwrap(commands.savePricingToFile()),
      invalidatesTags: ["Pricing"],
    }),
  }),
})

export const {
  useListPricingQuery,
  useSavePricingEntryMutation,
  useDeletePricingEntryMutation,
  useFetchLitellmPricingMutation,
  useReloadPricingFromFileMutation,
  useSavePricingToFileMutation,
} = pricingApi
