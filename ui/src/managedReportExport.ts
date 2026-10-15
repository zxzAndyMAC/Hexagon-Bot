// gap A4（2026-10-15，managed#report「导出」）：托管报告导出为 Markdown。
// 正文是界面文案（i18n，按当前语言），所以在 UI 侧生成；落盘走 core `export_managed_report`
// （只收 .md，路径来自保存对话框，与轨迹导出同口径）。浏览器 dev 合成 Blob 下载。
import { api, isTauri } from './api'
import type { ExternalOperation } from './gen/ExternalOperation'
import type { ManagedRecord } from './gen/ManagedRecord'
import type { ManagedSummary } from './gen/ManagedSummary'
import { fmtYuan } from './usage'

export const MANAGED_REPORT_FILE = 'hexagon-managed-report.md'

type T = (key: string, opts?: Record<string, unknown>) => string
export type ReportDoc = {
  title: string
  period: string | null
  records: ManagedRecord[]
  external: ExternalOperation[]
  summary: ManagedSummary | null
  recordTitle: (record: ManagedRecord) => string
}

const cell = (text: string) => text.replace(/\|/g, '\\|').replace(/\n/g, ' ')

export function managedReportMarkdown(t: T, doc: ReportDoc): string {
  const unfinished = doc.records.filter(r => r.result !== 'done')
  const done = doc.records.filter(r => r.result === 'done')
  const lines = [`# ${doc.title}`, '']
  if (doc.period) lines.push(doc.period, '')
  lines.push(`| ${t('delegation.kpiDecisions')} | ${t('delegation.kpiDone')} | ${t('delegation.kpiIncomplete')} | ${t('delegation.kpiSpend')} |`,
    '| --- | --- | --- | --- |',
    `| ${doc.records.length} | ${done.length} | ${unfinished.length} | ${doc.summary ? spendText(t, doc.summary) : '—'} |`, '')
  if (unfinished.length) {
    lines.push(`## ${t('delegation.groupUnfinished')}`, '')
    for (const r of unfinished) lines.push(`- ${cell(doc.recordTitle(r))} — ${t(`delegation.rowChip.${r.scope === 'out_of_scope' ? 'out_of_scope' : r.result}`)} · \`${cell(r.reason)}\``)
    lines.push('')
  }
  if (done.length) {
    lines.push(`## ${t('delegation.groupDone')}`, '')
    for (const r of done) lines.push(`- ${cell(doc.recordTitle(r))} — ${t(`delegation.evidence.${r.evidence}`)}`)
    lines.push('')
  }
  lines.push(`## ${t('delegation.externalTitle')}`, '')
  if (!doc.external.length) lines.push(t('delegation.externalEmpty'))
  for (const op of doc.external) {
    lines.push(`- ${t(`delegation.externalKind.${op.kind}`, { defaultValue: op.kind })} \`${cell(op.target)}\` · ${t(`delegation.externalAction.${op.action}`, { defaultValue: op.action })} · ${t(`delegation.verification.${op.verification}`, { defaultValue: op.verification })} · ${op.at}`)
  }
  lines.push('')
  return lines.join('\n')
}

/** 本次托管花费；时段内有成本未知的用量时写成下界。 */
export function spendText(t: T, summary: ManagedSummary): string {
  const amount = fmtYuan(summary.session_spent_mc)
  return summary.cost_unknown ? t('delegation.spendAtLeast', { amount }) : amount
}

/** 导出；用户取消保存对话框返回 false。 */
export async function exportManagedReport(markdown: string): Promise<boolean> {
  if (isTauri) {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const path = await save({ defaultPath: MANAGED_REPORT_FILE, filters: [{ name: 'Markdown', extensions: ['md'] }] })
    if (!path) return false
    await api.exportManagedReport(path, markdown)
    return true
  }
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([markdown], { type: 'text/markdown' }))
  a.download = MANAGED_REPORT_FILE
  a.click()
  URL.revokeObjectURL(a.href)
  return true
}
