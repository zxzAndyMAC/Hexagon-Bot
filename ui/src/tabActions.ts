// 画布页签的批量 / 路径动作（components A16b 第一批「画布页签」，owner-followups-1015 #05）。
// 右键菜单、命令面板、keymap 三个入口共用这里——右键不是唯一入口（A16b「定位」）。
// 「关闭」不在这里：复用 store.closeTab。
import i18n from './i18n'
import { useUiStore, type WorkTab } from './store'
import { toastError } from './toast'

/** 目标右侧的页签 id（目标不在列表里时为空）。 */
export function tabsRightOf(tabs: readonly WorkTab[], id: string): string[] {
  const i = tabs.findIndex((t) => t.id === id)
  return i < 0 ? [] : tabs.slice(i + 1).map((t) => t.id)
}

/** 除目标外的页签 id。 */
export function otherTabs(tabs: readonly WorkTab[], id: string): string[] {
  return tabs.some((t) => t.id === id) ? tabs.filter((t) => t.id !== id).map((t) => t.id) : []
}

/** 可复制的项目相对路径：只有文件 / 产物页签有（Agent、diff、列表页签 → null，菜单项不渲染）。 */
export function tabPath(tab: WorkTab | undefined): string | null {
  return tab && (tab.kind === 'file' || tab.kind === 'artifact') && tab.path ? tab.path : null
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

/** 复制页签路径；成功 toast「已复制路径」，失败走 toastError（A16b 错误态：菜单已关）。 */
export async function copyTabPath(id: string) {
  const path = tabPath(tabsNow().find((t) => t.id === id))
  if (!path) return
  try {
    await navigator.clipboard.writeText(path)
    useUiStore.getState().pushToast(i18n.t('ctx.pathCopied'), 'ok')
  } catch (e) {
    toastError(e)
  }
}
