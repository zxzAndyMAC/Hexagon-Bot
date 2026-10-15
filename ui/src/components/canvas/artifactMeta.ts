// 产物视图与产物清单共用的取值（components D13，canvas#artifact）。纯函数，组件文件只导出组件。
import type { ArtifactRow, TimelineItem } from '../../api'

export type BadgeTone = 'neutral' | 'ok' | 'warn' | 'err' | 'info' | 'line'

/** 产物状态胶囊（A4）：待恢复 / 待定 = warn，有效 = ok，已盖章 = info（accent），被取代 = line。 */
export function artifactTone(a: Pick<ArtifactRow, 'status' | 'materialization'>): BadgeTone {
  if (a.materialization === 'pending_recovery' || a.status === 'pending') return 'warn'
  if (a.status === 'stamped') return 'info'
  if (a.status === 'valid') return 'ok'
  return 'line'
}

/**
 * 交接说明（CONTEXT「产物」：交付时附一段，语境、坑、为什么）。
 * 先找该版本的 artifact_delivered 事件（payload.handoff），找不到再读正文元数据头的 `handoff:` 行
 * ——时间线只加载了最近一窗，旧版本的交付事件可能不在内存里。
 */
export function handoffNote(timeline: TimelineItem[], path: string, version: number, text: string | null): string | null {
  for (let i = timeline.length - 1; i >= 0; i--) {
    const ev = timeline[i].event
    if (ev.kind !== 'artifact_delivered') continue
    const p = ev.payload as Record<string, unknown>
    if (p.path === path && Number(p.version) === version && typeof p.handoff === 'string' && p.handoff.trim()) return p.handoff.trim()
  }
  const head = text ? /^---\n([\s\S]*?)\n---/.exec(text) : null
  const line = head?.[1].split('\n').find((l) => /^handoff\s*:/.test(l))
  const note = line?.replace(/^handoff\s*:\s*/, '').trim()
  return note || null
}
