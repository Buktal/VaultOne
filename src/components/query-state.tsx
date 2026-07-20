// Unified loading / error / empty state for RTK Query results (ADR-0007).
// Empty renders <EmptyState> so callers can attach an icon, description and
// next-step action instead of a bare string.

import type { LucideIcon } from "lucide-react"
import type { ReactNode } from "react"

import { EmptyState } from "@/components/empty-state"
import { Skeleton } from "@/components/ui/skeleton"

export function QueryState({
  isLoading,
  error,
  isEmpty,
  emptyLabel = "暂无数据",
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
  if (isLoading) {
    return <Skeleton className="h-24 w-full rounded-md" />
  }
  if (error) {
    return (
      <div className="text-destructive text-sm">
        加载失败：{describeError(error)}
      </div>
    )
  }
  if (isEmpty) {
    return (
      <EmptyState
        icon={emptyIcon}
        title={emptyLabel}
        description={emptyDescription}
        action={emptyAction}
      />
    )
  }
  return <>{children}</>
}

export function describeError(e: unknown): string {
  if (!e) return "未知错误"
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
