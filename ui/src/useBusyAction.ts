import { useCallback, useState } from 'react'
import { errText } from './api'
import { useUiStore } from './store'

/// 写操作的忙态包装（ui-redesign review S5，2026-10-17 从设置页 6 处同形
/// `setBusy(true); try … catch toast … finally setBusy(false)` 抽出，行为不变）：
/// 跑期间 `busy`，失败统一 `pushToast(errText(e), 'err')`，收尾复位。
/// 不做防重——与抽出前一致，调用方按钮以 `disabled={busy}` 挡连点；需要
/// ref 防重 / 作用域拦截 / 收尾 invalidate 的裁决类动作不走这里。
export function useBusyAction() {
  const [busy, setBusy] = useState(false)
  const run = useCallback(async (action: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await action()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }, [])
  return { busy, run }
}
