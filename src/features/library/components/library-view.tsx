// Library view — per-device, git-mediated cloud storage. Drag files / dirs in
// to upload (= push to the sync repo); drill into directories (the same surface
// at every depth — drag-in, export, single-file download all work inside);
// preview a file in the webview; export to a path you choose. VaultOne never
// writes into an AI tool's own config dir.

import { getCurrentWebview } from "@tauri-apps/api/webview"
import { open } from "@tauri-apps/plugin-dialog"
import dayjs from "dayjs"
import relativeTime from "dayjs/plugin/relativeTime"
import {
  ArrowUp,
  Check,
  ChevronRight,
  Download,
  File as FileIcon,
  FileJson,
  FilePlus,
  FileText,
  Folder,
  Image as ImageIcon,
  Loader2,
  Pencil,
  Trash2,
  X,
} from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  useDeleteFromLibraryMutation,
  useDevicesQuery,
  useExportFromLibraryMutation,
  useRenameInLibraryMutation,
  useScanLibraryQuery,
} from "@/app/store/api"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import type { LibraryEntry } from "@/types/generated/bindings"
import { PreviewSheet } from "./preview-sheet"
import { UploadDialog } from "./upload-dialog"

dayjs.extend(relativeTime)

const ALL = "__all__"

function kindIcon(name: string, isDir: boolean) {
  if (isDir) return Folder
  const ext = name.split(".").pop()?.toLowerCase()
  if (!ext) return FileIcon
  if (ext === "json") return FileJson
  if (["md", "markdown", "txt", "log"].includes(ext)) return FileText
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"].includes(ext))
    return ImageIcon
  return FileIcon
}

function formatSize(bytes: number | null): string {
  if (!bytes) return "—"
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}

export function LibraryView() {
  const { t } = useTranslation()
  const [deviceScope, setDeviceScope] = useState<string>(ALL)
  const [subpath, setSubpath] = useState("")
  const [dragging, setDragging] = useState(false)
  const [pendingPaths, setPendingPaths] = useState<string[] | null>(null)
  const [preview, setPreview] = useState<LibraryEntry | null>(null)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [renameVal, setRenameVal] = useState("")
  const [busyRelPath, setBusyRelPath] = useState<string | null>(null)

  const atRoot = subpath === ""
  const scope = deviceScope === ALL ? "all" : deviceScope
  const { data: entries = [], isLoading } = useScanLibraryQuery({
    deviceScope: scope,
    subpath,
  })
  // Same source as the logs/dashboard device picker (listDevices), but NOT
  // filtered down to ≤1 — Library always lists every known device, even this
  // machine alone, so the picker is never empty.
  const { data: devices = [] } = useDevicesQuery()
  const [exportMut] = useExportFromLibraryMutation()
  const [deleteMut] = useDeleteFromLibraryMutation()
  const [renameMut] = useRenameInLibraryMutation()

  // Webview-level file drag-drop → collect dropped paths into the pending
  // upload dialog. (HTML5 drop events don't expose local file paths under
  // Tauri; onDragDropEvent is the supported path.)
  useEffect(() => {
    let active = true
    let unlisten: (() => void) | undefined
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload
        if (p.type === "enter" || p.type === "over") setDragging(true)
        else if (p.type === "leave") setDragging(false)
        else if (p.type === "drop") {
          setDragging(false)
          if (p.paths.length > 0) setPendingPaths(p.paths)
        }
      })
      .then((un) => {
        if (active) unlisten = un
        else un()
      })
    return () => {
      active = false
      unlisten?.()
    }
  }, [])

  const deviceOptions = useMemo(
    () =>
      devices.map((d) => ({
        id: d.device_id,
        label: d.is_self
          ? t("devices.thisDevice")
          : d.display_name || t("common.unnamed"),
      })),
    [devices, t],
  )

  const breadcrumb = useMemo(() => {
    if (atRoot)
      return [] as Array<{ key: string; label: string; onClick: () => void }>
    const parts = subpath.split("/").filter(Boolean)
    const deviceId = parts[0]
    const deviceLabel =
      deviceOptions.find((o) => o.id === deviceId)?.label ?? deviceId
    const crumbs: Array<{ key: string; label: string; onClick: () => void }> = [
      {
        key: deviceId,
        label: deviceLabel,
        onClick: () => {
          setDeviceScope(deviceId)
          setSubpath("")
        },
      },
    ]
    for (let i = 1; i < parts.length; i++) {
      const sub = parts.slice(1, i + 1).join("/")
      crumbs.push({
        key: `${deviceId}/${sub}`,
        label: parts[i],
        onClick: () => {
          setDeviceScope(deviceId)
          setSubpath(sub)
        },
      })
    }
    return crumbs
  }, [subpath, atRoot, deviceOptions])

  const showDevice = scope === "all"

  function drill(entry: LibraryEntry) {
    const [deviceId, ...rest] = entry.rel_path.split("/")
    setDeviceScope(deviceId)
    setSubpath(rest.join("/"))
  }

  function goUp() {
    const parts = subpath.split("/").filter(Boolean)
    if (parts.length <= 1) {
      setSubpath("")
    } else {
      setDeviceScope(parts[0])
      setSubpath(parts.slice(1, -1).join("/"))
    }
  }

  async function onAddFiles() {
    const selected = await open({ multiple: true, directory: false })
    if (!selected) return
    const paths = Array.isArray(selected) ? selected : [selected]
    if (paths.length > 0) setPendingPaths(paths)
  }

  async function onExport(entry: LibraryEntry) {
    const dir = await open({ directory: true })
    if (!dir) return
    setBusyRelPath(entry.rel_path)
    try {
      await exportMut({ relPath: entry.rel_path, targetDir: dir }).unwrap()
      toast.success(t("library.toast.exported"))
    } catch {
      toast.error(t("library.toast.failed"))
    } finally {
      setBusyRelPath(null)
    }
  }

  async function onDelete(entry: LibraryEntry) {
    setBusyRelPath(entry.rel_path)
    try {
      await deleteMut(entry.rel_path).unwrap()
      toast.success(t("library.toast.deleted"))
    } catch {
      toast.error(t("library.toast.failed"))
    } finally {
      setBusyRelPath(null)
    }
  }

  function startRename(entry: LibraryEntry) {
    setRenaming(entry.rel_path)
    setRenameVal(entry.name)
  }
  async function commitRename(entry: LibraryEntry) {
    const name = renameVal.trim()
    if (!name || name === entry.name) {
      setRenaming(null)
      return
    }
    setBusyRelPath(entry.rel_path)
    try {
      await renameMut({ relPath: entry.rel_path, newName: name }).unwrap()
      toast.success(t("library.toast.renamed"))
      setRenaming(null)
    } catch {
      toast.error(t("library.toast.failed"))
    } finally {
      setBusyRelPath(null)
    }
  }

  return (
    <div className="flex flex-1 flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        {!atRoot ? (
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  aria-label={t("library.up")}
                  onClick={goUp}
                />
              }
            >
              <ArrowUp />
            </TooltipTrigger>
            <TooltipContent>{t("library.up")}</TooltipContent>
          </Tooltip>
        ) : null}

        {atRoot ? (
          <Select
            value={deviceScope}
            onValueChange={(v) => setDeviceScope(v ?? ALL)}
          >
            <SelectTrigger
              className="border-border bg-card hover:bg-muted/60 h-8 w-40 rounded-md"
              aria-label={t("library.scope.all")}
            >
              <SelectValue className="min-w-0">
                {(value: string) =>
                  value === ALL
                    ? t("library.scope.all")
                    : (deviceOptions.find((o) => o.id === value)?.label ??
                      value)
                }
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t("library.scope.all")}</SelectItem>
              {deviceOptions.map((o) => (
                <SelectItem key={o.id} value={o.id}>
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}

        <div className="ml-auto" />
        <Button size="sm" onClick={onAddFiles}>
          <FilePlus />
          {t("library.add")}
        </Button>
      </div>

      {!atRoot ? (
        <div className="text-muted-foreground flex flex-wrap items-center gap-1 text-xs">
          {breadcrumb.map((c, i) => (
            <span key={c.key} className="flex items-center gap-1">
              {i > 0 ? <ChevronRight className="size-3" /> : null}
              <button
                type="button"
                className="hover:text-foreground"
                onClick={c.onClick}
              >
                {c.label}
              </button>
            </span>
          ))}
        </div>
      ) : null}

      <Card
        className={cn(
          "flex flex-1 flex-col transition-colors",
          dragging && "border-accent-brand bg-accent-tint",
        )}
      >
        <CardHeader>
          <CardTitle>{t("library.title")}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-1 flex-col overflow-auto">
          {isLoading ? (
            <div className="text-muted-foreground p-4 text-sm">
              {t("common.loading")}
            </div>
          ) : entries.length === 0 ? (
            <div className="flex flex-1 items-center justify-center">
              <EmptyState
                icon={Folder}
                title={t("library.empty.title")}
                description={t("library.empty.desc")}
              />
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("library.col.name")}</TableHead>
                  <TableHead className="w-24">
                    {t("library.col.kind")}
                  </TableHead>
                  <TableHead className="w-24">
                    {t("library.col.size")}
                  </TableHead>
                  <TableHead className="w-40">
                    {showDevice
                      ? t("library.col.device")
                      : t("library.col.modified")}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("library.col.actions")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {entries.map((e) => {
                  const Icon = kindIcon(e.name, e.kind === "dir")
                  const isRenaming = renaming === e.rel_path
                  const busy = busyRelPath === e.rel_path
                  const kindLabel =
                    e.kind === "dir"
                      ? t("library.kind.dir")
                      : e.name.split(".").pop()?.toUpperCase() ||
                        t("library.kind.file")
                  return (
                    <TableRow key={e.rel_path}>
                      <TableCell>
                        {isRenaming ? (
                          <div className="flex items-center gap-1">
                            <Input
                              value={renameVal}
                              onChange={(ev) => setRenameVal(ev.target.value)}
                              className="h-7 w-44"
                              onKeyDown={(ev) => {
                                if (ev.key === "Enter") commitRename(e)
                                if (ev.key === "Escape") setRenaming(null)
                              }}
                              autoFocus
                            />
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              disabled={busy}
                              onClick={() => commitRename(e)}
                            >
                              {busy ? (
                                <Loader2 className="animate-spin" />
                              ) : (
                                <Check />
                              )}
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              onClick={() => setRenaming(null)}
                            >
                              <X />
                            </Button>
                          </div>
                        ) : (
                          <button
                            type="button"
                            className="hover:text-accent-brand-strong flex items-center gap-2"
                            onClick={() =>
                              e.kind === "dir" ? drill(e) : setPreview(e)
                            }
                          >
                            <Icon className="size-4 shrink-0" />
                            <span>{e.name}</span>
                          </button>
                        )}
                      </TableCell>
                      <TableCell className="text-muted-foreground text-xs">
                        {kindLabel}
                      </TableCell>
                      <TableCell className="text-muted-foreground text-xs tabular-nums">
                        {formatSize(e.size)}
                      </TableCell>
                      <TableCell className="text-muted-foreground text-xs">
                        {showDevice ? (
                          e.is_self ? (
                            t("devices.thisDevice")
                          ) : (
                            e.device_name
                          )
                        ) : (
                          <span
                            title={dayjs(e.modified_ms).format("MM/DD HH:mm")}
                          >
                            {dayjs(e.modified_ms).fromNow()}
                          </span>
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex justify-end gap-1">
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t("library.row.export")}
                                  disabled={busy}
                                  onClick={() => onExport(e)}
                                />
                              }
                            >
                              {busy ? (
                                <Loader2 className="animate-spin" />
                              ) : (
                                <Download />
                              )}
                            </TooltipTrigger>
                            <TooltipContent>
                              {t("library.row.export")}
                            </TooltipContent>
                          </Tooltip>
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t("library.row.rename")}
                                  disabled={busy}
                                  onClick={() => startRename(e)}
                                />
                              }
                            >
                              <Pencil />
                            </TooltipTrigger>
                            <TooltipContent>
                              {t("library.row.rename")}
                            </TooltipContent>
                          </Tooltip>
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t("library.row.delete")}
                                  disabled={busy}
                                  onClick={() => onDelete(e)}
                                />
                              }
                            >
                              {busy ? (
                                <Loader2 className="animate-spin" />
                              ) : (
                                <Trash2 />
                              )}
                            </TooltipTrigger>
                            <TooltipContent>
                              {t("library.row.delete")}
                            </TooltipContent>
                          </Tooltip>
                        </div>
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {dragging ? (
        <div className="pointer-events-none fixed inset-0 z-30 flex items-center justify-center">
          <div className="border-accent-brand bg-accent-tint text-accent-brand-strong rounded-xl border-2 border-dashed px-8 py-6 text-sm font-medium">
            {t("library.drop.active")}
          </div>
        </div>
      ) : null}

      {pendingPaths ? (
        <UploadDialog
          paths={pendingPaths}
          subpath={subpath}
          onClose={() => setPendingPaths(null)}
        />
      ) : null}

      {preview ? (
        <PreviewSheet entry={preview} onClose={() => setPreview(null)} />
      ) : null}
    </div>
  )
}
