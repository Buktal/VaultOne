// App providers (ADR-0007): Redux <Provider> + Toaster. No theme provider — the
// theme hook toggles `.dark` on <html> directly.

import { listen } from "@tauri-apps/api/event"
import type { ReactNode } from "react"
import { useEffect } from "react"
import { Provider } from "react-redux"
import { Toaster } from "@/components/ui/sonner"

import { api } from "./store/api"
import { store } from "./store/store"

export function AppProviders({ children }: { children: ReactNode }) {
  // ADR-0005 event-driven refresh: Rust emits `usage_changed` after writing the
  // Local Store (collect / sync); invalidate the Usage cache so views re-query.
  useEffect(() => {
    const off = listen("usage_changed", () => {
      store.dispatch(api.util.invalidateTags(["Usage"]))
    })
    return () => {
      off.then((unlisten) => unlisten())
    }
  }, [])

  return (
    <Provider store={store}>
      {children}
      <Toaster richColors closeButton />
    </Provider>
  )
}
