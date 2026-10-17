import { useCallback, useEffect, useRef, useState } from 'react'
import { errText } from './api'
import { useUiStore } from './store'

/// 「已复制」回执停留时长（代码块 / 执行卡 / 消息复制共用）。
export const COPY_FLASH_MS = 1500

/// 复制到剪贴板 + 短暂「已复制」回执（ui-redesign review S5，2026-10-17 从
/// Md 代码块、ExecCard、时间线消息 3 处抽出）。失败 toast；再次复制重新计时；
/// 卸载清计时器。
export function useCopyFlash() {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(null)
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current) }, [])
  const copy = useCallback(async (text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), COPY_FLASH_MS)
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    }
  }, [])
  return { copied, copy }
}
