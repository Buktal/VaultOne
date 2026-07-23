// Supported display languages (ADR-0016). The single source of truth that the
// Rust `Language` enum, this registry, the `src/locales/*.json` files, and the
// dayjs locale map must all stay in agreement with. To add a language: extend
// Rust `Language`, drop a JSON in `src/locales/`, register it here, and add the
// dayjs locale import below.

import dayjs from "dayjs"
import "dayjs/locale/ja"
import "dayjs/locale/zh-cn"

import type { Language } from "@/types/preferences"

export interface LanguageOption {
  /** Rust `Language` code (serde lowercase). */
  code: Language
  /** Native-name label shown in the selector — so users find theirs regardless of the active UI language. */
  nativeName: string
  /** dayjs locale name; drives relative-time (`fromNow`). */
  dayjsLocale: string
}

export const LANGUAGES: readonly LanguageOption[] = [
  { code: "en", nativeName: "English", dayjsLocale: "en" },
  { code: "zh", nativeName: "中文", dayjsLocale: "zh-cn" },
  { code: "ja", nativeName: "日本語", dayjsLocale: "ja" },
]

const byCode = new Map<Language, LanguageOption>(
  LANGUAGES.map((o) => [o.code, o]),
)

/** Set dayjs's global locale so `fromNow()` follows the display language. */
export function setDayjsLocale(code: Language): void {
  dayjs.locale(byCode.get(code)?.dayjsLocale ?? "en")
}
