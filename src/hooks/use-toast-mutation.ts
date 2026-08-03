// Single-point strategy for "fire an RTK Query mutation → toast the outcome".
//
// The pattern (await trigger → on error toast.error with a describeError
// description, on success toast.success) was hand-copied at ~15 call sites.
// This hook collects it in one place: every site passes i18n keys (and, where
// the success message depends on the resolved payload — e.g. "fetched N
// entries" — a tiny message builder) and reacts to the returned boolean for
// side effects (close dialog, clear inputs, mark freshness …). RTK cache
// invalidation is untouched — it is driven by each endpoint's
// invalidatesTags, never by toasts.
//
// The failure description always goes through #6's `describeError`, so the
// type→i18n mapping (errors.<type>) also lives in exactly one place. The
// mutation result is never thrown — RTK resolves it as `{ data } | { error }`
// — so the trigger is awaited directly.

import type { SerializedError } from "@reduxjs/toolkit"
import { useCallback } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"

import { describeError } from "@/lib/error"
import type { AppError } from "@/types/generated/bindings"

/** Resolved shape of an RTK Query mutation. The endpoint error is `AppError`
 *  (fakeBaseQuery<AppError>), but RTK always unions in `SerializedError` for
 *  internal failures (e.g. a rejected precondition), so both are possible.
 *  RTK types both branches as optional; the `in`/truthy check at runtime is
 *  authoritative. `describeError` accepts either (it takes `unknown`). */
type ToastResult<R> = { data?: R; error?: AppError | SerializedError }

/** Static success toast: an i18n key with optional interpolation vars. The
 *  vars are evaluated by the caller at call time, so closure values (a draft
 *  model key, a count from the request, …) are captured as intended. */
export interface ToastSuccessStatic {
  key: string
  vars?: Record<string, unknown>
}

/** Dynamic success toast: build the message from the resolved payload. Use
 *  this when the message interpolates server-returned data (e.g. rows
 *  inserted, entries fetched). The caller closes over its own `t`. */
export interface ToastSuccessDynamic<R> {
  message: (data: R) => string
}

export type ToastSuccess<R> = ToastSuccessStatic | ToastSuccessDynamic<R>

export interface ToastFailure {
  /** i18n key for the failure toast title. */
  key: string
}

export interface ToastMutationOptions<R> {
  /** Success toast. Omit entirely (e.g. instant-effect preference changes that
   *  give no success feedback) to suppress the success toast. */
  success?: ToastSuccess<R>
  /** Failure toast title; its description is always derived via describeError. */
  failed: ToastFailure
}

/**
 * Run an RTK Query mutation trigger and toast the outcome through one shared
 * strategy.
 *
 * @returns `true` on success (caller runs side effects) or `false` after
 *   firing the error toast.
 */
export function useMutateWithToast() {
  const { t } = useTranslation()
  return useCallback(
    async <A, R>(
      // PromiseLike, not Promise: an RTK Query trigger returns a
      // MutationActionCreatorResult (a thenable), which is assignable to
      // PromiseLike but not to a plain Promise.
      trigger: (arg: A) => PromiseLike<ToastResult<R>>,
      arg: A,
      opts: ToastMutationOptions<R>,
    ): Promise<boolean> => {
      const r = await trigger(arg)
      if (r.error) {
        toast.error(t(opts.failed.key), {
          description: describeError(r.error, t) || t("common.unknownReason"),
        })
        return false
      }
      if (opts.success) {
        const s = opts.success
        toast.success(
          "message" in s ? s.message(r.data as R) : t(s.key, s.vars),
        )
      }
      return true
    },
    [t],
  )
}
