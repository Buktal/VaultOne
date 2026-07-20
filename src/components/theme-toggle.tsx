// Theme toggle — cycles light → dark → system (next-themes). Icon-only with a
// tooltip; mounted in the CommandBar.

import { Monitor, Moon, Sun } from "lucide-react"
import { useTheme } from "next-themes"

import { Button } from "@/components/ui/button"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

const NEXT: Record<string, "light" | "dark" | "system"> = {
  light: "dark",
  dark: "system",
  system: "light",
}

const META: Record<string, { Icon: typeof Sun; label: string }> = {
  light: { Icon: Sun, label: "浅色" },
  dark: { Icon: Moon, label: "深色" },
  system: { Icon: Monitor, label: "跟随系统" },
}

export function ThemeToggle() {
  const { theme = "system", setTheme } = useTheme()
  const { Icon, label } = META[theme] ?? META.system

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`切换主题（当前：${label}）`}
            onClick={() => setTheme(NEXT[theme] ?? "dark")}
          />
        }
      >
        <Icon />
      </TooltipTrigger>
      <TooltipContent>主题 · {label}</TooltipContent>
    </Tooltip>
  )
}
