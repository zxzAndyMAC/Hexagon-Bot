// components §0「禁用」：禁用控件仍可聚焦以读出原因，所以不用原生 disabled（它把控件移出 Tab 序列，
// tooltip 也触发不了），改为 aria-disabled + 在点击处拦截。Radix 原语用 composeEventHandlers 合并
// 外部 onClick：外部先跑、defaultPrevented 后内部切换逻辑整个跳过，Space 键也走 click，一处拦全。
import type * as React from 'react'

export function guardInert<E extends React.SyntheticEvent>(inert: boolean | undefined, handler?: (e: E) => void) {
  return (e: E) => {
    if (inert) {
      e.preventDefault()
      return
    }
    handler?.(e)
  }
}

/** 提交中（busy）模态：Esc 与点遮罩都不关（components A17「提交中 Esc 无效」）。 */
export function guardBusy<E extends Event>(busy: boolean, handler?: (e: E) => void) {
  return (e: E) => {
    if (busy) e.preventDefault()
    handler?.(e)
  }
}
