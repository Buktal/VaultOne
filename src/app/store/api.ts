// Base RTK Query api (ADR-0007 / 0008). Endpoints live in feature files and are
// injected with `injectEndpoints`; each endpoint's `queryFn` calls the typed
// tauri-specta command directly (no stringly-typed `invoke`). The base query is
// a no-op because the real data fetch is the typed command, not an HTTP call.
//
// Injection is triggered by importing the feature api modules in `store.ts`
// (keeps this file free of feature imports → no import cycle; ADR-0008 intent).

import { createApi } from "@reduxjs/toolkit/query/react"

const NO_OP_BASE_QUERY = () => ({ data: null })

export const baseSplitApi = createApi({
  baseQuery: NO_OP_BASE_QUERY,
  tagTypes: ["Usage", "Pricing", "Device", "App", "Sync"],
  endpoints: () => ({}),
})

// The fully-injected api (feature modules augment this at import time).
export const api = baseSplitApi
