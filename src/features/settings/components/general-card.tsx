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
//
// Row-based layout (ADR-0011 v2): each preference is a SettingRow — label +
// hint on the left, control on the right, hairline between rows — so the card
// stays scannable as more options land. Trigger labels are derived from the
// value via a SelectValue render function; without it Base UI shows the raw
// value ("10" / "300" / "zh"), not the localized "10 秒" / "5 分钟" / "中文".

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
    <div className="flex flex-col">
      {prefsError ? (
        <p className="border-destructive/40 bg-destructive/5 text-destructive mb-2 rounded-md border p-2 text-xs leading-relaxed">
          {t("settings.general.readError", {
            detail: describeError(prefsError, t("common.unknownReason")),
          })}
        </p>
      ) : null}

      <SettingRow
        label={t("settings.general.language")}
        hint={t("settings.general.languageHint")}
      >
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
            <SelectValue placeholder="—">
              {(v: string) =>
                LANGUAGES.find((o) => o.code === v)?.nativeName ?? "—"
              }
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {LANGUAGES.map((o) => (
              <SelectItem key={o.code} value={o.code}>
                {o.nativeName}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingRow>

      <SettingRow
        label={t("settings.general.collectInterval")}
        hint={t("settings.general.collectIntervalHint")}
      >
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
            <SelectValue placeholder="—">
              {(v: string) => t("common.seconds", { n: Number(v) })}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {COLLECT_OPTIONS.map((v) => (
              <SelectItem key={v} value={String(v)}>
                {t("common.seconds", { n: v })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingRow>

      <SettingRow
        label={t("settings.general.pushInterval")}
        hint={t("settings.general.pushIntervalHint")}
      >
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
              <SelectValue placeholder="—">
                {(v: string) => t("common.minutes", { n: Number(v) / 60 })}
              </SelectValue>
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
          <span className="text-muted-foreground text-xs">
            {t("settings.general.pushNeedsSync")}
          </span>
        )}
      </SettingRow>

      <SettingRow
        label={t("settings.general.closeBehavior")}
        hint={t("settings.general.closeBehaviorHint")}
      >
        <div className="flex flex-wrap justify-end gap-2">
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
      </SettingRow>

      <SettingRow
        label={t("settings.general.versionUpdate")}
        hint={t("settings.general.updateHint")}
      >
        <UpdateControl />
      </SettingRow>
    </div>
  )
}

/** Row-based preference row (ADR-0011 v2): label + hint on the left, control
 *  on the right, hairline between rows. Content-only — rendered inside the
 *  通用 Section's CardContent, whose vertical padding comes from the Card
 *  (--card-spacing), so rows only carry their own inter-row spacing. */
function SettingRow({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div className="border-border flex items-start justify-between gap-4 border-t py-2.5 first:border-t-0 first:pt-0 last:pb-0">
      <div className="flex min-w-0 flex-col gap-1">
        <Label className="text-foreground">{label}</Label>
        {hint ? (
          <p className="text-muted-foreground text-xs leading-relaxed">
            {hint}
          </p>
        ) : null}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  )
}
