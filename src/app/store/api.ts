import { createApi, fakeBaseQuery } from "@reduxjs/toolkit/query/react"
import type {
  AppError,
  AppInfo,
  ConfigConflictResolution,
  ConfigSyncOutcome,
  DeviceInfo,
  DeviceLibrarySummary,
  IngestReport,
  LibraryEntry,
  LibraryForgetAction,
  LogsQuery,
  ModelStatsRow,
  PricingEntry,
  RunMode,
  SyncReport,
  TrendBucket,
  TrendPoint,
  UploadItem,
  UsageFilter,
  UsageLogRow,
  UsageStats,
  VerifyReport,
} from "@/types/generated/bindings"
import {
  type CloseBehavior,
  commands,
  type Language,
  type LightweightExpand,
  type Preferences_Serialize,
  type Skin_Deserialize,
} from "@/types/generated/bindings"

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
  tagTypes: ["Usage", "Logs", "Models", "Devices", "Pricing", "Library", "App"],
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

    // ---- library ----
    scanLibrary: b.query<
      LibraryEntry[],
      { deviceScope: string; subpath: string }
    >({
      queryFn: async ({ deviceScope, subpath }) => ({
        data: await run(commands.scanLibrary(deviceScope, subpath)),
      }),
      providesTags: ["Library"],
    }),
    uploadToLibrary: b.mutation<null, { items: UploadItem[]; subpath: string }>(
      {
        queryFn: async ({ items, subpath }) => ({
          data: await run(commands.uploadToLibrary(items, subpath)),
        }),
        invalidatesTags: ["Library"],
      },
    ),
    exportFromLibrary: b.mutation<null, { relPath: string; targetDir: string }>(
      {
        queryFn: async ({ relPath, targetDir }) => ({
          data: await run(commands.exportFromLibrary(relPath, targetDir)),
        }),
      },
    ),
    deleteFromLibrary: b.mutation<null, string>({
      queryFn: async (relPath) => ({
        data: await run(commands.deleteFromLibrary(relPath)),
      }),
      invalidatesTags: ["Library"],
    }),
    renameInLibrary: b.mutation<null, { relPath: string; newName: string }>({
      queryFn: async ({ relPath, newName }) => ({
        data: await run(commands.renameInLibrary(relPath, newName)),
      }),
      invalidatesTags: ["Library"],
    }),
    /** Pre-flight file/folder counts for one device's library subtree — drives
     *  the forget-device dialog's migrate-vs-delete choice. Read-only probe. */
    libraryDeviceSummary: b.query<DeviceLibrarySummary, string>({
      queryFn: async (deviceId) => ({
        data: await run(commands.libraryDeviceSummary(deviceId)),
      }),
    }),

    // ---- device / repo config ----
    setSyncRepo: b.mutation<RunMode, { repoUrl: string; githubToken: string }>({
      queryFn: async ({ repoUrl, githubToken }) => ({
        data: await run(commands.setSyncRepo(repoUrl, githubToken)),
      }),
      invalidatesTags: ["App"],
    }),
    verifySyncRepo: b.mutation<
      VerifyReport,
      { repoUrl: string | null; githubToken: string | null }
    >({
      queryFn: async ({ repoUrl, githubToken }) => ({
        data: await run(commands.verifySyncRepo(repoUrl, githubToken)),
      }),
      // Probe is read-only (ls-remote) — never invalidates any cache.
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
    forgetDevice: b.mutation<
      null,
      { deviceId: string; libraryAction: LibraryForgetAction }
    >({
      queryFn: async ({ deviceId, libraryAction }) => ({
        data: await run(commands.forgetDevice(deviceId, libraryAction)),
      }),
      // "Library" too: migrate/delete rewrites the library listing.
      invalidatesTags: ["Devices", "Usage", "Logs", "Models", "Library"],
    }),

    // ---- preferences ----
    // Go through the generated `commands.*` so tauri-specta's `typedError`
    // wrapping matches what `run` expects. Raw `invoke` skips that wrapping.
    preferences: b.query<Preferences_Serialize, void>({
      queryFn: async () => ({ data: await run(commands.getPreferences()) }),
      providesTags: ["App"],
    }),
    setCloseBehavior: b.mutation<Preferences_Serialize, CloseBehavior>({
      queryFn: async (closeBehavior) => ({
        data: await run(commands.setCloseBehavior(closeBehavior)),
      }),
      invalidatesTags: ["App"],
    }),
    setCollectInterval: b.mutation<Preferences_Serialize, number>({
      queryFn: async (seconds) => ({
        data: await run(commands.setCollectInterval(seconds)),
      }),
      invalidatesTags: ["App"],
    }),
    setPushInterval: b.mutation<Preferences_Serialize, number>({
      queryFn: async (seconds) => ({
        data: await run(commands.setPushInterval(seconds)),
      }),
      invalidatesTags: ["App"],
    }),
    setLanguage: b.mutation<Preferences_Serialize, Language>({
      queryFn: async (language) => ({
        data: await run(commands.setLanguage(language)),
      }),
      invalidatesTags: ["App"],
    }),
    setLightweightExpand: b.mutation<Preferences_Serialize, LightweightExpand>({
      queryFn: async (mode) => ({
        data: await run(commands.setLightweightExpand(mode)),
      }),
      invalidatesTags: ["App"],
    }),
    setSkin: b.mutation<Preferences_Serialize, Skin_Deserialize>({
      queryFn: async (skin) => ({
        data: await run(commands.setSkin(skin)),
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
  useScanLibraryQuery,
  useUploadToLibraryMutation,
  useExportFromLibraryMutation,
  useDeleteFromLibraryMutation,
  useRenameInLibraryMutation,
  useLibraryDeviceSummaryQuery,
  useSetSyncRepoMutation,
  useVerifySyncRepoMutation,
  useClearSyncRepoMutation,
  useSetDisplayNameMutation,
  useSetDeviceDisplayNameMutation,
  useForgetDeviceMutation,
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
  useSetPushIntervalMutation,
  useSetLanguageMutation,
  useSetLightweightExpandMutation,
  useSetSkinMutation,
} = vaultApi

export type VaultApi = typeof vaultApi

/**
 * Resolve the one-time close dialog. Not an RTK Query endpoint —
 * it is a one-shot action (hide window / exit app). `remember` pins `choice`.
 */
export async function confirmClose(
  choice: CloseBehavior,
  remember: boolean,
): Promise<void> {
  await run(commands.confirmClose(choice, remember))
}
