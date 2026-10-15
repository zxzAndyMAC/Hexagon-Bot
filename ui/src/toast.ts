// 失败 toast 的统一出口（ui-redesign 票 13，components A18 错误行 / C5 文案形）。
// 文案仍由 errText 出（errors.<code> 已登记则译文，否则宿主原话）；
// 原因码只在「已登记的领域码」时附上——unknown / internal 之类不进界面（噪音，不是补救线索）。
// 出处：SessionHost 影响审查 2026-10-13——工作树确认时事实已漂移，toast 必须写出
// worktree_binding_drift（D8），否则负责人分不清「没执行」与「执行失败」。
import { asCmdError, errText } from './api'
import i18n from './i18n'
import { useUiStore, type ToastOpts } from './store'

const GENERIC = new Set(['unknown', 'internal', 'invalid_input'])

/** 可展示的原因码：已在 errors.* 登记、且不是兜底类。 */
export function reasonCode(e: unknown): string | undefined {
  const { code } = asCmdError(e)
  if (!code || GENERIC.has(code)) return undefined
  return i18n.exists(`errors.${code}`) ? code : undefined
}

/** 失败 toast：circleX 红 + 原因（+ 码）+ 可选补救动作；渲染层给 role=alert。
 *  wrap = 外层文案 key（如 errors.actionFailed，参数 detail）。 */
export function toastError(e: unknown, opts: Omit<ToastOpts, 'code'> & { wrap?: string } = {}) {
  const detail = errText(e)
  if (!detail) return // 本地取消的导航不是错误（同 pushToast 的空文案约定）
  const { wrap, ...rest } = opts
  const text = wrap ? i18n.t(wrap, { detail }) : detail
  useUiStore.getState().pushToast(text, 'err', { ...rest, code: reasonCode(e) })
}
