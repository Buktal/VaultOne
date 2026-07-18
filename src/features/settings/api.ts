// Settings feature RTK Query endpoints (ADR-0008): app info, mode, devices,
// repo binding (Standalone ↔ Synced, ADR-0011).

import { api } from "@/app/store/api"
import type {
  AppInfo,
  ConfigConflictResolution,
  ConfigSyncOutcome,
  DeviceInfo,
  RunMode,
  SyncReport,
} from "@/types/generated/bindings"
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

export const settingsApi = api.injectEndpoints({
  endpoints: (build) => ({
    getAppInfo: build.query<AppInfo, void>({
      queryFn: () => unwrap(commands.getAppInfo()),
      providesTags: ["App"],
    }),
    setSyncRepo: build.mutation<
      RunMode,
      { repo_url: string; github_token: string }
    >({
      queryFn: ({ repo_url, github_token }) =>
        unwrap(commands.setSyncRepo(repo_url, github_token)),
      invalidatesTags: ["App", "Sync"],
    }),
    clearSyncRepo: build.mutation<RunMode, void>({
      queryFn: () => unwrap(commands.clearSyncRepo()),
      invalidatesTags: ["App", "Sync"],
    }),
    setDisplayName: build.mutation<null, string>({
      queryFn: (display_name) => unwrap(commands.setDisplayName(display_name)),
      invalidatesTags: ["App", "Device"],
    }),
    setDeviceDisplayName: build.mutation<
      null,
      { device_id: string; display_name: string }
    >({
      queryFn: ({ device_id, display_name }) =>
        unwrap(commands.setDeviceDisplayName(device_id, display_name)),
      invalidatesTags: ["Device"],
    }),
    listDevices: build.query<DeviceInfo[], void>({
      queryFn: () => unwrap(commands.listDevices()),
      providesTags: ["Device"],
    }),
    syncNow: build.mutation<SyncReport, void>({
      queryFn: () => unwrap(commands.syncNow()),
      invalidatesTags: ["Usage", "Device", "Sync"],
    }),
    syncConfig: build.mutation<ConfigSyncOutcome, void>({
      queryFn: () => unwrap(commands.syncConfig()),
      invalidatesTags: ["Usage", "Device", "Pricing", "Sync"],
    }),
    resolveConfigConflict: build.mutation<
      ConfigSyncOutcome,
      ConfigConflictResolution[]
    >({
      queryFn: (choices) => unwrap(commands.resolveConfigConflict(choices)),
      invalidatesTags: ["Usage", "Device", "Pricing", "Sync"],
    }),
  }),
})

export const {
  useGetAppInfoQuery,
  useSetSyncRepoMutation,
  useClearSyncRepoMutation,
  useSetDisplayNameMutation,
  useSetDeviceDisplayNameMutation,
  useListDevicesQuery,
  useSyncNowMutation,
  useSyncConfigMutation,
  useResolveConfigConflictMutation,
} = settingsApi
