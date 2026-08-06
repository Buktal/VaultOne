// Pure derivations for the providers list & form: reading the basic form
// fields (endpoint / API key) and the five-role model mapping (Sonnet / Opus /
// Haiku / Fable / Subagent models, display names, 1M marker) out of a
// provider's `settingsConfig` snapshot, and rebuilding that snapshot from the
// form while preserving every other field. Extracted from the view/hook so
// each rule is testable in isolation — the single authority for how the form
// maps onto the settings.json text.

import type { ProviderPreset } from "@/features/providers/presets"
import type { Provider } from "@/types/generated/bindings"

// The env keys the basic form knows about. The endpoint and auth key are the
// two spellings Claude Code accepts (the form writes AUTH_TOKEN but reads
// either, so a provider configured with API_KEY edits cleanly).
const ENV_BASE_URL = "ANTHROPIC_BASE_URL"
const ENV_AUTH_TOKEN = "ANTHROPIC_AUTH_TOKEN"
const ENV_API_KEY = "ANTHROPIC_API_KEY"
const ENV_MODEL = "ANTHROPIC_MODEL"

/** The legacy small/fast model key — Haiku's backfill source. Model writes
 *  delete it, so a snapshot never keeps both the new role keys and the old
 *  spelling that preceded them. */
const ENV_SMALL_FAST_MODEL = "ANTHROPIC_SMALL_FAST_MODEL"

/** The `[1M]` suffix declaring the 1M-context capability — a marker Claude
 *  Code reads natively off the model name in env (e.g. "claude-opus-5[1M]"),
 *  appended when the form's 1M box is checked. */
const ONE_M_MARKER = "[1M]"

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
 *  former; the latter is a legacy spelling some providers use). */
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

/** The primary model, from `env.ANTHROPIC_MODEL`. The basic form does not own
 *  it — it is the fallback the role mapping reads when a role key is missing. */
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
 * lost. A non-empty key is written as AUTH_TOKEN and the legacy API_KEY
 * spelling dropped; an empty endpoint / key removes the stale env entry.
 */
export function withBasicFieldsInText(
  configText: string,
  fields: { endpoint: string; apiKey: string },
): string {
  const config = parseSettingsConfig(configText)
  const env = { ...(config.env ?? {}) }
  if (fields.endpoint) env[ENV_BASE_URL] = fields.endpoint
  else delete env[ENV_BASE_URL]
  if (fields.apiKey) {
    env[ENV_AUTH_TOKEN] = fields.apiKey
    delete env[ENV_API_KEY]
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
 * A non-empty key is written as AUTH_TOKEN and the legacy API_KEY spelling is
 * dropped, so editing a provider that only carried API_KEY migrates to one
 * credential instead of leaving both in the snapshot. An empty endpoint / key
 * removes the stale env entry, so clearing a field in the form clears it in
 * the snapshot too (both key spellings on clear).
 */
export function withBasicFields(
  provider: Provider,
  fields: { endpoint: string; apiKey: string },
): Provider {
  return {
    ...provider,
    settingsConfig: withBasicFieldsInText(provider.settingsConfig, fields),
  }
}

// ------------------------------------------------------- model roles + 1M --

/** The five model roles Claude Code routes to. Each role carries its own
 *  request model, an optional display name for the model picker, and — except
 *  Haiku — a 1M-capability checkbox. */
export type ModelRoleId = "sonnet" | "opus" | "haiku" | "fable" | "subagent"

/** A role's env mapping: where its model and display name live, what to fall
 *  back to when the model key is missing, and whether it may declare the 1M
 *  context marker (Haiku cannot — it is stripped on write). */
export interface ModelRole {
  id: ModelRoleId
  modelKey: string
  nameKey: string
  /** Env keys tried in order when `modelKey` is missing — mirrors the runtime
   *  mapping chain so the form never shows a hole a configured provider fills. */
  backfillKeys: string[]
  supportsOneM: boolean
}

/** The five roles, in form-display order — single source of truth for the env
 *  key mapping: the form iterates this table and the helpers look roles up in
 *  it. Backfill chains: Haiku falls back to the legacy small-fast key, then
 *  the primary model; Fable through Opus's key, Subagent through Sonnet's, the
 *  rest to the primary model. */
export const MODEL_ROLES: ModelRole[] = [
  {
    id: "sonnet",
    modelKey: "ANTHROPIC_DEFAULT_SONNET_MODEL",
    nameKey: "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    backfillKeys: [ENV_MODEL],
    supportsOneM: true,
  },
  {
    id: "opus",
    modelKey: "ANTHROPIC_DEFAULT_OPUS_MODEL",
    nameKey: "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    backfillKeys: [ENV_MODEL],
    supportsOneM: true,
  },
  {
    id: "haiku",
    modelKey: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    nameKey: "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    backfillKeys: [ENV_SMALL_FAST_MODEL, ENV_MODEL],
    supportsOneM: false,
  },
  {
    id: "fable",
    modelKey: "ANTHROPIC_DEFAULT_FABLE_MODEL",
    nameKey: "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    backfillKeys: ["ANTHROPIC_DEFAULT_OPUS_MODEL", ENV_MODEL],
    supportsOneM: true,
  },
  {
    id: "subagent",
    modelKey: "ANTHROPIC_DEFAULT_SUBAGENT_MODEL",
    nameKey: "ANTHROPIC_DEFAULT_SUBAGENT_MODEL_NAME",
    backfillKeys: ["ANTHROPIC_DEFAULT_SONNET_MODEL", ENV_MODEL],
    supportsOneM: true,
  },
]

function modelRole(role: ModelRoleId): ModelRole {
  const def = MODEL_ROLES.find((r) => r.id === role)
  if (!def) throw new Error(`Unknown model role: ${role}`)
  return def
}

/** Whether a model name carries the 1M marker. Read case-insensitively —
 *  proxies forward the marker lowercase upstream, Claude Code accepts both
 *  spellings, so the form strips either. */
export function hasOneM(model: string): boolean {
  return model.trimEnd().toLowerCase().endsWith("[1m]")
}

/** Strip a trailing 1M marker, leaving the bare model name. No marker → the
 *  input is returned unchanged; only the marker at the very end (with any
 *  trailing whitespace) is removed. */
export function stripOneM(model: string): string {
  if (!hasOneM(model)) return model
  return model.trimEnd().slice(0, -ONE_M_MARKER.length).trimEnd()
}

/** Apply (oneM) or remove the 1M marker — idempotent: an already-marked model
 *  is stripped first, so toggling never stacks markers. An empty model stays
 *  empty. */
export function setModelOneM(model: string, oneM: boolean): string {
  const base = stripOneM(model).trim()
  if (!base) return ""
  return oneM ? `${base}${ONE_M_MARKER}` : base
}

/** A role's effective model — its own env key first, then the backfill chain,
 *  then "". The raw env value is returned, `[1M]` marker included (the marker
 *  is a property of the model value, not of the read). */
export function configRoleModel(configText: string, role: ModelRoleId): string {
  const def = modelRole(role)
  const env = parseSettingsConfig(configText).env ?? {}
  const direct = env[def.modelKey]
  if (direct) return direct
  for (const key of def.backfillKeys) {
    const backfill = env[key]
    if (backfill) return backfill
  }
  return ""
}

/** A role's display name — the `_NAME` key, or the marker-free model name
 *  (the picker shows bare names, never the `[1M]` suffix). */
export function configRoleName(configText: string, role: ModelRoleId): string {
  const def = modelRole(role)
  const env = parseSettingsConfig(configText).env ?? {}
  return env[def.nameKey] || stripOneM(configRoleModel(configText, role))
}

/** Whether the role declares the 1M capability — its effective model carries
 *  the marker. Roles that do not support 1M always read false, even if a
 *  hand-edited snapshot carries a stray marker. */
export function configRoleHasOneM(
  configText: string,
  role: ModelRoleId,
): boolean {
  const def = modelRole(role)
  return def.supportsOneM && hasOneM(configRoleModel(configText, role))
}

/** The three fields the form edits per role, read together from the snapshot. */
export interface RoleFields {
  model: string
  name: string
  oneM: boolean
}

export function configRoleFields(
  configText: string,
  role: ModelRoleId,
): RoleFields {
  return {
    model: configRoleModel(configText, role),
    name: configRoleName(configText, role),
    oneM: configRoleHasOneM(configText, role),
  }
}

/** Rewrite a settingsConfig text's env via `write`, preserving every other
 *  field — the shared engine behind the role writes. */
function withEnvInText(
  configText: string,
  write: (env: Record<string, string>) => void,
): string {
  const config = parseSettingsConfig(configText)
  const env = { ...(config.env ?? {}) }
  write(env)
  return JSON.stringify({ ...config, env }, null, 2)
}

/**
 * Write a role's model into the settingsConfig text, syncing the display name
 * by the rule: when the role has no display name yet, or its display name
 * equals the old model name (marker stripped), it follows the new model —
 * a hand-typed model update keeps the picker label in step without clobbering
 * a custom name. Haiku (no 1M support) is stripped of any marker on write;
 * the other roles keep one typed or toggled in. Every write deletes the legacy
 * small-fast key — the role keys supersede it, and it must not linger
 * alongside them. An empty model clears the key (and a synced display name).
 */
export function withRoleModelInText(
  configText: string,
  role: ModelRoleId,
  model: string,
): string {
  const def = modelRole(role)
  const oldModelBase = stripOneM(configRoleModel(configText, role)).trim()
  const written = def.supportsOneM ? model.trim() : stripOneM(model)
  return withEnvInText(configText, (env) => {
    if (written) env[def.modelKey] = written
    else delete env[def.modelKey]
    delete env[ENV_SMALL_FAST_MODEL]
    const name = (env[def.nameKey] ?? "").trim()
    if (!name || name === oldModelBase) {
      const nextName = stripOneM(written).trim()
      if (nextName) env[def.nameKey] = nextName
      else delete env[def.nameKey]
    }
  })
}

/** Write a role's display name. Empty clears the key so the read-time default
 *  (the marker-free model name) shows again. */
export function withRoleNameInText(
  configText: string,
  role: ModelRoleId,
  name: string,
): string {
  const def = modelRole(role)
  return withEnvInText(configText, (env) => {
    const trimmed = name.trim()
    if (trimmed) env[def.nameKey] = trimmed
    else delete env[def.nameKey]
  })
}

/** Toggle the 1M marker on a role's model. Roles that do not support 1M are
 *  left untouched. The marker goes through the same write path as a typed
 *  model, so the display-name sync rule applies. */
export function withRoleOneMInText(
  configText: string,
  role: ModelRoleId,
  oneM: boolean,
): string {
  const def = modelRole(role)
  if (!def.supportsOneM) return configText
  return withRoleModelInText(
    configText,
    role,
    setModelOneM(configRoleModel(configText, role), oneM),
  )
}

/**
 * One-click apply: take the first filled model — the primary model, then the
 * roles in display order — and write it to every role, syncing display names
 * (marker-free) and stripping the marker for Haiku. A picked model that
 * carries `[1M]` propagates the marker to the roles that support it. Returns
 * null when no model is filled anywhere (callers disable the button).
 */
export function withAllRolesFromFirstInText(configText: string): string | null {
  const env = parseSettingsConfig(configText).env ?? {}
  const candidates = [
    env[ENV_MODEL],
    ...MODEL_ROLES.map((r) => env[r.modelKey]),
  ]
  const picked = candidates.find((m) => m?.trim())
  if (!picked) return null
  let next = configText
  for (const def of MODEL_ROLES) {
    next = withRoleModelInText(next, def.id, picked)
  }
  return next
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
