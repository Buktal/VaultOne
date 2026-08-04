// Group create dialog — used by both tracks. Local = SQLite write (near-
// instant); Synced = git push (ADR 0002: optimistic + loading). The dialog owns
// only its transient text input; the create mutation + loading flag live in the
// hook, so the dialog stays a pure render of an open/close + name + onConfirm.

import { Loader2 } from "lucide-react"
import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import type { GroupTrack } from "../derive"

export function GroupCreateDialog({
  open,
  onClose,
  onCreate,
  creating,
  track,
}: {
  open: boolean
  onClose: () => void
  onCreate: (name: string) => Promise<boolean>
  creating: boolean
  track: GroupTrack
}) {
  const { t } = useTranslation()
  const [name, setName] = useState("")

  // Reset the input each time the dialog opens so a previous draft doesn't linger.
  useEffect(() => {
    if (open) setName("")
  }, [open])

  async function submit() {
    const ok = await onCreate(name)
    if (ok) setName("")
  }

  const canSubmit = name.trim().length > 0 && !creating

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t(
              track === "local"
                ? "sessions.group.createLocal"
                : "sessions.group.createSynced",
            )}
          </DialogTitle>
        </DialogHeader>
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t("sessions.group.namePlaceholder")}
          autoFocus
          onKeyDown={(e) => {
            if (e.key === "Enter" && canSubmit) void submit()
          }}
        />
        <DialogFooter>
          <Button variant="ghost" onClick={onClose} disabled={creating}>
            {t("common.cancel")}
          </Button>
          <Button onClick={submit} disabled={!canSubmit}>
            {creating ? <Loader2 className="animate-spin" /> : null}
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
