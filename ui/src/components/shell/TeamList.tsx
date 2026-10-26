// 团队成员行 MemberRow（components B3，workbench#running）与侧栏「团队」分组。
// 34 高：24 头像（含状态点）+ 名 13px + 右侧状态：工作中 = 像素格；休眠 = 「休眠」11 ink-3，只头像半透明去饱和
// （owner-followups-1015 票 09 撤整行 opacity .5：名字与「休眠」字跟着掉到 AA 以下，foundations「对比度」）。
// 点击打开该 Agent（票 02 仍在中栏页签打开，03 改在画布打开）；当前打开的成员 --hover-2 底。
// 悬停名片 HoverCard（B3；gap B6 2026-10-15 起替代票 02 的过渡 Tooltip「角色 · 模型」）：
// 头像 + 角色 + 状态，下列 模型 / 上级 / 职责（TeamRow.model / superior / duty）。聚焦同样打开。
// 右键菜单（A16b 第一批「成员行」，owner-followups-1015 #08）：打开详情 · @ 点名 · 复制名字；右键不打开该成员，
// 菜单开着时悬停名片收起（两层浮层叠在行旁边会互相遮挡）。
// #18：分隔后 换头像 · 恢复默认（角色层，同角色详情）；角色没有自定义头像时「恢复默认」禁用写原因（A16b「不可用项」），
// 有没有要等菜单打开时问一次核（成员行只知道生效图，分不出角色图与实例覆盖）。
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import type { TeamRow } from '../../gen/TeamRow'
import { slotLabel } from '../../modelpick'
import { useWorkingAgents } from './teamHooks'
import { Avatar } from '@/components/ui/avatar'
import { PixelGrid } from '@/components/ui/spinner'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from '@/components/ui/context-menu'
import { cn } from '@/lib/utils'
import { isMac } from '../../keymap'
import { changeMemberAvatar, copyMemberName, memberHasRoleAvatar, mentionMember, openMemberDetail, restoreMemberAvatar } from '../../memberActions'

function MemberRow({ m, working }: { m: TeamRow; working: boolean }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const providers = useUiStore((s) => s.providers)
  const src = useUiStore((s) => s.avatars[m.id])
  const current = useUiStore((s) => s.activeTab === `agent:${m.id}`)
  const sleeping = m.status === 'sleeping'
  const [hover, setHover] = useState(false)
  const [menu, setMenu] = useState(false)
  // 角色有无自定义头像：菜单打开时问核；问到之前按「没有」（禁用比误删便宜）。
  const [roleAvatar, setRoleAvatar] = useState(false)
  const onMenu = (open: boolean) => {
    setMenu(open)
    if (!open) return
    setRoleAvatar(false)
    void memberHasRoleAvatar(m.id).then(setRoleAvatar)
  }
  // 「@ 点名」把焦点送去输入条：菜单关闭时不还焦点给成员行。
  const skipRestore = useRef(false)
  return (
    <li>
      <ContextMenu onOpenChange={onMenu}>
      <HoverCard open={hover && !menu} onOpenChange={setHover}>
        <ContextMenuTrigger asChild>
        <HoverCardTrigger asChild>
          <button
            type="button"
            data-member={m.id}
            data-member-state={working ? 'working' : sleeping ? 'sleeping' : 'online'}
            aria-current={current || undefined}
            // A16b：macOS ctrl+点击由系统当右键（发 contextmenu 开菜单），不能顺带打开该成员（「右键不改选中」）。
            onClick={(e) => { if (!(isMac && e.ctrlKey)) openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role }) }}
            className={cn(
              'hx-press hx-focus mx-2.5 flex h-[34px] w-[calc(100%-20px)] cursor-pointer items-center gap-[9px] rounded-[var(--r-sm)] px-2 text-left text-[13px] text-ink live:hover:bg-hover',
              current && 'bg-hover-2',
            )}
          >
            <Avatar agentId={m.id} name={m.role} size={24} src={src} status={sleeping ? 'sleep' : 'online'} dimmed={sleeping} />
            <span className="min-w-0 truncate">{m.role}</span>
            <span className="ml-auto flex shrink-0 items-center text-[11px] text-ink-3">
              {working ? <PixelGrid label={t('sidebar.working')} /> : sleeping ? t('sidebar.sleeping') : null}
            </span>
          </button>
        </HoverCardTrigger>
        </ContextMenuTrigger>
        <HoverCardContent side="right" align="start" data-member-card={m.id} className="w-[260px] px-3 py-2.5">
          <div className="flex items-center gap-2">
            <Avatar agentId={m.id} name={m.role} size={24} src={src} status={sleeping ? 'sleep' : 'online'} />
            <b className="min-w-0 truncate text-[13px] font-semibold">{m.role}</b>
            <span className="ml-auto shrink-0 text-[11px] text-ink-3">
              {working ? t('sidebar.working') : sleeping ? t('sidebar.sleeping') : t('sidebar.memberOnline')}
            </span>
          </div>
          <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]">
            <dt className="text-ink-3">{t('sidebar.memberModel')}</dt>
            <dd className="min-w-0 truncate">{m.model ?? slotLabel(m.model_slot, providers, t('agent.dedicatedTag'))}</dd>
            <dt className="text-ink-3">{t('sidebar.memberSuperior')}</dt>
            <dd className="min-w-0 truncate">{m.superior ?? t('sidebar.memberNoSuperior')}</dd>
            {m.duty && <>
              <dt className="text-ink-3">{t('sidebar.memberDuty')}</dt>
              <dd className="min-w-0 text-ink-2">{m.duty}</dd>
            </>}
          </dl>
        </HoverCardContent>
      </HoverCard>
      <ContextMenuContent data-member-menu={m.id}
        onCloseAutoFocus={(e) => { if (skipRestore.current) { skipRestore.current = false; e.preventDefault() } }}>
        <ContextMenuItem data-ctx-item="openDetail" action="openMemberDetail" onSelect={() => openMemberDetail(m.id)}>{t('ctxMember.openDetail')}</ContextMenuItem>
        <ContextMenuItem data-ctx-item="mention" action="mentionMember"
          onSelect={() => { skipRestore.current = true; mentionMember(m.id) }}>{t('ctxMember.mention')}</ContextMenuItem>
        <ContextMenuItem data-ctx-item="copyName" action="copyMemberName" onSelect={() => void copyMemberName(m.id)}>
          {t('ctxMember.copyName')}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem data-ctx-item="changeAvatar" action="changeMemberAvatar" onSelect={() => changeMemberAvatar(m.id)}>
          {t('ctxMember.changeAvatar')}
        </ContextMenuItem>
        <ContextMenuItem data-ctx-item="restoreAvatar" action="restoreMemberAvatar"
          reason={roleAvatar ? undefined : t('ctxMember.noRoleAvatar')} onSelect={() => void restoreMemberAvatar(m.id)}>
          {t('ctxMember.restoreAvatar')}
        </ContextMenuItem>
      </ContextMenuContent>
      </ContextMenu>
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
