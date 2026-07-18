// Unified loading / error / empty state for RTK Query results (ADR-0007).

import type { ReactNode } from "react"
import { Skeleton } from "@/components/ui/skeleton"

/** Render a skeleton block while loading, or a message for error/empty. */
export function QueryState({
  isLoading,
  error,
  isEmpty,
  emptyLabel = "暂无数据",
  children,
}: {
  isLoading: boolean
  error: unknown
  isEmpty: boolean
  emptyLabel?: string
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
    return <div className="text-muted-foreground text-sm">{emptyLabel}</div>
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
