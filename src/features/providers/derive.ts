// Pure derivations for the providers list & form: reading the basic form
// fields (endpoint / API key / model) out of a provider's `settingsConfig`
// snapshot, and rebuilding that snapshot from the form while preserving every
// other field. Extracted from the view/hook so each rule is testable in
// isolation — the single authority for how the form maps onto the
// settings.json text.

import type { Provider } from "@/types/generated/bindings"

// The env keys the basic form knows about. The endpoint and auth key are the
// two spellings Claude Code accepts (the form writes AUTH_TOKEN but reads
// either, so a provider configured with API_KEY edits cleanly).
const ENV_BASE_URL = "ANTHROPIC_BASE_URL"
const ENV_AUTH_TOKEN = "ANTHROPIC_AUTH_TOKEN"
const ENV_API_KEY = "ANTHROPIC_API_KEY"
const ENV_MODEL = "ANTHROPIC_MODEL"

type SettingsConfig = { env?: Record<string, string> }

/** Parse a provider's settingsConfig JSON text; garbage or empty → `{}` so a
 *  corrupt snapshot never throws the form open. A non-object `env` (a string,
 *  an array — anything a hand-edited snapshot could hold) is dropped to `{}`
 *  so the write-back never spreads e.g. a string into character-index keys. */
function parseSettingsConfig(config: string): SettingsConfig {
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

function envValue(provider: Provider, key: string): string {
  return parseSettingsConfig(provider.settingsConfig).env?.[key] ?? ""
}

/** The provider's base URL (endpoint), from `env.ANTHROPIC_BASE_URL`. */
export function providerEndpoint(provider: Provider): string {
  return envValue(provider, ENV_BASE_URL)
}

/** The API key — reads AUTH_TOKEN first, then API_KEY (the form writes the
 *  former; the latter is a legacy spelling some providers use). */
export function providerApiKey(provider: Provider): string {
  return envValue(provider, ENV_AUTH_TOKEN) || envValue(provider, ENV_API_KEY)
}

/** The primary model, from `env.ANTHROPIC_MODEL`. Display-only for now — the
 *  five-role model mapping is a later ticket. */
export function providerModel(provider: Provider): string {
  return envValue(provider, ENV_MODEL)
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
  const config = parseSettingsConfig(provider.settingsConfig)
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
  return {
    ...provider,
    settingsConfig: JSON.stringify({ ...config, env }, null, 2),
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
