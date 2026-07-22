// Settings view (ADR-0011): device identity, run mode, repo binding
// (Standalone ↔ Synced), manual collect / rebill.
//
// Redesigned into 6 sectioned cards (通用 / 本机 / 同步 / 云配置 / 设备 / 维护),
// each fronted by an eyebrow label. Cloud-config sync was split out of the
// sync card into its own section (ADR-0005 / #6) so it no longer shares a
// border-t with the usage-sync controls. Behaviour unchanged — layout only.

import { Calculator, CloudUpload, RefreshCw, Unplug } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"
import {
  useAppInfoQuery,
  useClearSyncRepoMutation,
  useRebillMutation,
  useSetDisplayNameMutation,
  useSetSyncRepoMutation,
  useSyncConfigMutation,
  useSyncMutation,
} from "@/app/store/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ConflictResolver } from "@/features/settings/components/conflict-resolver"
import { DeviceList } from "@/features/settings/components/device-list"
import { GeneralCard } from "@/features/settings/components/general-card"
import type { ConfigConflict } from "@/types/generated/bindings"

export function SettingsView() {
  const { data: info } = useAppInfoQuery()
  const [setRepo, { isLoading: binding }] = useSetSyncRepoMutation()
  const [clearRepo, { isLoading: clearing }] = useClearSyncRepoMutation()
  const [setName, { isLoading: naming }] = useSetDisplayNameMutation()
  const [rebill, { isLoading: rebilling }] = useRebillMutation()
  const [syncNow, { isLoading: syncing }] = useSyncMutation()
  const [syncConfig, { isLoading: syncingConfig }] = useSyncConfigMutation()

  const [displayName, setDisplayName] = useState("")
  const [repoUrl, setRepoUrl] = useState("")
  const [token, setToken] = useState("")
  const [conflicts, setConflicts] = useState<ConfigConflict[] | null>(null)

  const synced = info?.mode === "synced"

  const onSyncConfig = async () => {
    setConflicts(null)
    const r = await syncConfig()
    if ("error" in r) {
      toast.error("云配置同步失败")
      return
    }
    const o = r.data
    if (o?.has_conflict && o.conflicts.length > 0) {
      setConflicts(o.conflicts)
      toast.warning(`检测到 ${o.conflicts.length} 个冲突，请逐个选择保留版本`)
      return
    }
    toast.success(
      `云配置已同步${o?.pushed ? "（已推送本地改动）" : ""}${o?.pricing_changed ? "，定价已更新" : ""}`,
    )
  }

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6">
      {/* 通用 — tray / ADR-0012 */}
      <Section
        eyebrow="通用"
        description="应用常驻系统托盘。关窗后默认最小化到托盘、继续在后台定时读取使用记录。"
      >
        <GeneralCard />
      </Section>

      {/* 本机 */}
      <Section
        eyebrow="本机"
        description="设备 ID 用于在多台设备间区分这台机器，显示名只是方便你辨认。"
      >
        <Row label="设备 ID">
          <code className="bg-muted rounded px-2 py-1 font-mono text-xs">
            {info?.device_id ?? "—"}
          </code>
        </Row>
        <Row label="运行模式">
          <Badge variant={synced ? "default" : "secondary"}>
            {synced ? "已同步（多设备）" : "单机"}
          </Badge>
        </Row>
        <Row label="Claude 日志目录">
          <span className="text-muted-foreground truncate font-mono text-xs">
            {info?.claude_projects_dir ?? "—"}
          </span>
        </Row>
        <div className="bg-border h-px" />
        <div className="flex flex-col gap-2">
          <Label className="text-muted-foreground text-xs">设备显示名</Label>
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
                if ("error" in r) toast.error("重命名失败")
                else {
                  toast.success("已更新显示名")
                  setDisplayName("")
                }
              }}
            >
              保存
            </Button>
          </div>
        </div>
      </Section>

      {/* 同步 */}
      <Section
        eyebrow="同步"
        description="绑定一个私有 Git 仓库来开启多设备同步。访问令牌只存在本机，不会写入仓库。"
      >
        <Row label="当前仓库">
          <span className="text-muted-foreground font-mono text-xs">
            {info?.repo_url ?? "（未配置）"}
          </span>
        </Row>
        <Row label="Token">
          <span className="text-muted-foreground font-mono text-xs">
            {info?.masked_token ?? "（未配置）"}
          </span>
        </Row>
        <div className="bg-border h-px" />
        <div className="flex flex-col gap-2">
          <Label className="text-muted-foreground text-xs">
            仓库 URL（HTTPS）
          </Label>
          <Input
            placeholder="https://github.com/<owner>/<repo>.git"
            value={repoUrl}
            onChange={(e) => setRepoUrl(e.target.value)}
            disabled={synced}
          />
          <Label className="text-muted-foreground text-xs">
            GitHub 访问令牌（需 Contents 读写权限）
          </Label>
          <Input
            type="password"
            placeholder="github_pat_…"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            disabled={synced}
          />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            disabled={binding || synced || !repoUrl.trim() || !token.trim()}
            onClick={async () => {
              const r = await setRepo({
                repoUrl: repoUrl.trim(),
                githubToken: token.trim(),
              })
              if ("error" in r) toast.error("配置失败")
              else {
                toast.success("已开启多设备同步")
                setRepoUrl("")
                setToken("")
              }
            }}
          >
            <CloudUpload className="size-4" />
            绑定并开启同步
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={clearing || !synced}
            onClick={async () => {
              const r = await clearRepo()
              if ("error" in r) toast.error("解绑失败")
              else toast.success("已切回单机（本地数据保留）")
            }}
          >
            <Unplug className="size-4" />
            解绑（切回单机）
          </Button>
          {synced && (
            <Button
              variant="outline"
              size="sm"
              disabled={syncing}
              onClick={async () => {
                const r = await syncNow()
                if ("error" in r) toast.error("同步失败")
                else
                  toast.success(
                    `已同步（导入 ${r.data?.imported ?? 0} 行${r.data?.pushed ? "，已推送" : ""}）`,
                  )
              }}
            >
              <RefreshCw className="size-4" />
              {syncing ? "同步中…" : "立即同步"}
            </Button>
          )}
        </div>
      </Section>

      {/* 云配置 — split out of the sync card (ADR-0005 / #6) */}
      <Section
        eyebrow="云配置"
        description="手动拉取或推送云端的应用、用户、定价三类配置。仅在多设备同步模式下可用。"
      >
        <div className="flex items-center justify-between gap-3">
          <span className="text-muted-foreground text-sm">
            {synced
              ? "应用 / 用户 / 定价 三类配置"
              : "需先在上方开启多设备同步"}
          </span>
          {synced && (
            <Button
              variant="outline"
              size="sm"
              disabled={syncingConfig}
              onClick={onSyncConfig}
            >
              <RefreshCw className="size-4" />
              {syncingConfig ? "同步中…" : "同步云配置"}
            </Button>
          )}
        </div>
        {conflicts && conflicts.length > 0 && (
          <ConflictResolver conflicts={conflicts} />
        )}
      </Section>

      {/* 设备 */}
      <Section
        eyebrow="设备"
        description="所有同步过的设备。可以给其他设备起个好认的名字。"
      >
        <DeviceList />
      </Section>

      {/* 维护 */}
      <Section
        eyebrow="维护"
        description="为早期缺少定价、记为 0 成本的历史记录，用当前定价补上金额。"
      >
        <Row label="补算缺失成本">
          <Button
            variant="outline"
            size="sm"
            disabled={rebilling}
            onClick={async () => {
              const r = await rebill()
              if ("error" in r) toast.error("补算失败")
              else toast.success(`已补算 ${r.data ?? 0} 条`)
            }}
          >
            <Calculator className="size-4" />
            {rebilling ? "补算中…" : "补算"}
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
