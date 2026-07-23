// General preferences (ADR-0012 / 0014 / 0016 / 0017): display language,
// background-collect interval, push-to-sync interval, window-close behavior,
// and version/update. Content-only — rendered inside SettingsView's 通用
// section card, so no Card wrapper of its own.
//
// Collect and push are DECOUPLED (ADR-0014): collect is a short seconds-level
// local cadence, push is a longer minutes-level Git cadence (Synced only).
// Language (ADR-0016) is the one preference Rust must know at cold start (to
// build the localized tray), so it lives here alongside the others. All
// discrete presets (Select, instant-effect) — no save button.

import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { UpdateControl } from "@/app/shell/update-card"
import {
  useAppInfoQuery,
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
  useSetLanguageMutation,
  useSetPushIntervalMutation,
} from "@/app/store/api"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { LANGUAGES } from "@/i18n/languages"
import type { CloseBehavior, Language } from "@/types/preferences"

/** Pull a human-readable reason out of an RTK Query mutation error. */
function describeError(e: unknown, fallback: string): string {
  if (e && typeof e === "object") {
    const m = e as Record<string, unknown>
    if (typeof m.message === "string") return m.message
    if (typeof m.data === "string") return m.data
    if (typeof m.error === "string") return m.error
  }
  return fallback
}

/** Close-behavior option → i18n key. */
const CLOSE_OPTIONS: ReadonlyArray<[CloseBehavior, string]> = [
  ["ask", "settings.general.closeAsk"],
  ["minimize", "closeDialog.minimizeToTray"],
  ["quit", "common.quit"],
]

/** Collect presets (ADR-0014): seconds-level, local-only. */
const COLLECT_OPTIONS: ReadonlyArray<number> = [10, 30, 60]

/** Push presets (ADR-0014): minutes-level, Git, Synced only. */
const PUSH_OPTIONS: ReadonlyArray<number> = [300, 600, 900, 1800, 3600]

export function GeneralCard() {
  const { t } = useTranslation()
  const { data: prefs, error: prefsError } = usePreferencesQuery()
  const { data: info } = useAppInfoQuery()
  const synced = info?.mode === "synced"
  const [setLanguage, { isLoading: savingLang }] = useSetLanguageMutation()
  const [setCloseBehavior, { isLoading: savingClose }] =
    useSetCloseBehaviorMutation()
  const [setCollectInterval, { isLoading: savingCollect }] =
    useSetCollectIntervalMutation()
  const [setPushInterval, { isLoading: savingPush }] =
    useSetPushIntervalMutation()

  return (
    <div className="flex flex-col gap-5">
      {prefsError ? (
        <p className="border-destructive/40 bg-destructive/5 text-destructive rounded-md border p-2 text-xs leading-relaxed">
          {t("settings.general.readError", {
            detail: describeError(prefsError, t("common.unknownReason")),
          })}
        </p>
      ) : null}

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">
          {t("settings.general.language")}
        </Label>
        <Select
          value={prefs?.language}
          onValueChange={async (v) => {
            const r = await setLanguage(v as Language)
            if ("error" in r)
              toast.error(t("settings.toast.saveFailed"), {
                description: describeError(r.error, t("common.unknownReason")),
              })
          }}
        >
          <SelectTrigger className="w-36" disabled={savingLang}>
            <SelectValue placeholder="—" />
          </SelectTrigger>
          <SelectContent>
            {LANGUAGES.map((o) => (
              <SelectItem key={o.code} value={o.code}>
                {o.nativeName}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-muted-foreground text-xs">
          {t("settings.general.languageHint")}
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">
          {t("settings.general.collectInterval")}
        </Label>
        <Select
          value={prefs ? String(prefs.collect_interval_secs) : undefined}
          onValueChange={async (v) => {
            const r = await setCollectInterval(Number(v))
            if ("error" in r)
              toast.error(t("settings.toast.saveFailed"), {
                description: describeError(r.error, t("common.unknownReason")),
              })
          }}
        >
          <SelectTrigger className="w-36" disabled={savingCollect}>
            <SelectValue placeholder="—" />
          </SelectTrigger>
          <SelectContent>
            {COLLECT_OPTIONS.map((v) => (
              <SelectItem key={v} value={String(v)}>
                {t("common.seconds", { n: v })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-muted-foreground text-xs">
          {t("settings.general.collectIntervalHint")}
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">
          {t("settings.general.pushInterval")}
        </Label>
        {synced ? (
          <Select
            value={prefs ? String(prefs.push_interval_secs) : undefined}
            onValueChange={async (v) => {
              const r = await setPushInterval(Number(v))
              if ("error" in r)
                toast.error(t("settings.toast.saveFailed"), {
                  description: describeError(
                    r.error,
                    t("common.unknownReason"),
                  ),
                })
            }}
          >
            <SelectTrigger className="w-36" disabled={savingPush}>
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              {PUSH_OPTIONS.map((v) => (
                <SelectItem key={v} value={String(v)}>
                  {t("common.minutes", { n: v / 60 })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <p className="text-muted-foreground text-xs">
            {t("settings.general.pushNeedsSync")}
          </p>
        )}
        <p className="text-muted-foreground text-xs">
          {t("settings.general.pushIntervalHint")}
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">
          {t("settings.general.closeBehavior")}
        </Label>
        <div className="flex flex-wrap gap-2">
          {CLOSE_OPTIONS.map(([value, key]) => (
            <Button
              key={value}
              size="sm"
              variant={prefs?.close_behavior === value ? "default" : "outline"}
              disabled={savingClose}
              onClick={async () => {
                const r = await setCloseBehavior(value)
                if ("error" in r)
                  toast.error(t("settings.toast.saveFailed"), {
                    description: describeError(
                      r.error,
                      t("common.unknownReason"),
                    ),
                  })
              }}
            >
              {t(key)}
            </Button>
          ))}
        </div>
        <p className="text-muted-foreground text-xs">
          {t("settings.general.closeBehaviorHint")}
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">
          {t("settings.general.versionUpdate")}
        </Label>
        <UpdateControl />
        <p className="text-muted-foreground text-xs">
          {t("settings.general.updateHint")}
        </p>
      </div>
    </div>
  )
}
