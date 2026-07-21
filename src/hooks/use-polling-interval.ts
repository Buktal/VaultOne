// Shared polling cadence (ADR-0011). Returns ms from uiSlice.refreshInterval;
// 0 = off (RTK Query treats 0 as no polling). Used by every dashboard / logs
// query so the toolbar's refresh picker drives them all — replaces the
// five hardcoded 30_000 sites.

import { useAppSelector } from "@/app/store/hooks"

export function usePollingInterval(): number {
  return useAppSelector((s) => s.ui.refreshInterval) * 1000
}
