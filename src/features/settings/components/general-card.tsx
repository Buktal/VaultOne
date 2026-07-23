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

import { Check } from "lucide-react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { UpdateControl } from "@/app/shell/update-card"
import {
  useAppInfoQuery,
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
  useSetLanguageMutation,
  useSetLightweightExpandMutation,
  useSetPushIntervalMutation,
  useSetSkinMutation,
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
import { cn } from "@/lib/utils"
import type {
  CloseBehavior,
  Language,
  LightweightExpand,
  Skin,
} from "@/types/preferences"

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

/** Lightweight half-icon expand trigger (ADR-0015): click (default) or hover. */
const EXPAND_OPTIONS: ReadonlyArray<[LightweightExpand, string]> = [
  ["click", "settings.general.lightweightExpandClick"],
  ["hover", "settings.general.lightweightExpandHover"],
]

/** Collect presets (ADR-0014): seconds-level, local-only. */
const COLLECT_OPTIONS: ReadonlyArray<number> = [10, 30, 60]

/** Push presets (ADR-0014): minutes-level, Git, Synced only. */
const PUSH_OPTIONS: ReadonlyArray<number> = [300, 600, 900, 1800, 3600]

/**
 * Color skins (multi-skin theming). Chromatic swatches read straight from CSS
 * — each carries `data-skin={value}` and uses `var(--brand)`, so the swatch IS
 * the live accent from index.css (single source: edit a [data-skin] block and
 * the swatch follows, no TS sync). `neutral` is the exception: its grey is the
 * :root/.dark default with NO [data-skin] block, so var(--brand) would inherit
 * the active skin's brand on <html> — it uses a literal `brand` fill instead.
 * The selection check follows the MODE, not the skin: black in light, white in
 * dark, with a dark drop-shadow so it reads on any swatch fill. Names are
 * English literals (no i18n); `neutral` first as the default.
 */
const SKINS: ReadonlyArray<{
  value: Skin
  label: string
  brand?: string
}> = [
  {
    value: "neutral",
    label: "Neutral",
    brand: "#6b7280",
  },
  { value: "sage", label: "Sage" },
  { value: "azure", label: "Azure" },
  { value: "crimson", label: "Crimson" },
  { value: "mauve", label: "Mauve" },
]

export function GeneralCard() {
  const { t } = useTranslation()
  const { data: prefs, error: prefsError } = usePreferencesQuery()
  const { data: info } = useAppInfoQuery()
  const synced = info?.mode === "synced"
  const [setLanguage, { isLoading: savingLang }] = useSetLanguageMutation()
  const [setLightweightExpand, { isLoading: savingExpand }] =
    useSetLightweightExpandMutation()
  const [setCloseBehavior, { isLoading: savingClose }] =
    useSetCloseBehaviorMutation()
  const [setCollectInterval, { isLoading: savingCollect }] =
    useSetCollectIntervalMutation()
  const [setPushInterval, { isLoading: savingPush }] =
    useSetPushIntervalMutation()
  const [setSkin, { isLoading: savingSkin }] = useSetSkinMutation()

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
        label={t("settings.general.skin")}
        hint={t("settings.general.skinHint")}
      >
        <div className="flex gap-1.5">
          {SKINS.map((s) => {
            const selected = prefs?.skin === s.value
            return (
              <button
                key={s.value}
                type="button"
                title={s.label}
                aria-label={s.label}
                aria-pressed={selected}
                data-skin={s.brand ? undefined : s.value}
                disabled={savingSkin}
                onClick={async () => {
                  const r = await setSkin(s.value)
                  if ("error" in r)
                    toast.error(t("settings.toast.saveFailed"), {
                      description: describeError(
                        r.error,
                        t("common.unknownReason"),
                      ),
                    })
                }}
                className={cn(
                  "flex h-8 w-8 items-center justify-center rounded-md border-2 transition",
                  selected
                    ? "border-foreground"
                    : "border-transparent hover:border-border",
                )}
                style={{ background: s.brand ?? "var(--brand)" }}
              >
                {selected ? (
                  <Check
                    className="size-3.5 text-black dark:text-white"
                    style={{
                      filter: "drop-shadow(0 0 1px rgba(0, 0, 0, 0.55))",
                    }}
                  />
                ) : null}
              </button>
            )
          })}
        </div>
      </SettingRow>
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
        label={t("settings.general.lightweightExpand")}
        hint={t("settings.general.lightweightExpandHint")}
      >
        <div className="flex flex-wrap justify-end gap-2">
          {EXPAND_OPTIONS.map(([value, key]) => (
            <Button
              key={value}
              size="sm"
              variant={
                prefs?.lightweight_expand === value ? "default" : "outline"
              }
              disabled={savingExpand}
              onClick={async () => {
                const r = await setLightweightExpand(value)
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
