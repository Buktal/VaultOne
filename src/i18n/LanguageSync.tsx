// Drives i18next + the dayjs locale from the persisted display-language
// preference (ADR-0016). Mounted once inside <AppProviders>. Before the
// preferences query resolves, both stay at their English default (the app
// default), so there is no flash of the wrong language on cold start. A live
// change in Settings re-runs this effect and re-renders every `useTranslation`
// consumer + rebuilds the tray (the latter in the `set_language` command).

import { useEffect } from "react"
import { usePreferencesQuery } from "@/app/store/api"
import i18n from "@/i18n"
import { setDayjsLocale } from "@/i18n/languages"

export function LanguageSync() {
  const { data: prefs } = usePreferencesQuery()
  const language = prefs?.language
  useEffect(() => {
    if (!language) return
    void i18n.changeLanguage(language)
    setDayjsLocale(language)
  }, [language])
  return null
}
