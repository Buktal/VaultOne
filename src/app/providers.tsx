// App providers (ADR-0007): Redux <Provider> + next-themes <ThemeProvider> +
// base-ui <TooltipProvider> + Toaster. next-themes toggles `.dark` on <html>;
// attribute="class" matches the `@custom-variant dark` in index.css. The Toaster
// relies on useTheme() so the ThemeProvider must wrap it, else toasts never
// follow the active theme. TooltipProvider is mounted once here so every
// <Tooltip> in the tree shares delay/hover config without re-wrapping.

import { listen } from "@tauri-apps/api/event"
import { ThemeProvider } from "next-themes"
import type { ReactNode } from "react"
import { useEffect } from "react"
import { Provider } from "react-redux"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"

import { CloseRequestedDialog } from "./close-requested-dialog"
import { vaultApi } from "./store/api"
import { store } from "./store/store"
import { setMode } from "./store/slices/viewSlice"

export function AppProviders({ children }: { children: ReactNode }) {
  // ADR-0005 event-driven refresh: Rust emits `usage_changed` after writing the
  // Local Store (collect / sync); invalidate the derived caches so views
  // re-query. The consolidated `vaultApi` owns every endpoint.
  useEffect(() => {
    const off = listen("usage_changed", () => {
      store.dispatch(vaultApi.util.invalidateTags(["Usage", "Logs", "Models"]))
    })
    return () => {
      off.then((unlisten) => unlisten())
    }
  }, [])

  // ADR-0015: tray left-click means "show the full dashboard" (ADR-0012). If the
  // window is in lightweight mode, morph back — setMode("full") is a no-op when
  // already full, and useWindowMode restores the window geometry on the change.
  useEffect(() => {
    const off = listen("tray-show-main", () => {
      store.dispatch(setMode("full"))
    })
    return () => {
      off.then((unlisten) => unlisten())
    }
  }, [])

  return (
    <Provider store={store}>
      <ThemeProvider
        attribute="class"
        defaultTheme="dark"
        enableSystem
        disableTransitionOnChange
      >
        <TooltipProvider>
          {children}
          <CloseRequestedDialog />
          <Toaster richColors closeButton />
        </TooltipProvider>
      </ThemeProvider>
    </Provider>
  )
}
