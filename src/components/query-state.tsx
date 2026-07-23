// Unified loading / error / empty state for RTK Query results (ADR-0007).
// Empty renders <EmptyState> so callers can attach an icon, description and
// next-step action instead of a bare string.

import i18n from "i18next"
import type { LucideIcon } from "lucide-react"
import type { ReactNode } from "react"
import { useTranslation } from "react-i18next"

import { EmptyState } from "@/components/empty-state"
import { Skeleton } from "@/components/ui/skeleton"

export function QueryState({
  isLoading,
  error,
  isEmpty,
  emptyLabel,
  emptyIcon,
  emptyDescription,
  emptyAction,
  children,
}: {
  isLoading: boolean
  error: unknown
  isEmpty: boolean
  emptyLabel?: string
  emptyIcon?: LucideIcon
  emptyDescription?: string
  emptyAction?: { label: string; onClick: () => void; disabled?: boolean }
  children: ReactNode
}) {
  const { t } = useTranslation()
  if (isLoading) {
    return <Skeleton className="h-24 w-full rounded-md" />
  }
  if (error) {
    return (
      <div className="text-destructive text-sm">
        {t("common.loadFailed", { detail: describeError(error) })}
      </div>
    )
  }
  if (isEmpty) {
    return (
      <EmptyState
        icon={emptyIcon}
        title={emptyLabel ?? t("common.empty")}
        description={emptyDescription}
        action={emptyAction}
      />
    )
  }
  return <>{children}</>
}

export function describeError(e: unknown): string {
  if (!e) return i18n.t("common.unknownError")
  if (
    typeof e === "object" &&
    "data" in e &&
    typeof (e as { data: unknown }).data === "string"
  ) {
    return (e as { data: string }).data
  }
  if (typeof e === "object" && "error" in e) {
    return JSON.stringify((e as { error: unknown }).error)
  }
  return JSON.stringify(e)
}
