// 轨迹导出（US54）：头部 ⋯ 菜单「导出轨迹」用（components B5，票 02）。
// 与时间线筛选条上的导出钮同口径：Tauri 走保存对话框 + export_events，浏览器 dev 合成 Blob 下载。
// 时间线那份带当前筛选档（Timeline.tsx doExport，票 05 改时间线时可改调本函数）；这里导出全量。
import { api, isTauri } from './api'

export const TRACE_FILE = 'hexagon-trace.json'

/** 导出轨迹；用户取消保存对话框返回 null，否则返回条数。 */
export async function exportTrace(kinds?: string[]): Promise<number | null> {
  if (isTauri) {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const path = await save({ defaultPath: TRACE_FILE, filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (!path) return null
    return api.exportEvents({ path, kinds })
  }
  const items = await api.exportTimelineItems(kinds)
  const doc = { format: 'hexagon-trace-export', version: 1, count: items.length, events: items.map((i) => i.event) }
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }))
  a.download = TRACE_FILE
  a.click()
  URL.revokeObjectURL(a.href)
  return items.length
}
