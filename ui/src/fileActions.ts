// 项目路径动作（components A16b 第一批「文件树行」，owner-followups-1015 #07）。
// 右键菜单、命令面板、keymap 三个入口共用这里——右键不是唯一入口（A16b「定位」）。
// 路径一律项目相对（与树、页签、# 路径块同一形）；「在访达中显示」的根约束在 core 判，这里不判。
// 例外：「复制路径」复制绝对路径——画布页签自票 29、文件树行自 #30（2026-10-20 负责人裁决：两处曾一相对一绝对，
// 同一文件复制出两种形；票 05 / #07 原为相对路径）。两处都走 copyProjectPath（= absolutePath + copyPath）。
import i18n from './i18n'
import { api } from './api'
import { insertComposerPath } from './composerFill'
import { isMac } from './keymap'
import { useUiStore } from './store'
import { toastError } from './toast'

/** 揭示动作的平台名（访达 / 资源管理器 / 文件管理器），只影响文案。 */
export type RevealPlatform = 'mac' | 'windows' | 'linux'

const uaData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData
export const revealPlatform: RevealPlatform = isMac
  ? 'mac'
  : (uaData ? uaData.platform === 'Windows' : /Windows/.test(navigator.userAgent)) ? 'windows' : 'linux'

/** 「在访达中显示」/「在资源管理器中显示」/「在文件管理器中显示」的 i18n key。 */
export function revealLabelKey(platform: RevealPlatform = revealPlatform): string {
  return platform === 'mac' ? 'ctx.revealMac' : platform === 'windows' ? 'ctx.revealWindows' : 'ctx.revealLinux'
}

/** 把给定路径原样写入剪贴板；成功 toast「已复制路径」，失败走 toastError（A16b 错误态：菜单已关）。 */
export async function copyPath(path: string) {
  try {
    await navigator.clipboard.writeText(path)
    useUiStore.getState().pushToast(i18n.t('ctx.pathCopied'), 'ok')
  } catch (e) {
    toastError(e)
  }
}

/**
 * 项目根 + 项目相对路径 → 绝对路径（票 29：画布页签「复制路径」）。分隔符跟项目根走：根是 Windows 形
 * （含反斜杠、不含 `/`）时相对路径的 `/` 也换成反斜杠；根末尾的分隔符不重复。没有项目根时原样返回相对路径。
 */
export function absolutePath(root: string | null, rel: string): string {
  if (!root) return rel
  const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const base = root.endsWith(sep) ? root.slice(0, -1) : root
  return `${base}${sep}${sep === '/' ? rel : rel.replaceAll('/', sep)}`
}

/**
 * 绝对路径 → 项目相对（#32 文件菜单：时间线 / 执行卡 / 附件里的路径有的是绝对形）。在根内 → 相对（根本身 = 空串）；
 * 根外、没有项目根、或本来就是相对形 → 相对形原样返回、绝对形返回 null（null = 不出路径类项与「打开方式」；
 * 根约束仍由 core 判，这里只决定显不显示）。分隔符同 absolutePath 跟项目根走。
 */
export function projectRelPath(path: string, root: string | null): string | null {
  const isAbs = path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith('\\\\')
  if (!isAbs) {
    const rel = path.replace(/^\.\//, '')
    return rel.split(/[\\/]/).includes('..') ? null : rel
  }
  if (!root) return null
  const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const base = root.endsWith(sep) ? root.slice(0, -1) : root
  if (path === base) return ''
  if (!path.startsWith(base + sep)) return null
  const rel = path.slice(base.length + 1)
  return sep === '/' ? rel : rel.replaceAll('\\', '/')
}

/**
 * 产物登记路径 → 仓内路径：产物落在 `.hexagon/<path>`（core artifacts::content / deliver），登记的 path 是 `.hexagon/` 内相对。
 * 修复即文档（#32 2026-10-25）：#29 / #31 的产物页签「复制路径」「打开方式」直接用登记 path，指到项目根下同名路径
 * （不存在 → errors.open_with_missing，或复制出一条错路径）。绝对形 / 已带 `.hexagon/` 的原样返回。
 */
export function artifactRepoPath(path: string): string {
  if (path.startsWith('/') || path.startsWith('.hexagon/')) return path
  return `.hexagon/${path.replace(/^\.\//, '')}`
}

/** 复制项目内路径的绝对形（文件树行与画布页签共用，#30）：项目根 + 相对路径，没有项目根时退回相对路径。 */
export async function copyProjectPath(rel: string) {
  await copyPath(absolutePath(useUiStore.getState().projectRoot, rel))
}

/** 在系统文件管理器中显示。越界 / 不存在由 core 拒（errors.reveal_*），这里只出 toast。 */
export async function revealPath(path: string) {
  try {
    await api.revealRepoPath(path)
  } catch (e) {
    toastError(e)
  }
}

/**
 * 插入到输入条（无障碍 §3：拖路径进输入条的替代）。同拖入：一条 # 路径块、不读不展开。
 * 先回对话（全宽时输入条随对话收起），插入同步生效，聚焦等下一帧（同 sendSelectionToComposer）。
 */
export function insertPathToComposer(path: string) {
  useUiStore.getState().showConversation()
  insertComposerPath(path)
  requestAnimationFrame(() => document.getElementById('composer-input')?.focus())
}

/** 文件树行上挂的路径属性：键位入口据此找焦点行。 */
export const TREE_PATH_ATTR = 'data-tree-path'

/**
 * 键位入口的作用对象之一：焦点在文件树行上 → 那一行（路径 + 种类）；否则 null，
 * 调用方回落到画布当前文件 / 产物页签（tabActions.tabPath）。链接没有一条仓内路径可插。
 */
export function treeRowTarget(): { path: string; kind: 'file' | 'dir' | 'link' } | null {
  const row = (document.activeElement as HTMLElement | null)?.closest?.<HTMLElement>(`[${TREE_PATH_ATTR}]`)
  if (!row) return null
  const kind = row.dataset.treeKind
  return { path: row.getAttribute(TREE_PATH_ATTR)!, kind: kind === 'dir' || kind === 'link' ? kind : 'file' }
}
