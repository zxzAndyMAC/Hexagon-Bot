// 开场草案卡 IntakeCard（ui-redesign 票 05，components C13、workbench#fresh）。
// 由 IntakeBar（票 17 草案条）升格：在对话里成卡，三个动作层级分明——
// 写入 AGENTS.md（primary，confirmIntake）/ 先不写（quiet）/ 编辑（ghost）。
// 领域不变：草案只在负责人点头后落盘（confirm_intake_brief），自治档不替你写。
// 「先不写」只在本次打开里收起卡片，草案仍在 core（intake_draft），重开项目还会出现——
// core 没有「放弃草案」的命令，界面不伪造这层语义。
// 「编辑」：core 只接受整份写入，也没有读出草案正文的 IPC（全文在开场分析那条消息里），
// 按 components §0 禁用态处理（仍可聚焦，tooltip 写原因）。见 reconciliation §5 票 05。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { confirmIntakeDraft } from '../intakeDraft'
import { bindingFor } from '../keymap'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import { Button } from './ui/button'
import { CodeChip } from './ui/code-chip'
import { ShortcutKbd } from './ui/kbd'
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip'

const dismissed = new Set<string>()

export function IntakeCard() {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.intakeDraft)
  const projectRoot = useUiStore((s) => s.projectRoot) ?? ''
  const [, bump] = useState(0)
  const [busy, setBusy] = useState(false)
  if (!pending || dismissed.has(projectRoot)) return null
  const confirm = async () => {
    setBusy(true)
    try { await confirmIntakeDraft() } finally { setBusy(false) }
  }
  return (
    <section className="tl-intake" data-testid="intake-card" aria-label={t('intake.title')}>
      <div className="tl-intake-h">
        <span className="ic"><Icon name="file-text" size={14} /></span>
        <b>{t('intake.title')}</b>
        <CodeChip>AGENTS.md</CodeChip>
        <span className="tl-sp" style={{ flex: 1 }} />
        <span className="chip warn">{t('intake.unsaved')}</span>
      </div>
      <div className="tl-intake-f">
        <span className="tl-grow">{t('intake.confirmHint')}</span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button size="sm" variant="ghost" disabled>{t('intake.edit')}</Button>
          </TooltipTrigger>
          <TooltipContent>{t('intake.editUnavailable')}</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button size="sm" variant="quiet" onClick={() => { dismissed.add(projectRoot); bump((n) => n + 1) }}>{t('intake.later')}</Button>
          </TooltipTrigger>
          <TooltipContent>{t('intake.laterHint')}</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button size="sm" variant="primary" loading={busy} onClick={() => void confirm()}>{t('intake.confirm')}</Button>
          </TooltipTrigger>
          <TooltipContent><span>{t('intake.confirm')}</span><ShortcutKbd binding={bindingFor('confirmIntake')} /></TooltipContent>
        </Tooltip>
      </div>
    </section>
  )
}
