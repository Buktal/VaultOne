// Sync-card state machine for the settings view.
//
// Owns the repo-binding lifecycle (probe / bind / unbind / sync-now) and the
// draft inputs that feed the probe. The card JSX in settings-view stays pure
// presentation; this hook is the single source of truth for the interactions.
//
// Two pure decision helpers are split out so the verify state machine is
// unit-testable without a DOM — `buildVerifyArgs` picks the probe payload,
// `resolveVerifyResult` maps an RTK Query outcome to a banner payload. The
// hook wires them into the same path the inline code used to take.

import { useState } from "react"
import { useTranslation } from "react-i18next"
import {
  useAppInfoQuery,
  useClearSyncRepoMutation,
  useSetSyncRepoMutation,
  useSyncMutation,
  useVerifySyncRepoMutation,
} from "@/app/store/api"
import { useFreshness } from "@/hooks/use-freshness"
import { useMutateWithToast } from "@/hooks/use-toast-mutation"
import type { VerifyReport } from "@/types/generated/bindings"

/** Shape of an awaited RTK Query verify mutation result. Both branches are
 *  optional on one object (mirroring `ToastResult` in use-toast-mutation.ts);
 *  the `in`-check at runtime is authoritative (RTK types both fields as
 *  optional, but resolves exactly one). */
export type VerifyMutationResult = { data?: VerifyReport; error?: unknown }

/** Args for the verify probe. When the repo is already bound (synced) we pass
 *  nulls so the backend re-checks the config-stored PAT (the masked token
 *  can't be recovered on the client); otherwise we trim the user's draft. */
export function buildVerifyArgs(
  synced: boolean,
  repoUrl: string,
  token: string,
): { repoUrl: string | null; githubToken: string | null } {
  return synced
    ? { repoUrl: null, githubToken: null }
    : { repoUrl: repoUrl.trim(), githubToken: token.trim() }
}

/** Resolve an RTK Query verify outcome into a banner payload. A
 *  spawn_blocking join failure (rare) surfaces as `{ error }` and falls back
 *  to a generic message; the normal probe failure path is in `r.data.ok`. A
 *  data-less success branch collapses to `null`. */
export function resolveVerifyResult(
  r: VerifyMutationResult,
  fallbackMessage: string,
): VerifyReport | null {
  if ("error" in r) {
    return { ok: false, message: fallbackMessage }
  }
  return r.data ?? null
}

export function useSyncRepo() {
  const { t } = useTranslation()
  const { data: info } = useAppInfoQuery()
  const [setRepo, { isLoading: binding }] = useSetSyncRepoMutation()
  const [clearRepo, { isLoading: clearing }] = useClearSyncRepoMutation()
  const [syncNow, { isLoading: syncing }] = useSyncMutation()
  const [verify, { isLoading: verifying }] = useVerifySyncRepoMutation()
  const runWithToast = useMutateWithToast()
  // 「立即同步」 does the full align (collect + push) — stamp the per-device
  //   "last sync" freshness so the dashboard's "· 同步 X 分钟前" hint lands.
  const { markSynced } = useFreshness()

  const [repoUrl, setRepoUrlRaw] = useState("")
  const [token, setTokenRaw] = useState("")
  const [verifyResult, setVerifyResult] = useState<VerifyReport | null>(null)

  const synced = info?.mode === "synced"

  /** Editing either draft input invalidates the previous probe — clear the
   *  stale banner so the user isn't misled by a result for old values. */
  const setRepoUrl = (v: string) => {
    setRepoUrlRaw(v)
    setVerifyResult(null)
  }
  const setToken = (v: string) => {
    setTokenRaw(v)
    setVerifyResult(null)
  }

  /** 测试连接：未绑定时用输入框里的值校验；已绑定时传 null，由后端读 config
   *  里的原文令牌复查。 */
  const onVerify = async () => {
    setVerifyResult(null)
    const r = await verify(buildVerifyArgs(synced, repoUrl, token))
    setVerifyResult(
      resolveVerifyResult(r, t("settings.sync.verifyRequestFailed")),
    )
  }

  /** Bind + enable sync. On success the draft inputs are cleared directly
   *  (raw setters) — the probe result is intentionally untouched, matching the
   *  prior inline behaviour. */
  const bindRepo = async () => {
    const ok = await runWithToast(
      setRepo,
      { repoUrl: repoUrl.trim(), githubToken: token.trim() },
      {
        success: { key: "settings.toast.syncEnabled" },
        failed: { key: "settings.toast.configFailed" },
      },
    )
    if (ok) {
      setRepoUrlRaw("")
      setTokenRaw("")
    }
  }

  const unbind = async () => {
    await runWithToast(clearRepo, undefined, {
      success: { key: "settings.toast.unbound" },
      failed: { key: "settings.toast.unbindFailed" },
    })
  }

  const syncNowAction = async () => {
    const ok = await runWithToast(syncNow, undefined, {
      success: {
        message: (data) =>
          t("settings.toast.synced", { count: data.imported ?? 0 }) +
          (data.pushed ? t("settings.toast.syncedPushed") : ""),
      },
      failed: { key: "settings.toast.syncFailed" },
    })
    if (ok) markSynced()
  }

  return {
    info,
    synced,
    repoUrl,
    setRepoUrl,
    token,
    setToken,
    verifyResult,
    onVerify,
    bindRepo,
    unbind,
    syncNowAction,
    binding,
    clearing,
    verifying,
    syncing,
  }
}
