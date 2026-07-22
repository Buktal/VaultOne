import { createApi, fakeBaseQuery } from "@reduxjs/toolkit/query/react"
import type {
  AppError,
  AppInfo,
  ConfigConflictResolution,
  ConfigSyncOutcome,
  DeviceInfo,
  IngestReport,
  LogsQuery,
  ModelStatsRow,
  PricingEntry,
  RunMode,
  SyncReport,
  TrendBucket,
  TrendPoint,
  UsageFilter,
  UsageLogRow,
  UsageStats,
} from "@/types/generated/bindings"
import { commands } from "@/types/generated/bindings"
import type { CloseBehavior, Preferences } from "@/types/preferences"

/**
 * RTK Query data layer over the typed Tauri command contract.
 *
 * Every command returns a `{ status: "ok" | "error" }` envelope (tauri-specta).
 * `run` unwraps it: ok ⇒ data, error ⇒ throw (RTK Query surfaces it as the
 * query's `error`). The UI never sees SQL or invoke() directly.
 */

type Envelope<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: AppError }

async function run<T>(p: Promise<Envelope<T>>): Promise<T> {
  const r = await p
  if (r.status === "ok") return r.data
  throw new Error(`${r.error.type}: ${r.error.data}`)
}

/** Stable cache id for a filter (so each filter scope caches independently). */
export function filterId(f: UsageFilter): string {
  return [f.from_ts, f.to_ts, f.model, f.source, f.device_scope].join("|")
}

/** Default filter = the active dashboard scope. */
export const EMPTY_FILTER: UsageFilter = {
  from_ts: null,
  to_ts: null,
  model: null,
  source: null,
  device_scope: null,
}

/** Zero-value UsageStats — shared UI fallback for loading/empty. */
export const ZERO_STATS: UsageStats = {
  request_count: 0,
  total_tokens: 0,
  input_tokens: 0,
  output_tokens: 0,
  cache_creation_tokens: 0,
  cache_read_tokens: 0,
  cache_hit_rate: 0,
  total_cost_usd: 0,
  turn_count: 0,
  avg_turn_duration_ms: 0,
}

export const vaultApi = createApi({
  reducerPath: "vaultApi",
  baseQuery: fakeBaseQuery(),
  tagTypes: ["Usage", "Logs", "Models", "Devices", "Pricing", "App"],
  endpoints: (b) => ({
    // ---- reads ----
    appInfo: b.query<AppInfo, void>({
      queryFn: async () => ({ data: await run(commands.getAppInfo()) }),
      providesTags: ["App"],
    }),
    stats: b.query<UsageStats, UsageFilter>({
      queryFn: async (filter) => ({
        data: await run(commands.queryUsageStats(filter)),
      }),
      providesTags: (_r, _e, filter) => [
        { type: "Usage", id: filterId(filter) },
      ],
    }),
    trend: b.query<TrendPoint[], { filter: UsageFilter; bucket: TrendBucket }>({
      queryFn: async ({ filter, bucket }) => ({
        data: await run(commands.queryUsageTrend(filter, bucket)),
      }),
      providesTags: (_r, _e, { filter }) => [
        { type: "Usage", id: filterId(filter) },
      ],
    }),
    logs: b.query<UsageLogRow[], LogsQuery>({
      queryFn: async (q) => ({ data: await run(commands.queryUsageLogs(q)) }),
      providesTags: (_r, _e, q) => [{ type: "Logs", id: filterId(q.filter) }],
    }),
    count: b.query<number, UsageFilter>({
      queryFn: async (filter) => ({
        data: await run(commands.countUsageLogs(filter)),
      }),
      providesTags: (_r, _e, filter) => [
        { type: "Logs", id: filterId(filter) },
      ],
    }),
    models: b.query<ModelStatsRow[], UsageFilter>({
      queryFn: async (filter) => ({
        data: await run(commands.queryModels(filter)),
      }),
      providesTags: (_r, _e, filter) => [
        { type: "Models", id: filterId(filter) },
      ],
    }),
    distinctSources: b.query<string[], void>({
      queryFn: async () => ({
        data: await run(commands.queryDistinctSources()),
      }),
      providesTags: ["Usage"],
    }),
    distinctModels: b.query<string[], void>({
      queryFn: async () => ({
        data: await run(commands.queryDistinctModels()),
      }),
      providesTags: ["Usage"],
    }),
    devices: b.query<DeviceInfo[], void>({
      queryFn: async () => ({ data: await run(commands.listDevices()) }),
      providesTags: ["Devices"],
    }),
    pricing: b.query<PricingEntry[], void>({
      queryFn: async () => ({ data: await run(commands.listPricing()) }),
      providesTags: ["Pricing"],
    }),

    // ---- mutations ----
    collect: b.mutation<IngestReport, void>({
      queryFn: async () => ({ data: await run(commands.collectNow()) }),
      invalidatesTags: ["Usage", "Logs", "Models", "Devices"],
    }),
    sync: b.mutation<SyncReport, void>({
      queryFn: async () => ({ data: await run(commands.syncNow()) }),
      invalidatesTags: ["Usage", "Logs", "Models", "Devices", "Pricing"],
    }),
    syncConfig: b.mutation<ConfigSyncOutcome, void>({
      queryFn: async () => ({ data: await run(commands.syncConfig()) }),
      invalidatesTags: ["Pricing", "App"],
    }),
    resolveConfigConflict: b.mutation<
      ConfigSyncOutcome,
      ConfigConflictResolution[]
    >({
      queryFn: async (choices) => ({
        data: await run(commands.resolveConfigConflict(choices)),
      }),
      invalidatesTags: ["Pricing", "App"],
    }),
    rebill: b.mutation<number, void>({
      queryFn: async () => ({ data: await run(commands.rebillZeroCost()) }),
      invalidatesTags: ["Usage", "Logs", "Models"],
    }),

    // ---- pricing writes ----
    savePricing: b.mutation<
      null,
      { entry: PricingEntry; isBuiltin: boolean | null }
    >({
      queryFn: async ({ entry, isBuiltin }) => ({
        data: await run(commands.savePricingEntry(entry, isBuiltin)),
      }),
      invalidatesTags: ["Pricing"],
    }),
    deletePricing: b.mutation<null, string>({
      queryFn: async (modelKey) => ({
        data: await run(commands.deletePricingEntry(modelKey)),
      }),
      invalidatesTags: ["Pricing"],
    }),
    reloadPricing: b.mutation<number, void>({
      queryFn: async () => ({
        data: await run(commands.reloadPricingFromFile()),
      }),
      invalidatesTags: ["Pricing"],
    }),
    savePricingToFile: b.mutation<null, void>({
      queryFn: async () => ({ data: await run(commands.savePricingToFile()) }),
    }),
    fetchLitellm: b.mutation<number, void>({
      queryFn: async () => ({
        data: await run(commands.fetchLitellmPricing()),
      }),
      invalidatesTags: ["Pricing"],
    }),

    // ---- device / repo config ----
    setSyncRepo: b.mutation<RunMode, { repoUrl: string; githubToken: string }>({
      queryFn: async ({ repoUrl, githubToken }) => ({
        data: await run(commands.setSyncRepo(repoUrl, githubToken)),
      }),
      invalidatesTags: ["App"],
    }),
    clearSyncRepo: b.mutation<RunMode, void>({
      queryFn: async () => ({ data: await run(commands.clearSyncRepo()) }),
      invalidatesTags: ["App"],
    }),
    setDisplayName: b.mutation<null, string>({
      queryFn: async (displayName) => ({
        data: await run(commands.setDisplayName(displayName)),
      }),
      invalidatesTags: ["App", "Devices"],
    }),
    setDeviceDisplayName: b.mutation<
      null,
      { deviceId: string; displayName: string }
    >({
      queryFn: async ({ deviceId, displayName }) => ({
        data: await run(commands.setDeviceDisplayName(deviceId, displayName)),
      }),
      invalidatesTags: ["Devices"],
    }),

    // ---- preferences (ADR-0012: tray + background) ----
    // Go through the generated `commands.*` so tauri-specta's `typedError`
    // wrapping matches what `run` expects. Raw `invoke` skips that wrapping.
    preferences: b.query<Preferences, void>({
      queryFn: async () => ({ data: await run(commands.getPreferences()) }),
      providesTags: ["App"],
    }),
    setCloseBehavior: b.mutation<Preferences, CloseBehavior>({
      queryFn: async (closeBehavior) => ({
        data: await run(commands.setCloseBehavior(closeBehavior)),
      }),
      invalidatesTags: ["App"],
    }),
    setCollectInterval: b.mutation<Preferences, number>({
      queryFn: async (seconds) => ({
        data: await run(commands.setCollectInterval(seconds)),
      }),
      invalidatesTags: ["App"],
    }),
  }),
})

export const {
  useAppInfoQuery,
  useStatsQuery,
  useTrendQuery,
  useLogsQuery,
  useCountQuery,
  useModelsQuery,
  useDistinctSourcesQuery,
  useDistinctModelsQuery,
  useDevicesQuery,
  usePricingQuery,
  useCollectMutation,
  useSyncMutation,
  useSyncConfigMutation,
  useResolveConfigConflictMutation,
  useRebillMutation,
  useSavePricingMutation,
  useDeletePricingMutation,
  useReloadPricingMutation,
  useSavePricingToFileMutation,
  useFetchLitellmMutation,
  useSetSyncRepoMutation,
  useClearSyncRepoMutation,
  useSetDisplayNameMutation,
  useSetDeviceDisplayNameMutation,
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
} = vaultApi

export type VaultApi = typeof vaultApi

/**
 * Resolve the one-time close dialog (ADR-0012). Not an RTK Query endpoint —
 * it is a one-shot action (hide window / exit app). `remember` pins `choice`.
 */
export async function confirmClose(
  choice: CloseBehavior,
  remember: boolean,
): Promise<void> {
  await run(commands.confirmClose(choice, remember))
}
