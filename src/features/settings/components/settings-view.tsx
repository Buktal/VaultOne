// Settings view: device identity, run mode, repo binding
// (Standalone ↔ Synced), manual sync / rebill.
//
// Five sectioned cards (通用 / 本机 / 设备 / 同步 / 维护), each fronted by an
// eyebrow label. The sync card holds the repo binding + a manual「立即同步」
// entry (same align as the dashboard button — collect + pull + push in Synced
// mode). Pricing is a per-device local concern now, so there is no separate
// config-sync entry.

import {
  Calculator,
  CheckCircle2,
  CloudUpload,
  Loader2,
  PlugZap,
  RefreshCw,
  Unplug,
  XCircle,
} from "lucide-react"
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  useAppInfoQuery,
  useClearSyncRepoMutation,
  useRebillMutation,
  useSetDisplayNameMutation,
  useSetSyncRepoMutation,
  useSyncMutation,
  useVerifySyncRepoMutation,
} from "@/app/store/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { DeviceList } from "@/features/settings/components/device-list"
import { GeneralCard } from "@/features/settings/components/general-card"
import { describeError } from "@/lib/error"
import type { VerifyReport } from "@/types/generated/bindings"

export function SettingsView() {
  const { t } = useTranslation()
  const { data: info } = useAppInfoQuery()
  const [setRepo, { isLoading: binding }] = useSetSyncRepoMutation()
  const [clearRepo, { isLoading: clearing }] = useClearSyncRepoMutation()
  const [setName, { isLoading: naming }] = useSetDisplayNameMutation()
  const [rebill, { isLoading: rebilling }] = useRebillMutation()
  const [syncNow, { isLoading: syncing }] = useSyncMutation()
  const [verify, { isLoading: verifying }] = useVerifySyncRepoMutation()

  const [displayName, setDisplayName] = useState("")
  const [repoUrl, setRepoUrl] = useState("")
  const [token, setToken] = useState("")
  const [verifyResult, setVerifyResult] = useState<VerifyReport | null>(null)

  const synced = info?.mode === "synced"
  const unknown = t("common.unknownReason")

  /** 测试连接：未绑定时用输入框里的值校验；已绑定时传 null，由后端读 config
   *  里的原文令牌复查（脱敏 token 拿不到原文）。改输入框会先清掉旧结果。 */
  const onVerify = async () => {
    setVerifyResult(null)
    const r = await verify(
      synced
        ? { repoUrl: null, githubToken: null }
        : { repoUrl: repoUrl.trim(), githubToken: token.trim() },
    )
    if ("error" in r) {
      // 只有 spawn_blocking join 失败才会走到这（罕见）；正常探活失败在 r.data.ok。
      setVerifyResult({
        ok: false,
        message: t("settings.sync.verifyRequestFailed"),
      })
      return
    }
    setVerifyResult(r.data ?? null)
  }

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6">
      {/* 通用 — tray / language / update */}
      <Section
        eyebrow={t("settings.section.general")}
        description={t("settings.sectionDesc.general")}
      >
        <GeneralCard />
      </Section>

      {/* 本机 */}
      <Section
        eyebrow={t("settings.section.local")}
        description={t("settings.sectionDesc.local")}
      >
        <Row label={t("settings.local.deviceId")}>
          <code className="bg-muted rounded px-2 py-1 font-mono text-xs">
            {info?.device_id ?? "—"}
          </code>
        </Row>
        <Row label={t("settings.local.runMode")}>
          <Badge variant={synced ? "default" : "secondary"}>
            {synced ? t("settings.local.modeSynced") : t("shell.standalone")}
          </Badge>
        </Row>
        <Row label={t("settings.local.claudeLogDir")}>
          <span className="text-muted-foreground truncate font-mono text-xs">
            {info?.claude_projects_dir ?? "—"}
          </span>
        </Row>
        <div className="bg-border h-px" />
        <div className="flex flex-col gap-2">
          <Label className="text-muted-foreground text-xs">
            {t("settings.local.displayName")}
          </Label>
          <div className="flex items-center gap-2">
            <Input
              className="flex-1"
              placeholder={info?.display_name ?? "Device"}
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
            />
            <Button
              size="sm"
              disabled={naming || !displayName.trim()}
              onClick={async () => {
                const r = await setName(displayName.trim())
                if ("error" in r) toast.error(t("settings.toast.renameFailed"))
                else {
                  toast.success(t("settings.toast.displayNameUpdated"))
                  setDisplayName("")
                }
              }}
            >
              {t("common.save")}
            </Button>
          </div>
        </div>
      </Section>

      {/* 设备 */}
      <Section
        eyebrow={t("settings.section.devices")}
        description={t("settings.sectionDesc.devices")}
      >
        <DeviceList />
      </Section>

      {/* 同步 — 仓库绑定 + 用量同步 */}
      <Section
        eyebrow={t("settings.section.sync")}
        description={t("settings.sectionDesc.sync")}
      >
        <Row label={t("settings.sync.currentRepo")}>
          <span className="text-muted-foreground font-mono text-xs">
            {info?.repo_url ?? t("settings.sync.notConfigured")}
          </span>
        </Row>
        <Row label="Token">
          <span className="text-muted-foreground font-mono text-xs">
            {info?.masked_token ?? t("settings.sync.notConfigured")}
          </span>
        </Row>
        <div className="bg-border h-px" />
        <div className="flex flex-col gap-2">
          <Label className="text-muted-foreground text-xs">
            {t("settings.sync.repoUrl")}
          </Label>
          <Input
            placeholder="https://github.com/<owner>/<repo>.git"
            value={repoUrl}
            onChange={(e) => {
              setRepoUrl(e.target.value)
              setVerifyResult(null)
            }}
            disabled={synced}
          />
          <Label className="text-muted-foreground text-xs">
            {t("settings.sync.githubToken")}
          </Label>
          <Input
            type="password"
            placeholder="github_pat_…"
            value={token}
            onChange={(e) => {
              setToken(e.target.value)
              setVerifyResult(null)
            }}
            disabled={synced}
          />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={
              verifying || (!synced && (!repoUrl.trim() || !token.trim()))
            }
            onClick={onVerify}
          >
            <PlugZap className="size-4" />
            {verifying
              ? t("settings.sync.verifying")
              : t("settings.sync.testConnection")}
          </Button>
          <Button
            size="sm"
            disabled={binding || synced || !repoUrl.trim() || !token.trim()}
            onClick={async () => {
              const r = await setRepo({
                repoUrl: repoUrl.trim(),
                githubToken: token.trim(),
              })
              if ("error" in r)
                toast.error(t("settings.toast.configFailed"), {
                  description: describeError(r.error) || unknown,
                })
              else {
                toast.success(t("settings.toast.syncEnabled"))
                setRepoUrl("")
                setToken("")
              }
            }}
          >
            <CloudUpload className="size-4" />
            {t("settings.sync.bindAndEnable")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={clearing || !synced}
            onClick={async () => {
              const r = await clearRepo()
              if ("error" in r)
                toast.error(t("settings.toast.unbindFailed"), {
                  description: describeError(r.error) || unknown,
                })
              else toast.success(t("settings.toast.unbound"))
            }}
          >
            <Unplug className="size-4" />
            {t("settings.sync.unbind")}
          </Button>
          {synced && (
            <Button
              variant="outline"
              size="sm"
              disabled={syncing}
              onClick={async () => {
                const r = await syncNow()
                if ("error" in r)
                  toast.error(t("settings.toast.syncFailed"), {
                    description: describeError(r.error) || unknown,
                  })
                else
                  toast.success(
                    t("settings.toast.synced", {
                      count: r.data?.imported ?? 0,
                    }) +
                      (r.data?.pushed ? t("settings.toast.syncedPushed") : ""),
                  )
              }}
            >
              <RefreshCw className="size-4" />
              {syncing
                ? t("settings.sync.syncing")
                : t("settings.sync.syncNow")}
            </Button>
          )}
        </div>
        {(verifying || verifyResult) && (
          <VerifyBanner verifying={verifying} result={verifyResult} />
        )}
      </Section>

      {/* 维护 */}
      <Section
        eyebrow={t("settings.section.maintenance")}
        description={t("settings.sectionDesc.maintenance")}
      >
        <Row label={t("settings.maintenance.rebillLabel")}>
          <Button
            variant="outline"
            size="sm"
            disabled={rebilling}
            onClick={async () => {
              const r = await rebill()
              if ("error" in r) toast.error(t("settings.toast.rebillFailed"))
              else
                toast.success(
                  t("settings.toast.rebilled", { count: r.data ?? 0 }),
                )
            }}
          >
            <Calculator className="size-4" />
            {rebilling
              ? t("settings.maintenance.rebilling")
              : t("settings.maintenance.rebillButton")}
          </Button>
        </Row>
      </Section>
    </div>
  )
}

function Section({
  eyebrow,
  description,
  children,
}: {
  eyebrow: string
  description?: string
  children: React.ReactNode
}) {
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex flex-col gap-1 px-0.5">
        <h2 className="text-muted-foreground text-[11px] font-semibold tracking-[0.14em]">
          {eyebrow}
        </h2>
        {description ? (
          <p className="text-muted-foreground/70 text-xs leading-relaxed">
            {description}
          </p>
        ) : null}
      </div>
      <Card interactive>
        <CardContent className="flex flex-col gap-3">{children}</CardContent>
      </Card>
    </section>
  )
}

function Row({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-muted-foreground text-sm">{label}</span>
      {children}
    </div>
  )
}

/** 测试连接结果 banner（诊断型操作，结果需持久可见，故用 inline 而非 toast）。
 *  `result.message` 来自 Rust 后端，保持英文不本地化。 */
function VerifyBanner({
  verifying,
  result,
}: {
  verifying: boolean
  result: VerifyReport | null
}) {
  const { t } = useTranslation()
  if (verifying) {
    return (
      <div className="bg-muted/50 text-muted-foreground flex items-center gap-2 rounded-md border border-dashed p-2 text-xs">
        <Loader2 className="size-3.5 animate-spin" />
        {t("settings.sync.verifyingBanner")}
      </div>
    )
  }
  if (!result) return null
  if (result.ok) {
    return (
      <div className="border-emerald-500/40 bg-emerald-500/5 text-emerald-600 dark:text-emerald-400 flex flex-col gap-0.5 rounded-md border p-2 text-xs leading-relaxed">
        <span className="flex items-start gap-2">
          <CheckCircle2 className="mt-0.5 size-3.5 shrink-0" />
          {result.message}
        </span>
        <span className="text-muted-foreground pl-5">
          {t("settings.sync.verifyReadPermNote")}
        </span>
      </div>
    )
  }
  return (
    <div className="border-destructive/40 bg-destructive/5 text-destructive flex items-start gap-2 rounded-md border p-2 text-xs leading-relaxed">
      <XCircle className="mt-0.5 size-3.5 shrink-0" />
      <span>{result.message}</span>
    </div>
  )
}
