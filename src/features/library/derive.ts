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
 * Resolve a "go up" click: drop the last segment of `subpath`. `deviceScope` is
 * independent of `subpath` — the scan query takes them as separate args, and
 * `drill` sets deviceScope + a subpath that does NOT contain the device id — so
 * going up never touches deviceScope (only the scope picker returns to "all
 * devices"). Returns the new subpath.
 */
export function upFromSubpath(subpath: string): string {
  const parts = subpath.split("/").filter(Boolean)
  return parts.slice(0, -1).join("/")
}

/**
 * Build the breadcrumb trail as pure data (labels + navigation targets, no
 * callbacks). The device crumb is labelled from `deviceScope` (passed in —
 * `subpath` never carries the device id, matching how `drill` and the scan
 * query treat them as separate values); each following crumb adds one more
 * `subpath` segment. Every crumb keeps the same `deviceScope` — clicking one
 * navigates within the current device, only `subpath` changes. Returns an empty
 * list when subpath is empty (the scope picker, not the breadcrumb, handles
 * leaving the device).
 */
export function buildBreadcrumb(
  deviceScope: string,
  subpath: string,
  deviceOptions: DeviceOption[],
): BreadcrumbCrumb[] {
  if (!subpath) return []
  const deviceLabel =
    deviceOptions.find((o) => o.id === deviceScope)?.label ?? deviceScope
  const parts = subpath.split("/").filter(Boolean)
  const crumbs: BreadcrumbCrumb[] = [
    { key: deviceScope, label: deviceLabel, deviceScope, subpath: "" },
  ]
  for (let i = 0; i < parts.length; i++) {
    const sub = parts.slice(0, i + 1).join("/")
    crumbs.push({
      key: `${deviceScope}/${sub}`,
      label: parts[i],
      deviceScope,
      subpath: sub,
    })
  }
  return crumbs
}
