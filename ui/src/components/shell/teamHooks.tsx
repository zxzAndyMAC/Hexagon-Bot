// 侧栏团队的两个 hook（票 02）：工作中成员集合、「+ 添加角色」内联表单。与组件分文件，保 Fast Refresh。
import { useMemo, useState } from 'react'
import { useUiStore } from '../../store'
import { factOpenTurns } from '../../timelineFacts'
import { openTurns } from '../../timelineModel'
import { CreateRoleForm } from '../RoleEditor'

/**
 * 正在工作的成员 id：有未收束回合（宿主 facts；无 facts 的 dev 回退时间线扫描），
 * 或流式缓冲里已有输出 / 思考 / 计划。与时间线 StreamFooter 的「在说话」口径一致。
 */
export function useWorkingAgents(): ReadonlySet<string> {
  const facts = useUiStore((s) => s.timelineFacts)
  const projectRoot = useUiStore((s) => s.projectRoot)
  const timeline = useUiStore((s) => s.timeline)
  const caughtUp = useUiStore((s) => s.timelineCaughtUp)
  const streams = useUiStore((s) => s.streams)
  const thinkings = useUiStore((s) => s.thinkings)
  const plans = useUiStore((s) => s.plans)
  return useMemo(() => {
    const ids = new Set<string>()
    const open = facts || projectRoot ? factOpenTurns(facts) : openTurns(timeline, caughtUp)
    for (const id of open.keys()) if (id) ids.add(id)
    const speaking = (m: Record<string, Record<number, string>>) => {
      for (const [id, parts] of Object.entries(m)) if (Object.values(parts).some((x) => x.length > 0)) ids.add(id)
    }
    speaking(streams)
    speaking(thinkings)
    for (const [id, plan] of Object.entries(plans)) if (plan) ids.add(id)
    return ids
  }, [facts, projectRoot, timeline, caughtUp, streams, thinkings, plans])
}

/** 「+ 添加角色」：沿用旧右栏团队页的内联创建表单（RoleEditor，向导与设置共用的 E4 换皮在票 10/11）。 */
export function useAddRole() {
  const [creating, setCreating] = useState(false)
  const invalidate = useUiStore((s) => s.invalidate)
  const form = creating ? (
    <div className="px-2.5 pb-2">
      <CreateRoleForm onDone={() => { setCreating(false); invalidate('team') }} />
    </div>
  ) : null
  return { creating, toggle: () => setCreating((v) => !v), form }
}
