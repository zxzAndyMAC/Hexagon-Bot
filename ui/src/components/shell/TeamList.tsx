// 团队成员行 MemberRow（components B3，workbench#running）与侧栏「团队」分组。
// 34 高：24 头像（含状态点）+ 名 13px + 右侧状态：工作中 = 像素格；休眠 = 「休眠」11 ink-3 且整行 opacity .5。
// 点击打开该 Agent（票 02 仍在中栏页签打开，03 改在画布打开）；当前打开的成员 --hover-2 底。
// 悬停名片（B3 写 HoverCard）暂用 A15 Tooltip 显示「角色 · 模型」——基础件里还没有 HoverCard，对账表票 02 记录。
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import type { TeamRow } from '../../gen/TeamRow'
import { factOpenTurns } from '../../timelineFacts'
import { openTurns } from '../../timelineModel'
import { slotLabel } from '../../modelpick'
import { CreateRoleForm } from '../RoleEditor'
import { Avatar } from '@/components/ui/avatar'
import { PixelGrid } from '@/components/ui/spinner'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

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

function MemberRow({ m, working }: { m: TeamRow; working: boolean }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const providers = useUiStore((s) => s.providers)
  const src = useUiStore((s) => s.avatars[m.id])
  const current = useUiStore((s) => s.activeTab === `agent:${m.id}`)
  const sleeping = m.status === 'sleeping'
  return (
    <li>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            data-member={m.id}
            data-member-state={working ? 'working' : sleeping ? 'sleeping' : 'online'}
            aria-current={current || undefined}
            onClick={() => openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role })}
            className={cn(
              'hx-press hx-focus mx-2.5 flex h-[34px] w-[calc(100%-20px)] cursor-pointer items-center gap-[9px] rounded-[var(--r-sm)] px-2 text-left text-[13px] text-ink live:hover:bg-hover',
              sleeping && 'opacity-50',
              current && 'bg-hover-2',
            )}
          >
            <Avatar agentId={m.id} name={m.role} size={24} src={src} status={sleeping ? 'sleep' : 'online'} />
            <span className="min-w-0 truncate">{m.role}</span>
            <span className="ml-auto flex shrink-0 items-center text-[11px] text-ink-3">
              {working ? <PixelGrid label={t('sidebar.working')} /> : sleeping ? t('sidebar.sleeping') : null}
            </span>
          </button>
        </TooltipTrigger>
        <TooltipContent side="right">{m.role} · {slotLabel(m.model_slot, providers, t('agent.dedicatedTag'))}</TooltipContent>
      </Tooltip>
    </li>
  )
}

export function TeamList() {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const working = useWorkingAgents()
  return (
    <ul aria-label={t('sidebar.team')} className="flex flex-col">
      {team.length === 0 && <li className="px-[18px] py-1 text-[12px] text-ink-3">{t('sidebar.noMembers')}</li>}
      {team.map((m) => <MemberRow key={m.id} m={m} working={working.has(m.id) && m.status !== 'sleeping'} />)}
    </ul>
  )
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
