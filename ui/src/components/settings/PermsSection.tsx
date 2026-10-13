// 设置「权限」分区（ui-redesign 票 11；settings#perms，components E5，interaction §8「设置 · 权限」）。
// 顶部「暂停电脑操作」；电脑操作 / 项目浏览器两行承载票 14 的面板（DesktopControlPanel / BrowserControlPanel
// 内部不动，只换宿主行）；托管状态块只显示状态与报告，不放开关（开关在输入条批准模式菜单，ADR 0086 对齐）；
// 「已记住的规则」A21 表格，可筛选、逐条撤销（ui-audit-2 票 03：记住的规则曾无处查看、无法撤销——安全缺口）。
// 撤销成功的语义：同形状请求重新弹待决卡（后端测试钉了）；确认层暂用 askConfirm，票 13 换 AlertDialog。
// 批准模式选择器不再放在这里：interaction §8 / 规格 story 39「放权多少只在一个地方设置」（输入条左下）。
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PermissionRuleRow } from '../../api'
import { useUiStore } from '../../store'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { Icon } from '../Icon'
import { BrowserControlPanel } from '../BrowserControlPanel'
import { DesktopControlPanel } from '../DesktopControlPanel'
import { DesktopPauseButton } from '../DesktopPauseButton'
import { DesktopPermissions } from '../DesktopPermissions'
import { PermissionScope } from '../PermissionActions'
import { ProjectDelegation } from '../ProjectDelegation'
import { Empty, Group, InlineError, NeedsProject, SetHead, SetWrap, SkeletonRows, SRow, tableCls } from './kit'

/** 边界胶囊（settings#perms notes）：联网 warn + globe、禁止联网 / 后台 / 仅前台 / 命名会话 neutral。 */
function Boundaries({ r }: { r: PermissionRuleRow }) {
  const { t } = useTranslation()
  return (
    <span className="flex flex-wrap gap-1">
      {r.effect === 'allow' && (
        <Badge>{t(r.project_shared ? 'projectPermission.sharedLabel' : 'projectPermission.legacy')}</Badge>
      )}
      {r.effect === 'deny' && <Badge variant="err">{t('perms.effect_deny')}</Badge>}
      {r.tool === 'bash' && (
        <>
          {r.network_allowed
            ? <Badge variant="warn"><Icon name="web" size={12} />{t('perms.networkAllowed')}</Badge>
            : <Badge>{t('perms.networkDisabled')}</Badge>}
          <Badge>{t(r.background_allowed ? 'perms.backgroundAllowed' : 'perms.foregroundOnly')}</Badge>
          <Badge>{r.session_name ? t('perms.namedSession', { name: r.session_name }) : t('perms.noNamedSession')}</Badge>
        </>
      )}
      {r.scope === 'activation' && <Badge>{t('perms.scope_activation')}</Badge>}
    </span>
  )
}

function RulesTable() {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const askConfirm = useUiStore((s) => s.askConfirm)
  const team = useUiStore((s) => s.team)
  const [rules, setRules] = useState<PermissionRuleRow[] | null>(null)
  const [error, setError] = useState('')
  const [q, setQ] = useState('')

  const load = useCallback(async () => {
    setError('')
    try { setRules(await api.permissionRules()) } catch (e) { setError(errText(e)) }
  }, [])
  useEffect(() => { void load() }, [load])

  const roleOf = (r: PermissionRuleRow) => team.find((m) => m.id === r.agent_id)?.role ?? r.agent_id
  const revoke = (r: PermissionRuleRow) =>
    askConfirm({
      title: t('perms.revokeTitle'),
      body: `${r.tool} · ${r.shape}`,
      danger: true,
      confirmLabel: t('perms.revoke'),
      run: async () => {
        await api.revokePermissionRule(r.id).catch((e) => pushToast(errText(e), 'err'))
        await load()
      },
    })

  const shown = (rules ?? []).filter((r) => !q || `${roleOf(r) ?? ''} ${r.tool} ${r.shape}`.toLowerCase().includes(q.toLowerCase()))
  return (
    <section aria-label={t('settingsUi.rememberedRules')}>
      <div className="mb-2 flex items-center gap-3">
        <span className="ml-1 min-w-0 flex-1 text-[12.5px] font-medium text-ink-2">
          {t('settingsUi.rememberedRulesN', { count: rules?.length ?? 0 })}
        </span>
        <Input className="h-[30px] w-[220px]" leading={<Icon name="filter" size={14} />}
          placeholder={t('settingsUi.filterRules')} aria-label={t('settingsUi.filterRules')}
          value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <div className="rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
        {error ? (
          <InlineError text={error} onRetry={() => void load()} />
        ) : rules === null ? (
          <SkeletonRows />
        ) : rules.length === 0 ? (
          <Empty icon="shield" title={t('settingsUi.noRules')} desc={t('perms.empty')} />
        ) : (
          <table className={tableCls.table}>
            <thead>
              <tr>
                <th className={tableCls.th}>{t('settingsUi.colRole')}</th>
                <th className={tableCls.th}>{t('settingsUi.colTool')}</th>
                <th className={tableCls.th}>{t('settingsUi.colScope')}</th>
                <th className={tableCls.th}>{t('settingsUi.colBoundary')}</th>
                <th className={tableCls.th}>{t('settingsUi.colTime')}</th>
                <th className={tableCls.th}><span className="sr-only">{t('perms.revoke')}</span></th>
              </tr>
            </thead>
            <tbody>
              {shown.map((r) => {
                const role = roleOf(r)
                return (
                  <tr key={r.id} data-rule={r.id} className={tableCls.tr}>
                    <td className={cn(tableCls.td, 'whitespace-nowrap')}>
                      {role ? (
                        <span className="inline-flex items-center gap-1.5">
                          <Avatar agentId={r.agent_id ?? role} name={role} size={18} />{role}
                        </span>
                      ) : <span className="text-ink-3">{t('settingsUi.allRoles')}</span>}
                    </td>
                    <td className={cn(tableCls.td, 'font-mono text-[12px]')}>{r.tool}</td>
                    <td className={cn(tableCls.td, 'max-w-[220px] font-mono text-[12px] break-all')} title={r.shape}>
                      {r.project_shared ? <PermissionScope tool={r.tool} shape={r.shape} generalized={!r.shape.startsWith('exact:')} /> : r.shape}
                    </td>
                    <td className={tableCls.td}><Boundaries r={r} /></td>
                    <td className={cn(tableCls.td, 'whitespace-nowrap text-[12px] text-ink-3')}>{r.created_at.slice(0, 10)}</td>
                    <td className={cn(tableCls.td, 'text-right')}>
                      <Button size="xs" variant="ghost" onClick={() => revoke(r)}>{t('perms.revoke')}</Button>
                    </td>
                  </tr>
                )
              })}
              {shown.length === 0 && (
                <tr><td colSpan={6} className={cn(tableCls.td, 'text-center text-[12.5px] text-ink-3')}>{t('settingsUi.noMatches')}</td></tr>
              )}
            </tbody>
          </table>
        )}
      </div>
      <p className="mt-2 px-1 text-[12px] leading-[1.6] text-ink-3">{t('perms.intro')} {t('perms.grantsHint')}</p>
    </section>
  )
}

export function PermsSection({ projectless, onOpenProject }: { projectless: boolean; onOpenProject?: () => void }) {
  const { t } = useTranslation()
  return (
    <SetWrap wide>
      <SetHead
        title={t('settings.nav_perms')}
        desc={t('settingsUi.permsDesc')}
        actions={!projectless && <DesktopPauseButton />}
      />
      {projectless ? (
        <>
          <Group><SRow icon="computer" label={t('settingsUi.computerRow')} body={<DesktopPermissions />} /></Group>
          <NeedsProject onOpen={onOpenProject} />
        </>
      ) : (
        <>
          <Group>
            <SRow icon="computer" label={t('settingsUi.computerRow')} hint={t('settingsUi.computerRowHint')} body={<DesktopControlPanel inline />} />
            <SRow icon="web" label={t('browser.title')} hint={t('settingsUi.browserRowHint')} body={<BrowserControlPanel inline />} />
          </Group>
          <ProjectDelegation />
          <RulesTable />
        </>
      )}
    </SetWrap>
  )
}
