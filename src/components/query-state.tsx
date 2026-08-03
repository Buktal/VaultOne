// Unified loading / error / empty state for RTK Query results.
// Empty renders <EmptyState> so callers can attach an icon, description and
// next-step action instead of a bare string.

import type { LucideIcon } from "lucide-react"
import type { ReactNode } from "react"
import { useTranslation } from "react-i18next"

import { EmptyState } from "@/components/empty-state"
import { Skeleton } from "@/components/ui/skeleton"
import { describeError } from "@/lib/error"

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
        {t("common.loadFailed", {
          detail: describeError(error, t) || t("common.unknownError"),
        })}
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
