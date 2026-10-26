// 「打开方式」（owner-followups-1015 #31，components A16b）：右键子菜单、命令面板、keymap 三个入口共用。
// 路径一律项目相对（空串 = 项目根）；例外：启动页最近项目（#32）传 recents 记录里的绝对目录，走 recent_* 命令。
// 白名单、系统登记、根约束全在 core 判，这里只调 IPC、出 toast。
import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import type { OpenWithApp } from './gen/OpenWithApp'
import { toastError } from './toast'

export type { OpenWithApp }

/** 候选读取状态：读取中 / 读到（可能为空）/ 失败。 */
export type OpenWithState = { kind: 'loading' } | { kind: 'ready'; apps: OpenWithApp[] } | { kind: 'error' }

const LOADING: OpenWithState = { kind: 'loading' }

/**
 * 挂载即读（菜单内容只在打开时挂载 = 右键那一刻预取，子菜单展开时多半已到）；`retry` 重读。null = 不读。
 * 结果按请求键（路径 + 第几轮）存：键变了（换目标 / 重试）在渲染时就退回读取中，不在 effect 里同步 setState。
 */
export function useOpenWithCandidates(path: string | null, recent = false): { state: OpenWithState; retry: () => void } {
  const [round, setRound] = useState(0)
  const key = path == null ? null : `${round}:${recent ? 'recent' : 'project'}:${path}`
  const [result, setResult] = useState<{ key: string; state: OpenWithState } | null>(null)
  useEffect(() => {
    if (key == null || path == null) return
    let live = true
    ;(recent ? api.recentOpenWithCandidates(path) : api.openWithCandidates(path)).then(
      (apps) => { if (live) setResult({ key, state: { kind: 'ready', apps } }) },
      () => { if (live) setResult({ key, state: { kind: 'error' } }) },
    )
    return () => { live = false }
  }, [key, path, recent])
  const retry = useCallback(() => setRound((n) => n + 1), [])
  return { state: result && result.key === key ? result.state : LOADING, retry }
}

/** 用指定应用打开；失败走 toastError（A16b 错误态：菜单已关）。`recent` = 启动页最近项目（`path` 是记录里的目录，#32）。 */
export async function openPathWith(path: string, appId: string, recent = false) {
  try {
    await (recent ? api.recentOpenWith(path, appId) : api.openWith(path, appId))
  } catch (e) {
    toastError(e)
  }
}
