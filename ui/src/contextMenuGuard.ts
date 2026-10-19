// 全局右键拦截（components A16b「系统菜单」，2026-10-17 负责人裁决；owner-followups-1015 #05）。
// WebView 自带菜单只有「重新载入 / 检查元素」——负责人误点刷新会丢掉画布草稿与弹层状态，所以整个应用拦掉；
// 未定义右键菜单的表面右键无反应。豁免：input / textarea / select / contenteditable 保留系统菜单
// （拼写、粘贴、查询），Monaco 保留自己的菜单（明文例外）。
//
// 挂在 window 的冒泡阶段（不是捕获）：React 的根监听先跑，radix ContextMenu 在触发器上 preventDefault 并打开菜单；
// 若在捕获阶段先 preventDefault，合成事件读到 defaultPrevented，radix 的 composeEventHandlers 会跳过打开——
// 所有自定义右键菜单就都失灵了。
const EXEMPT = 'input, textarea, select, [contenteditable]:not([contenteditable="false"]), .monaco-editor'

/** 右键目标是否保留系统 / 编辑器自己的菜单。 */
export function keepsNativeMenu(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(EXEMPT) != null
}

/** 装上全局拦截；返回卸载函数。 */
export function installContextMenuGuard(win: Window = window): () => void {
  const onContextMenu = (e: MouseEvent) => {
    if (!keepsNativeMenu(e.target)) e.preventDefault()
  }
  win.addEventListener('contextmenu', onContextMenu)
  return () => win.removeEventListener('contextmenu', onContextMenu)
}
