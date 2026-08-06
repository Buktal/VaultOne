// Pure derivations for the providers list & form: reading the basic form
// fields (endpoint / API key / model) out of a provider's `settingsConfig`
// snapshot, and rebuilding that snapshot from the form while preserving every
// other field. Also the auth-field toggle (moving the key between the two
// spellings), the `${VAR}` template-variable substitution for preset
// snapshots, and the app-side `meta` record that makes re-editing work.
// Extracted from the view/hook so each rule is testable in isolation — the
// single authority for how the form maps onto the settings.json text.

import type { ProviderPreset } from "@/features/providers/presets"
import type { Provider } from "@/types/generated/bindings"

// The env keys the basic form knows about. The auth key has two spellings
// Claude Code accepts — AUTH_TOKEN (the default the form writes) and API_KEY
// (the legacy spelling some providers require; the form reads either, so a
// provider configured with API_KEY edits cleanly).
const ENV_BASE_URL = "ANTHROPIC_BASE_URL"
const ENV_AUTH_TOKEN = "ANTHROPIC_AUTH_TOKEN"
const ENV_API_KEY = "ANTHROPIC_API_KEY"
const ENV_MODEL = "ANTHROPIC_MODEL"

type SettingsConfig = { env?: Record<string, string> }

/** Parse a provider's settingsConfig JSON text; garbage or empty → `{}` so a
 *  corrupt snapshot never throws the form open. A non-object `env` (a string,
 *  an array — anything a hand-edited snapshot could hold) is dropped to `{}`
 *  so the write-back never spreads e.g. a string into character-index keys. */
export function parseSettingsConfig(config: string): SettingsConfig {
  if (!config) return {}
  try {
    const parsed: unknown = JSON.parse(config)
    if (typeof parsed !== "object" || parsed === null) return {}
    const cfg = parsed as SettingsConfig
    if (
      cfg.env !== undefined &&
      (typeof cfg.env !== "object" ||
        cfg.env === null ||
        Array.isArray(cfg.env))
    ) {
      return { ...cfg, env: {} }
    }
    return cfg
  } catch {
    return {}
  }
}

function envValue(configText: string, key: string): string {
  return parseSettingsConfig(configText).env?.[key] ?? ""
}

/** The provider's base URL (endpoint), from `env.ANTHROPIC_BASE_URL`. */
export function providerEndpoint(provider: Provider): string {
  return envValue(provider.settingsConfig, ENV_BASE_URL)
}

/** The API key — reads AUTH_TOKEN first, then API_KEY (the form writes the
 *  former by default; the latter is the legacy spelling some providers use). */
export function providerApiKey(provider: Provider): string {
  return (
    envValue(provider.settingsConfig, ENV_AUTH_TOKEN) ||
    envValue(provider.settingsConfig, ENV_API_KEY)
  )
}

/** Text-level twin of `providerEndpoint` — reads the endpoint straight from a
 *  settingsConfig JSON text (the JSON editor's working value). */
export function configEndpoint(configText: string): string {
  return envValue(configText, ENV_BASE_URL)
}

/** Text-level twin of `providerApiKey` — reads the API key straight from a
 *  settingsConfig JSON text, AUTH_TOKEN first then the legacy API_KEY. */
export function configApiKey(configText: string): string {
  return (
    envValue(configText, ENV_AUTH_TOKEN) || envValue(configText, ENV_API_KEY)
  )
}

/** The primary model, from `env.ANTHROPIC_MODEL`. Display-only for now — the
 *  five-role model mapping is a later ticket. */
export function providerModel(provider: Provider): string {
  return envValue(provider.settingsConfig, ENV_MODEL)
}

/**
 * Merge the basic form fields (endpoint / API key) into a settingsConfig JSON
 * text, keeping every field the form does not own (extra env keys, non-env
 * settings) untouched — the text-level twin of `withBasicFields` that the form
 * sheet uses to keep the JSON editor in sync while typing. Callers must only
 * pass config that parses to an object (`parseJsonObject`), else a garbage
 * snapshot would be replaced by a bare `{"env": …}` and the in-progress edit
 * lost. A non-empty key is written under the selected auth field
 * (`fields.authField`, AUTH_TOKEN by default) and the other spelling dropped;
 * an empty endpoint / key removes the stale env entry.
 */
export function withBasicFieldsInText(
  configText: string,
  fields: { endpoint: string; apiKey: string; authField?: AuthField },
): string {
  const config = parseSettingsConfig(configText)
  const env = { ...(config.env ?? {}) }
  if (fields.endpoint) env[ENV_BASE_URL] = fields.endpoint
  else delete env[ENV_BASE_URL]
  if (fields.apiKey) {
    const target = authFieldKey(fields.authField ?? "auth_token")
    env[target] = fields.apiKey
    delete env[target === ENV_AUTH_TOKEN ? ENV_API_KEY : ENV_AUTH_TOKEN]
  } else {
    delete env[ENV_AUTH_TOKEN]
    delete env[ENV_API_KEY]
  }
  return JSON.stringify({ ...config, env }, null, 2)
}

/**
 * Rebuild a provider's settingsConfig from the basic form fields, keeping
 * every field the form does not own (extra env keys, non-env settings)
 * untouched — an endpoint / key edit must never drop the rest of a snapshot.
 * A non-empty key is written under the selected auth field
 * (`fields.authField`, AUTH_TOKEN by default) and the other spelling dropped,
 * so a provider that only carried API_KEY migrates to one credential instead
 * of leaving both in the snapshot — and a provider toggled to API_KEY stays
 * on that spelling. An empty endpoint / key removes the stale env entry, so
 * clearing a field in the form clears it in the snapshot too (both key
 * spellings on clear).
 */
export function withBasicFields(
  provider: Provider,
  fields: { endpoint: string; apiKey: string; authField?: AuthField },
): Provider {
  return {
    ...provider,
    settingsConfig: withBasicFieldsInText(provider.settingsConfig, fields),
  }
}

/** A blank provider for the "new provider" sheet (custom category, empty env).
 *  `id` is empty so `save_provider_cmd` allocates a fresh one. */
export function emptyProvider(): Provider {
  return {
    id: "",
    name: "",
    websiteUrl: "",
    category: "custom",
    icon: "",
    iconColor: "",
    sortIndex: 0,
    notes: "",
    settingsConfig: '{\n  "env": {}\n}',
    meta: "{}",
    updatedAt: "",
  }
}

/** Build the "new provider" draft from a built-in preset: category goes to
 *  `custom` (a preset is the starting point, customization is the end — the
 *  saved row is a user provider, not a preset), `id` stays empty so
 *  `save_provider_cmd` allocates a fresh one. The preset's settingsConfig
 *  snapshot is copied verbatim (its `${VAR}` placeholders stay until the
 *  template-variable step); the preset constant itself is never mutated. */
export function providerFromPreset(preset: ProviderPreset): Provider {
  return {
    id: "",
    name: preset.name,
    websiteUrl: preset.websiteUrl,
    category: "custom",
    icon: preset.icon,
    iconColor: preset.iconColor,
    sortIndex: 0,
    notes: preset.notes ?? "",
    settingsConfig: preset.settingsConfig,
    meta: "{}",
    updatedAt: "",
  }
}

// ── Auth field toggle ──────────────────────────────────────────────────────
//
// The form can write the API key under either of the two env spellings Claude
// Code accepts. `switchAuthField` moves the value between them so toggling
// never loses or duplicates the credential; `configAuthField` derives the
// current field from the snapshot (the JSON editor stays the source of
// truth).

/** The two env keys the auth-field toggle can write to. AUTH_TOKEN is the
 *  default (what Claude Code documents); API_KEY is the legacy spelling some
 *  providers require. */
export type AuthField = "auth_token" | "api_key"

/** The env key a field value maps to. */
export function authFieldKey(field: AuthField): string {
  return field === "auth_token" ? ENV_AUTH_TOKEN : ENV_API_KEY
}

/** Which auth field the snapshot currently uses. API_KEY only when that is
 *  the sole spelling present — with both, or neither, the default AUTH_TOKEN
 *  wins, mirroring the read preference of `providerApiKey`. */
export function configAuthField(configText: string): AuthField {
  const env = parseSettingsConfig(configText).env ?? {}
  return env[ENV_AUTH_TOKEN] === undefined && env[ENV_API_KEY] !== undefined
    ? "api_key"
    : "auth_token"
}

/** Move the API key value from one auth field to the other and delete the old
 *  key, so the toggle never loses or duplicates the credential. The rest of
 *  the snapshot is untouched; a missing value just removes the old key. A
 *  no-op when `from === to`. */
export function switchAuthField(
  configText: string,
  from: AuthField,
  to: AuthField,
): string {
  if (from === to) return configText
  const config = parseSettingsConfig(configText)
  const env = { ...(config.env ?? {}) }
  const value = env[authFieldKey(from)]
  delete env[authFieldKey(from)]
  if (value !== undefined) env[authFieldKey(to)] = value
  return JSON.stringify({ ...config, env }, null, 2)
}

// ── Template variables ─────────────────────────────────────────────────────
//
// Some presets (Bedrock) carry `${VAR}` placeholders in their snapshot text.
// The form shows one input per variable (`extractTemplateVars`) and substitutes
// the values (`replaceTemplateVarsInText`). The values are also recorded in
// the provider's meta (`templateValues`) so re-editing a materialized snapshot
// can restore the placeholders (`restoreTemplatePlaceholders`) and pre-fill
// the inputs.

/** `${VAR}` placeholder pattern — letters, digits and underscores. */
const TEMPLATE_VAR_RE = /\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g

/** Recursive walk over an arbitrary JSON structure: every string value passes
 *  through the transform. One traversal defines where placeholders live, so
 *  the extract / replace / restore helpers cannot drift apart. */
function walkStrings(
  value: unknown,
  transform: (s: string) => string,
): unknown {
  if (typeof value === "string") return transform(value)
  if (Array.isArray(value)) return value.map((v) => walkStrings(v, transform))
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([k, v]) => [
        k,
        walkStrings(v, transform),
      ]),
    )
  }
  return value
}

/** Collect the `${VAR}` placeholder names in any string value of a
 *  settingsConfig snapshot, deduped, in order of first appearance. The form
 *  shows one template-variable input per name; an empty list hides the
 *  section. */
export function extractTemplateVars(configText: string): string[] {
  const names: string[] = []
  walkStrings(parseSettingsConfig(configText), (s) => {
    for (const match of s.matchAll(TEMPLATE_VAR_RE)) {
      const name = match[1]!
      if (!names.includes(name)) names.push(name)
    }
    return s
  })
  return names
}

/** Replace every `${VAR}` placeholder in any string value of the snapshot with
 *  its value. A variable with no value — missing or empty — keeps its
 *  placeholder verbatim: the user simply did not fill it, and an empty string
 *  in an env key would silently corrupt the config. Callers check the result
 *  with `extractTemplateVars` before persisting. */
export function replaceTemplateVarsInText(
  configText: string,
  values: Record<string, string>,
): string {
  const config = parseSettingsConfig(configText)
  const next = walkStrings(config, (s) =>
    s.replace(TEMPLATE_VAR_RE, (match, name: string) => {
      const value = values[name]
      return value === undefined || value === "" ? match : value
    }),
  )
  return JSON.stringify(next, null, 2)
}

/** Restore the placeholders a previous save substituted, so re-editing a
 *  materialized snapshot behaves like the preset flow. Every occurrence of a
 *  recorded value reverts to its placeholder — a recorded value came from a
 *  placeholder by definition, and the values are distinctive strings (region
 *  codes, access keys), so this is safe in practice. Longer values are
 *  reverted first so a value that is a substring of another cannot be
 *  mangled. Strings without a recorded template are untouched. */
export function restoreTemplatePlaceholders(
  configText: string,
  values: Record<string, string>,
): string {
  const config = parseSettingsConfig(configText)
  const entries = Object.entries(values)
    .filter((entry) => entry[1] !== "")
    .sort((a, b) => b[1].length - a[1].length)
  const next = walkStrings(config, (s) => {
    let out = s
    for (const [name, value] of entries) {
      // split/join 代替 replaceAll：ES2020 目标库不支持 replaceAll，且值可能含正则元字符
      out = out.split(value).join(`\${${name}}`)
    }
    return out
  })
  return JSON.stringify(next, null, 2)
}

// ── Provider meta ──────────────────────────────────────────────────────────
//
// `meta` is app-side JSON that never reaches the live settings file. It
// records the template-variable values so the sheet can pre-fill the inputs
// and restore placeholders when re-editing.

/** App-side provider metadata — the only consumer today is the template
 *  variable record. */
type ProviderMeta = { templateValues?: Record<string, string> }

/** Parse a provider's meta JSON text; garbage or empty → `{}` so a corrupt
 *  meta never throws the sheet open. A non-object `templateValues` is dropped
 *  to `{}`. */
export function parseMeta(metaText: string): ProviderMeta {
  if (!metaText) return {}
  try {
    const parsed: unknown = JSON.parse(metaText)
    if (typeof parsed !== "object" || parsed === null) return {}
    const meta = parsed as ProviderMeta
    if (
      meta.templateValues !== undefined &&
      (typeof meta.templateValues !== "object" ||
        meta.templateValues === null ||
        Array.isArray(meta.templateValues))
    ) {
      return { ...meta, templateValues: {} }
    }
    return meta
  } catch {
    return {}
  }
}

/** The template-variable values recorded in the meta (string entries only —
 *  a hand-edited meta could hold garbage). */
export function metaTemplateValues(metaText: string): Record<string, string> {
  const values = parseMeta(metaText).templateValues
  if (!values) return {}
  return Object.fromEntries(
    Object.entries(values).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  )
}

/** Record the current template-variable values in the meta text, replacing
 *  the previous record and keeping unknown meta keys. Empty values are
 *  dropped and an empty map removes the key, so a provider that no longer
 *  uses placeholders stays clean. */
export function withMetaTemplateValues(
  metaText: string,
  values: Record<string, string>,
): string {
  const meta = parseMeta(metaText)
  const filled = Object.fromEntries(
    Object.entries(values).filter((entry) => entry[1] !== ""),
  )
  if (Object.keys(filled).length === 0) delete meta.templateValues
  else meta.templateValues = filled
  return JSON.stringify(meta, null, 2)
}
