// Reflects the persisted color skin onto <html data-skin="…">. `pixso` (the
// default) clears the attribute so the :root/.dark baseline applies; any other
// skin sets it, and the [data-skin="…"] blocks in index.css override ONLY the
// data palette + brand — never fonts or neutral surfaces (the iron rule in
// index.css). Independent of next-themes, which still owns the `.dark` class;
// the two dimensions compose freely (mode × skin).

import { useEffect } from "react"
import { usePreferencesQuery } from "@/app/store/api"

/** Sync <html data-skin> with the persisted preference. Mount once, inside the
 *  Redux <Provider> (it reads usePreferencesQuery). */
export function useSkinEffect(): void {
  const { data: prefs } = usePreferencesQuery()
  const skin = prefs?.skin
  useEffect(() => {
    const el = document.documentElement
    if (skin && skin !== "pixso") {
      el.setAttribute("data-skin", skin)
    } else {
      el.removeAttribute("data-skin")
    }
  }, [skin])
}
