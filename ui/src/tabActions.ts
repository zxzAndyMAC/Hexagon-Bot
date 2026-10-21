// 画布页签的批量 / 路径动作（components A16b 第一批「画布页签」，owner-followups-1015 #05）。
// 右键菜单、命令面板、keymap 三个入口共用这里——右键不是唯一入口（A16b「定位」）。
// 「关闭」不在这里：复用 store.closeTab。
import { isPinnedTab, useUiStore, type WorkTab } from './store'
import { copyProjectPath } from './fileActions'

/** 可见顺序（A22「溢出」，#06）：固定页签（产物 / 项目文件）在前，其余保持原序。页签栏、「全部页签」菜单、
 * 「关闭右侧」、上 / 下一个页签都按它走——store 由 openTab 维持同序，这里兜住直接 setState 的旧顺序。 */
export function pinnedFirst(tabs: readonly WorkTab[]): WorkTab[] {
  return [...tabs.filter(isPinnedTab), ...tabs.filter((t) => !isPinnedTab(t))]
}

/** 目标右侧（可见顺序）的页签 id（目标不在列表里时为空）。 */
export function tabsRightOf(tabs: readonly WorkTab[], id: string): string[] {
  const ordered = pinnedFirst(tabs)
  const i = ordered.findIndex((t) => t.id === id)
  return i < 0 ? [] : ordered.slice(i + 1).map((t) => t.id)
}

/** 可见顺序里相邻的页签（dir 1 下一个 / -1 上一个，首尾循环）；只有一个或当前不在列表里时为 null。 */
export function adjacentTab(tabs: readonly WorkTab[], id: string | null, dir: 1 | -1): string | null {
  const ordered = pinnedFirst(tabs)
  const i = ordered.findIndex((t) => t.id === id)
  if (i < 0 || ordered.length < 2) return null
  return ordered[(i + dir + ordered.length) % ordered.length].id
}

/** 除目标外的页签 id。 */
export function otherTabs(tabs: readonly WorkTab[], id: string): string[] {
  return tabs.some((t) => t.id === id) ? tabs.filter((t) => t.id !== id).map((t) => t.id) : []
}

/** 可复制的项目相对路径：只有文件 / 产物页签有（Agent、diff、列表页签 → null，菜单项不渲染）。 */
export function tabPath(tab: WorkTab | undefined): string | null {
  return tab && (tab.kind === 'file' || tab.kind === 'artifact') && tab.path ? tab.path : null
}

/** 页签列滚动（A22「溢出」，#06）：把页签拉进视区要给 scrollLeft 加多少：左边露不全 → 负（左缘对齐）；右边露不全 → 正（右缘对齐）；比视区宽 → 左缘对齐。 */
export function revealDelta(view: { left: number; right: number }, tab: { left: number; right: number }): number {
  if (tab.left < view.left || tab.right - tab.left > view.right - view.left) return tab.left - view.left
  if (tab.right > view.right) return tab.right - view.right
  return 0
}

const tabsNow = () => useUiStore.getState().tabs

export function closeOtherTabs(id: string) {
  const ids = otherTabs(tabsNow(), id)
  if (ids.length) useUiStore.getState().closeTabs(ids)
}

export function closeTabsToRight(id: string) {
  const ids = tabsRightOf(tabsNow(), id)
  if (ids.length) useUiStore.getState().closeTabs(ids)
}

/** nextTab / prevTab（mod+shift+] / mod+shift+[）：切到可见顺序的下一个 / 上一个页签。 */
export function stepTab(dir: 1 | -1) {
  const s = useUiStore.getState()
  const next = adjacentTab(s.tabs, s.activeTab, dir)
  if (next) s.setActiveTab(next)
}

/**
 * 复制页签路径（与文件树行同一出口 fileActions.copyProjectPath，#07 / #30）。右键菜单、命令面板、copyPath 键位共用。
 * 写绝对路径：票 05 原写项目相对路径，2026-10-20 负责人裁决改为绝对路径（票 29）；文件树行随后同改（#30）。
 */
export async function copyTabPath(id: string) {
  const path = tabPath(tabsNow().find((t) => t.id === id))
  if (path) await copyProjectPath(path)
}
