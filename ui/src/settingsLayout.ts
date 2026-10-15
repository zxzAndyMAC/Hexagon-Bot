// 设置「列表 | 详情」的列表宽度（ui-redesign 票 11；components E2，interaction §1「区域尺寸」）。
// 默认 280，220–360，拖到下限停住、不吸附；双击还原。宽度按窗口存 `hexagon.layout.settingsList`
// （与 `hexagon.layout.side` 同一做法）。拖动与键盘取值复用 layout.ts 的 resolveDrag / keyTarget，
// 这里只放本分割线的尺寸与持久化——不改 layout.ts，避免与画布票（03）在同一文件上冲突。

export const SETTINGS_LIST = { default: 280, min: 220, max: 360 } as const
export const SETTINGS_LIST_KEY = 'hexagon.layout.settingsList'

type KV = Pick<Storage, 'getItem' | 'setItem'>

const clamp = (n: number) => Math.min(SETTINGS_LIST.max, Math.max(SETTINGS_LIST.min, n))

/** 读列表宽度：内容损坏回默认，不抛（布局记忆坏了只花一次重新拖动）。 */
export function readSettingsListWidth(storage: KV): number {
  try {
    const raw = storage.getItem(SETTINGS_LIST_KEY)
    if (raw == null) return SETTINGS_LIST.default
    const v = JSON.parse(raw) as { width?: unknown }
    if (typeof v !== 'object' || v == null || typeof v.width !== 'number' || !Number.isFinite(v.width)) return SETTINGS_LIST.default
    return Math.round(clamp(v.width))
  } catch {
    return SETTINGS_LIST.default
  }
}

export function writeSettingsListWidth(storage: KV, width: number) {
  storage.setItem(SETTINGS_LIST_KEY, JSON.stringify({ width: Math.round(clamp(width)) }))
}
