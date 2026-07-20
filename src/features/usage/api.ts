// Usage feature RTK Query endpoints (ADR-0008: queryFn calls typed commands).
// Each query reads the Local Store (SQLite); a collect mutation invalidates
// the Usage tag so the dashboard refreshes.

import { api } from "@/app/store/api"
import type {
  IngestReport,
  LogsQuery,
  ModelStatsRow,
  TrendPoint,
  UsageFilter,
  UsageLogRow,
  UsageStats,
} from "@/types/generated/bindings"
import { commands } from "@/types/generated/bindings"

type SpectaResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: unknown }

/** Await a tauri-specta command promise and map it into RTK Query's
 *  `{ data } | { error }` (ADR-0008). */
async function unwrap<T>(
  p: Promise<SpectaResult<T>>,
): Promise<{ data: T } | { error: unknown }> {
  const res = await p
  return res.status === "ok" ? { data: res.data } : { error: res.error }
}

export const usageApi = api.injectEndpoints({
  endpoints: (build) => ({
    queryUsageStats: build.query<UsageStats, UsageFilter>({
      queryFn: (filter) => unwrap(commands.queryUsageStats(filter)),
      providesTags: (_r, _e, filter) => [
        { type: "Usage" as const, id: filterToId(filter) },
      ],
    }),
    queryUsageTrend: build.query<TrendPoint[], UsageFilter>({
      queryFn: (filter) => unwrap(commands.queryUsageTrend(filter)),
      providesTags: (_r, _e, filter) => [
        { type: "Usage" as const, id: `trend-${filterToId(filter)}` },
      ],
    }),
    queryUsageLogs: build.query<UsageLogRow[], LogsQuery>({
      queryFn: (query) => unwrap(commands.queryUsageLogs(query)),
      providesTags: (_r, _e, query) => [
        { type: "Usage" as const, id: `logs-${query.offset}` },
      ],
    }),
    countUsageLogs: build.query<number, UsageFilter>({
      queryFn: (filter) => unwrap(commands.countUsageLogs(filter)),
      providesTags: ["Usage"],
    }),
    queryModels: build.query<ModelStatsRow[], UsageFilter>({
      queryFn: (filter) => unwrap(commands.queryModels(filter)),
      // Filter-scoped cache key (else picking a model to narrow the filter
      // wouldn't re-fetch — the ModelDistribution list would stay stale).
      providesTags: (_r, _e, filter) => [
        { type: "Usage" as const, id: `models-${filterToId(filter)}` },
      ],
    }),
    queryDistinctSources: build.query<string[], void>({
      queryFn: () => unwrap(commands.queryDistinctSources()),
      providesTags: ["Usage"],
    }),
    queryDistinctModels: build.query<string[], void>({
      queryFn: () => unwrap(commands.queryDistinctModels()),
      providesTags: ["Usage"],
    }),
    collectNow: build.mutation<IngestReport, void>({
      queryFn: () => unwrap(commands.collectNow()),
      invalidatesTags: ["Usage"],
    }),
    rebillZeroCost: build.mutation<number, void>({
      queryFn: () => unwrap(commands.rebillZeroCost()),
      invalidatesTags: ["Usage"],
    }),
  }),
})

export const {
  useQueryUsageStatsQuery,
  useQueryUsageTrendQuery,
  useQueryUsageLogsQuery,
  useCountUsageLogsQuery,
  useQueryModelsQuery,
  useQueryDistinctSourcesQuery,
  useQueryDistinctModelsQuery,
  useCollectNowMutation,
  useRebillZeroCostMutation,
} = usageApi

/** Stable cache id derived from the semantic filter (ADR-0008: no display in key). */
export function filterToId(f: UsageFilter): string {
  return [
    f.from_day ?? "",
    f.to_day ?? "",
    f.model ?? "",
    f.source ?? "",
    f.device_scope ?? "",
  ].join("|")
}
