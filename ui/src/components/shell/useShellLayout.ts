// 外壳布局 hook（票 03）：窗口宽度 + store 布局切片 → layout.ts resolveShell 的结果。
// 取值规则全在 layout.ts（纯函数，有单测）；这里只订阅窗口 resize。
import { useEffect, useState } from 'react'
import { useUiStore } from '../../store'
import { resolveShell, type ShellLayout } from '../../layout'

export function useWindowWidth() {
  const [w, setW] = useState(() => window.innerWidth)
  useEffect(() => {
    const on = () => setW(window.innerWidth)
    window.addEventListener('resize', on)
    return () => window.removeEventListener('resize', on)
  }, [])
  return w
}

export function useShellLayout(): ShellLayout {
  const windowWidth = useWindowWidth()
  const side = useUiStore((s) => s.sideLayout)
  const sideHidden = useUiStore((s) => s.focusSnapshot != null)
  const canvasOpen = useUiStore((s) => s.canvasOpen)
  const canvas = useUiStore((s) => s.canvasLayout)
  const usage = useUiStore((s) => s.usageView)
  return resolveShell({ windowWidth, side, sideHidden, canvasOpen, canvas, usage })
}
