// Provider editor as a Sheet (side panel) — new and edit both flow through
// here. The basic form owns three fields (name / endpoint / API key); on save
// it maps them onto the provider's settingsConfig snapshot via the derive.ts
// helpers (withBasicFields preserves every field the form doesn't own), then
// calls the upsert mutation and closes.
//
// The settings.json editor at the bottom makes the JSON snapshot the single
// source of truth: editing the JSON re-derives the endpoint / API key fields
// (they are a quick view of `env`), and editing a field merges it back into
// the JSON. The merge is guarded by `parseJsonObject` — while the JSON text
// does not parse to an object, field edits only update the field state and the
// in-progress JSON edit is never clobbered; once it parses again the fields
// re-derive. Save still goes through withBasicFields, so the field values win
// for the two keys they own and everything else survives verbatim.

import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { useSaveProviderMutation } from "@/app/store/api"
import { JsonEditor } from "@/components/json-editor"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Sheet,
  SheetContent,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import {
  configApiKey,
  configEndpoint,
  emptyProvider,
  providerApiKey,
  providerEndpoint,
  providerFromPreset,
  withBasicFields,
  withBasicFieldsInText,
} from "@/features/providers/derive"
import type { ProviderPreset } from "@/features/providers/presets"
import { useMutateWithToast } from "@/hooks/use-toast-mutation"
import { parseJsonObject } from "@/lib/json"

import type { Provider } from "@/types/generated/bindings"

export function ProviderFormSheet({
  open,
  onOpenChange,
  editing,
  preset,
  onSaved,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The provider being edited, or null for a new one. */
  editing: Provider | null
  /** When set, the sheet opens pre-filled from this built-in preset. The preset
   *  constant is never mutated — providerFromPreset copies its settingsConfig
   *  snapshot into a fresh custom-category draft. */
  preset: ProviderPreset | null
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const base =
    editing ?? (preset ? providerFromPreset(preset) : emptyProvider())
  const [name, setName] = useState(base.name)
  const [configText, setConfigText] = useState(base.settingsConfig)
  const [endpoint, setEndpoint] = useState(providerEndpoint(base))
  const [apiKey, setApiKey] = useState(providerApiKey(base))
  const [save, { isLoading: saving }] = useSaveProviderMutation()
  const runWithToast = useMutateWithToast()

  useEffect(() => {
    if (!open) return
    const b = editing ?? (preset ? providerFromPreset(preset) : emptyProvider())
    setName(b.name)
    setConfigText(b.settingsConfig)
    setEndpoint(providerEndpoint(b))
    setApiKey(providerApiKey(b))
  }, [editing, preset, open])

  // JSON → form fields: whenever the snapshot text changes (editor edit or a
  // field merge), the env-backed fields mirror it. A text that doesn't parse
  // to an object keeps the fields as-is — the linter flags the red line, and
  // clobbering the fields with an empty derive would hide a broken edit.
  useEffect(() => {
    const result = parseJsonObject(configText)
    if (!result.ok) return
    setEndpoint(configEndpoint(configText))
    setApiKey(configApiKey(configText))
  }, [configText])

  function onEndpointChange(value: string) {
    setEndpoint(value)
    if (parseJsonObject(configText).ok) {
      setConfigText((prev) =>
        withBasicFieldsInText(prev, { endpoint: value, apiKey }),
      )
    }
  }

  function onApiKeyChange(value: string) {
    setApiKey(value)
    if (parseJsonObject(configText).ok) {
      setConfigText((prev) =>
        withBasicFieldsInText(prev, { endpoint, apiKey: value }),
      )
    }
  }

  async function onSave() {
    if (!name.trim()) {
      toast.error(t("providers.toast.nameRequired"))
      return
    }
    const snapshot = parseJsonObject(configText)
    if (!snapshot.ok) {
      toast.error(t("providers.toast.invalidConfig"), {
        description: snapshot.error,
      })
      return
    }
    // Rebuild the snapshot from the working JSON text (preserving everything
    // the form doesn't own), then attach the edited name and ship the upsert.
    // The endpoint is trimmed here, not on every keystroke, so typing an
    // in-progress value (trailing spaces mid-edit) isn't fought by the input.
    const next = withBasicFields(
      { ...base, settingsConfig: configText },
      { endpoint: endpoint.trim(), apiKey },
    )
    const ok = await runWithToast(
      save,
      { ...next, name: name.trim() },
      {
        success: { key: "providers.toast.saved", vars: { name: name.trim() } },
        failed: { key: "providers.toast.saveFailed" },
      },
    )
    if (ok) onSaved()
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="max-w-lg">
        <SheetHeader>
          <SheetTitle>
            {editing
              ? t("providers.form.editTitle")
              : preset
                ? t("providers.form.presetTitle")
                : t("providers.form.newTitle")}
          </SheetTitle>
          {preset ? (
            <p className="text-muted-foreground text-xs">
              {t("providers.form.presetHint", { name: preset.name })}
            </p>
          ) : null}
        </SheetHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-0.5">
          <Field label={t("providers.form.name")}>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("providers.form.namePlaceholder")}
            />
          </Field>
          <Field label={t("providers.form.endpoint")}>
            <Input
              value={endpoint}
              onChange={(e) => onEndpointChange(e.target.value)}
              placeholder="https://api.example.com"
              spellCheck={false}
            />
          </Field>
          <Field label={t("providers.form.apiKey")}>
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => onApiKeyChange(e.target.value)}
              placeholder={t("providers.form.apiKeyPlaceholder")}
              spellCheck={false}
            />
          </Field>
          <Field label={t("providers.form.settingsJson")}>
            <JsonEditor
              value={configText}
              onChange={setConfigText}
              placeholder={t("providers.form.settingsJsonPlaceholder")}
              className="h-72"
            />
          </Field>
        </div>

        <SheetFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button disabled={saving} onClick={onSave}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  )
}

function Field({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label className="text-muted-foreground text-xs">{label}</Label>
      {children}
    </div>
  )
}
