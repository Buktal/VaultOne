// Pure navigation derivations for the library browser: splitting an entry's
// rel_path into device + subpath, resolving a "go up" click to its target, and
// building the breadcrumb trail. Extracted from the view so the navigation
// rules are testable in isolation (architecture.md: "关键不变量用代码表达") —
// the hook wires these to React state, these own the math.

/** One row in the device scope picker. Built in the hook (labels need i18n)
 *  and consumed by buildBreadcrumb to label the device crumb. */
export interface DeviceOption {
  id: string
  label: string
}

/** A breadcrumb entry as pure data: a render label plus the navigation target
 *  the hook wires to setDeviceScope / setSubpath on click. No callbacks live
 *  here so the structure stays testable without React. */
export interface BreadcrumbCrumb {
  key: string
  label: string
  deviceScope: string
  subpath: string
}

/**
 * Split a library entry's rel_path (`<deviceId>/<rest...>`) into the owning
 * device id and the subpath below it. Drilling into a directory uses this to
 * narrow deviceScope + subpath in one step.
 */
export function splitEntryPath(relPath: string): {
  deviceId: string
  rest: string
} {
  const [deviceId, ...rest] = relPath.split("/")
  return { deviceId, rest: rest.join("/") }
}

/**
 * Resolve a "go up" click on the current subpath into its navigation target.
 * Returns `deviceScope: undefined` when the up-click leaves deviceScope
 * untouched (subpath has at most one segment — subpath just clears back to the
 * device root); otherwise the device id to restore. The first segment of
 * `subpath` carries the device id under the existing breadcrumb/up semantics.
 */
export function upFromSubpath(subpath: string): {
  deviceScope: string | undefined
  subpath: string
} {
  const parts = subpath.split("/").filter(Boolean)
  if (parts.length <= 1) return { deviceScope: undefined, subpath: "" }
  return {
    deviceScope: parts[0],
    subpath: parts.slice(1, -1).join("/"),
  }
}

/**
 * Build the breadcrumb trail for the current subpath as pure data (labels +
 * navigation targets, no callbacks). The first segment of `subpath` carries
 * the device id; its label is resolved against `deviceOptions`, falling back
 * to the raw id. Returns an empty list when subpath is empty.
 */
export function buildBreadcrumb(
  subpath: string,
  deviceOptions: DeviceOption[],
): BreadcrumbCrumb[] {
  if (!subpath) return []
  const parts = subpath.split("/").filter(Boolean)
  const deviceId = parts[0]
  const deviceLabel =
    deviceOptions.find((o) => o.id === deviceId)?.label ?? deviceId
  const crumbs: BreadcrumbCrumb[] = [
    { key: deviceId, label: deviceLabel, deviceScope: deviceId, subpath: "" },
  ]
  for (let i = 1; i < parts.length; i++) {
    const sub = parts.slice(1, i + 1).join("/")
    crumbs.push({
      key: `${deviceId}/${sub}`,
      label: parts[i],
      deviceScope: deviceId,
      subpath: sub,
    })
  }
  return crumbs
}
