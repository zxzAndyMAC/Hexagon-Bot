// 设置「日志」分区（diagnostic-records 票 01；ui-redesign 票 11 换皮：settings#logs，components E8 / A21）。
// 诊断开关在这里（通用页不放）；A7 四类筛选 + 刷新；A21 表格（时间等宽 / 分类 chip / 分支 · 原因码 / 耗时）；
// 行可展开看项目 / Agent / 激活 / 轨迹 id；脚注「记录不离开本机」。只有表体滚动，标题与筛选钉住。
// projectless：只出宿主记录；选中非宿主分类时提示先进入一个项目——这类记录按项目归，提示比空列表诚实。
import { Fragment, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type DiagRecord } from '../../api'
import { localLogTime } from '../../diagTime'
import { Badge } from '@/components/ui/badge'
import { Dot } from '@/components/ui/dot'
import { IconButton } from '@/components/ui/icon-button'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import { Icon } from '../Icon'
import { cn } from '@/lib/utils'
import { Empty } from './kit'
import { tableCls } from './styles'

/// 诊断分类四档（词表定名，core diag.rs 同源）。显示走 i18n，
/// 发往 core 的 class 参数是字面量本身——记录里存的就是这四个词。
const DIAG_CLASSES = ['判定', '拒绝', '槽位', '宿主'] as const
const diagClassKey = (c: string): string =>
  c === '判定' ? 'judge' : c === '拒绝' ? 'reject' : c === '槽位' ? 'slot' : 'host'
const ALL = 'all'

/// 行身份：全字段合成。ts 只到秒，同秒同分支的两条会在合成里仍撞上
/// trace/activation；真正全同的两行内容可互换，共享展开态无害。
const rowKey = (r: DiagRecord): string =>
  `${r.ts}|${r.class}|${r.branch}|${r.code}|${r.project ?? ''}|${r.agent ?? ''}|${r.activation ?? ''}|${r.trace ?? ''}|${r.ms}`

/** 分类 chip（诊断记录约定）：拒绝 err、槽位上的失败 warn「槽位失败」、其余 neutral。 */
function ClassChip({ r }: { r: DiagRecord }) {
  const { t } = useTranslation()
  if (r.class === '拒绝') return <Badge variant="err">{t('settings.logs_class_reject')}</Badge>
  if (r.class === '槽位' && r.level === 'warn') return <Badge variant="warn">{t('settingsUi.logsSlotFailed')}</Badge>
  return <Badge variant={r.level === 'warn' ? 'warn' : 'neutral'}>{t(`settings.logs_class_${diagClassKey(r.class)}`)}</Badge>
}

export function LogsSection({ projectless = false }: { projectless?: boolean }) {
  const { t } = useTranslation()
  const [logOn, setLogOn] = useState(true)
  const [cls, setCls] = useState<string | null>(null)
  const [rows, setRows] = useState<DiagRecord[]>([])
  const [open, setOpen] = useState<string | null>(null)
  // core 项目 id 恒为 PROJECT_ID('p1')；无项目上下文传 null → 只剩宿主。
  const project = projectless ? null : 'p1'

  const refresh = useCallback(() => {
    api.diagnosticRecords(project, cls).then(setRows).catch(() => setRows([]))
  }, [project, cls])

  useEffect(() => { api.logEnabled().then(setLogOn).catch(() => {}) }, [])
  useEffect(() => { refresh() }, [refresh])

  const needProject = projectless && cls !== null && cls !== '宿主'
  const lang = document.documentElement.lang || navigator.language

  return (
    <div data-logs className="mx-auto flex h-full w-full max-w-[860px] min-h-0 flex-col px-8 pt-9 pb-6">
      <div className="mb-1 flex flex-none items-center gap-3">
        <h2 className="min-w-0 flex-1 text-[20px] font-semibold tracking-[-.01em] text-ink">{t('settings.nav_logs')}</h2>
      </div>
      <div className="mb-4 flex flex-none items-center gap-3.5 rounded-[12px] bg-[var(--app-card)] px-4 py-[13px] shadow-[var(--shadow-card)]">
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-medium text-ink">{t('settings.logging')}</div>
          <div className="text-[12.5px] text-ink-3">{t('settings.loggingHint')}</div>
        </div>
        <Switch
          aria-label={t('settings.logging')}
          checked={logOn}
          onCheckedChange={async (on) => {
            setLogOn(on)
            await api.setLogEnabled(on).catch(() => {})
            refresh()
          }}
        />
      </div>
      <div className="mb-3 flex flex-none items-center gap-1.5">
        <Segmented aria-label={t('settingsUi.logsFilter')} value={cls ?? ALL} onValueChange={(v) => { setCls(v === ALL ? null : v); setOpen(null) }}>
          <SegmentedItem value={ALL}>
            {t('settings.logs_all')}
            {cls === null && <span className="text-ink-3 tabular-nums">{rows.length}</span>}
          </SegmentedItem>
          {DIAG_CLASSES.map((c) => (
            <SegmentedItem key={c} value={c}>
              {c === '判定' && <Dot tone="info" />}
              {c === '拒绝' && <Dot tone="err" />}
              {t(`settings.logs_class_${diagClassKey(c)}`)}
            </SegmentedItem>
          ))}
        </Segmented>
        <span className="flex-1" />
        <IconButton size="sm" label={t('settings.logs_refresh')} onClick={refresh}>
          <Icon name="refresh" size={12} />
        </IconButton>
      </div>
      <div data-logs-scroll className="min-h-0 flex-1 overflow-y-auto rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
        {needProject ? (
          <Empty icon="lock" title={t('settingsUi.needsProjectTitle')} desc={t('settings.logs_needProject')} />
        ) : rows.length === 0 ? (
          <Empty icon="logs" title={t('settings.logs_empty')} desc={t('settingsUi.logsEmptyHint')}
            action={<Button size="sm" onClick={refresh}>{t('settings.logs_refresh')}</Button>} />
        ) : (
          <table className={tableCls.table}>
            <thead className="sticky top-0 bg-[var(--app-card)]">
              <tr>
                <th className={tableCls.th}>{t('settingsUi.colTime')}</th>
                <th className={tableCls.th}>{t('settingsUi.colClass')}</th>
                <th className={tableCls.th}>{t('settingsUi.colBranch')}</th>
                <th className={cn(tableCls.th, 'text-right')}>{t('settingsUi.colMs')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                // 行身份 = 全字段合成，不用序号——刷新后新记录顶到前面时，序号身份会把展开态挂到别行头上。
                const k = rowKey(r)
                const expanded = open === k
                return (
                  <Fragment key={k}>
                    <tr className={tableCls.tr}>
                      <td className={cn(tableCls.td, 'w-[88px] font-mono text-[12px] text-ink-2')}>{localLogTime(r.ts, lang)}</td>
                      <td className={cn(tableCls.td, 'w-[110px]')}><ClassChip r={r} /></td>
                      <td className={cn(tableCls.td, 'max-w-0')}>
                        <button type="button" aria-expanded={expanded} onClick={() => setOpen(expanded ? null : k)}
                          className="hx-focus flex w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[6px] text-left font-mono text-[12px] text-ink">
                          <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} className="flex-none text-ink-3" />
                          <span className="truncate">{r.branch} · {r.code}</span>
                        </button>
                      </td>
                      <td className={cn(tableCls.td, 'w-[64px] text-right font-mono text-[12px] text-ink-2 tabular-nums')}>{r.ms}ms</td>
                    </tr>
                    {expanded && (
                      <tr>
                        <td colSpan={4} className={cn(tableCls.td, 'bg-inset font-mono text-[12px] leading-[1.8] text-ink-3')}>
                          <div>{r.ts}</div>
                          <div>{t('settings.logs_f_project')}: {r.project ?? '—'}</div>
                          <div>{t('settings.logs_f_agent')}: {r.agent ?? '—'}</div>
                          <div>{t('settings.logs_f_activation')}: {r.activation ?? '—'}</div>
                          <div>{t('settings.logs_f_trace')}: {r.trace ?? '—'}</div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                )
              })}
            </tbody>
          </table>
        )}
      </div>
      <div className="mt-2 flex-none text-[12px] text-ink-3">{t('settingsUi.logsLocal')}</div>
    </div>
  )
}
