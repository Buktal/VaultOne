// Settings view (ADR-0011): device identity, run mode, repo binding
// (Standalone ↔ Synced), manual collect / rebill.

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
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ConflictResolver } from "@/features/settings/components/conflict-resolver"
import { DeviceList } from "@/features/settings/components/device-list"
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
    <div className="mx-auto flex max-w-2xl flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>设备身份</CardTitle>
          <CardDescription>
            设备 ID 是多设备同步的唯一键（ADR-0002）；显示名仅用于展示。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
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
          <div className="mt-2 flex items-end gap-2">
            <div className="flex flex-1 flex-col gap-1.5">
              <Label className="text-muted-foreground text-xs">
                设备显示名
              </Label>
              <Input
                placeholder={info?.display_name ?? "Device"}
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
            </div>
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
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>多设备同步（GitHub 仓库）</CardTitle>
          <CardDescription>
            配置仓库 + fine-grained PAT 升级为 Synced（ADR-0011）。PAT
            仅存本地，绝不进入仓库。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
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
              Fine-grained PAT（Contents 读写）
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
                  toast.success("已升级为 Synced 模式")
                  setRepoUrl("")
                  setToken("")
                }
              }}
            >
              <CloudUpload className="size-4" />
              绑定并升级
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={clearing || !synced}
              onClick={async () => {
                const r = await clearRepo()
                if ("error" in r) toast.error("解绑失败")
                else toast.success("已切回 Standalone（本地数据保留）")
              }}
            >
              <Unplug className="size-4" />
              解绑（降级单机）
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

          {/* Cloud-config manual sync (ADR-0005 / #6) */}
          <div className="mt-2 flex flex-col gap-2 border-t pt-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex flex-col">
                <span className="text-sm font-medium">云配置同步</span>
                <span className="text-muted-foreground text-xs">
                  手动同步 app / user / pricing.json 云配置（ADR-0005/#6）。
                </span>
              </div>
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
            {!synced && (
              <p className="text-muted-foreground text-xs">
                需先配置 GitHub 仓库升级为 Synced（上方）后，云配置同步才可用。
              </p>
            )}
            {conflicts && conflicts.length > 0 && (
              <ConflictResolver conflicts={conflicts} />
            )}
          </div>
        </CardContent>
      </Card>

      <DeviceList />

      <Card>
        <CardHeader>
          <CardTitle>维护</CardTitle>
          <CardDescription>
            补算零成本行（ADR-0009：只补当初缺价的行）。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={rebilling}
            onClick={async () => {
              const r = await rebill()
              if ("error" in r) toast.error("回算失败")
              else toast.success(`已补算 ${r.data ?? 0} 行`)
            }}
          >
            <Calculator className="size-4" />
            {rebilling ? "回算中…" : "补算零成本"}
          </Button>
        </CardContent>
      </Card>
    </div>
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
