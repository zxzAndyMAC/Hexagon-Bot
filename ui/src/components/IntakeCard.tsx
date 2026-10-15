// 开场草案卡 IntakeCard（ui-redesign 票 05，components C13、workbench#fresh）。
// 由 IntakeBar（票 17 草案条）升格：在对话里成卡，三个动作层级分明——
// 写入 AGENTS.md（primary，confirmIntake）/ 先不写（quiet）/ 编辑（ghost）。
// 领域不变：草案只在负责人点头后落盘（confirm_intake_brief），自治档不替你写。
//
// gap B1（2026-10-15）补齐原型功能：票 05 时 core 没有读草案 / 改草案 / 丢弃草案的门面，
// 卡内没有 inset 预览、「编辑」禁用、「先不写」只在本次打开里收起（重开项目又出现）。
// 现在：预览读 `intake_draft`；「编辑」把 inset 换成文本框，保存走 `update_intake_draft`
// （只改库里的草案，不落盘——写入仍要点头）；「先不写」走 `discard_intake_draft` 持久丢弃，
// 开场分析消息保留在时间线上。
// gap B10（2026-10-15）：「先不写」后出 A18 撤销类 toast「已丢弃草案 · 撤销」，撤销走
// `restore_intake_draft`（core 软删，正文挪回待点头，不落盘）。B1 时丢弃即永久，点错找不回来。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { confirmIntakeDraft } from '../intakeDraft'
import { bindingFor } from '../keymap'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import { Button } from './ui/button'
import { CodeChip } from './ui/code-chip'
import { ShortcutKbd } from './ui/kbd'
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip'

/** 撤销「先不写」：失败（开场分析已起草了新的 / 已写入）报错，不静默。 */
async function restoreDiscarded() {
  try {
    await api.restoreIntakeDraft()
    useUiStore.setState({ intakeDraft: true })
  } catch (e) {
    useUiStore.getState().pushToast(errText(e), 'err')
  }
}

export function IntakeCard() {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.intakeDraft)
  const [body, setBody] = useState<string | null>(null)
  const [editing, setEditing] = useState<string | null>(null)
  const [busy, setBusy] = useState<'' | 'confirm' | 'save' | 'discard'>('')

  useEffect(() => {
    if (!pending) return
    let live = true
    api.intakeDraft().then((d) => { if (live) setBody(d) }).catch(() => { if (live) setBody(null) })
    return () => { live = false }
  }, [pending])

  if (!pending) return null
  const fail = (e: unknown) => useUiStore.getState().pushToast(errText(e), 'err')
  const run = async (kind: typeof busy, f: () => Promise<void>) => {
    setBusy(kind)
    try { await f() } catch (e) { fail(e) } finally { setBusy('') }
  }
  const confirm = () => run('confirm', async () => { setEditing(null); await confirmIntakeDraft() })
  const discard = () => run('discard', async () => {
    await api.discardIntakeDraft()
    setEditing(null)
    useUiStore.setState({ intakeDraft: false })
    useUiStore.getState().pushToast(t('intake.discarded'), 'info', {
      icon: 'undo',
      action: { label: t('intake.undo'), run: restoreDiscarded },
    })
  })
  const save = () => run('save', async () => {
    const next = editing ?? ''
    if (next !== body) await api.updateIntakeDraft(next)
    setBody(next)
    setEditing(null)
  })

  return (
    <section className="tl-intake" data-testid="intake-card" aria-label={t('intake.title')}>
      <div className="tl-intake-h">
        <span className="ic"><Icon name="file-text" size={14} /></span>
        <b>{t('intake.title')}</b>
        <CodeChip>AGENTS.md</CodeChip>
        <span className="tl-sp" style={{ flex: 1 }} />
        <span className="chip warn">{t('intake.unsaved')}</span>
      </div>
      {editing !== null ? (
        <textarea className="tl-intake-pre tl-intake-edit" data-testid="intake-edit" autoFocus spellCheck={false}
          aria-label={t('intake.editLabel')} value={editing}
          onChange={(e) => setEditing(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); setEditing(null) } }} />
      ) : body ? (
        <pre className="tl-intake-pre" data-testid="intake-preview">{body}</pre>
      ) : null}
      {editing !== null ? (
        <div className="tl-intake-f">
          <span className="tl-grow">{t('intake.editHint')}</span>
          <Button size="sm" variant="quiet" onClick={() => setEditing(null)}>{t('intake.cancelEdit')}</Button>
          <Button size="sm" variant="primary" loading={busy === 'save'} disabled={!editing.trim()}
            onClick={() => void save()}>{t('intake.saveEdit')}</Button>
        </div>
      ) : (
        <div className="tl-intake-f">
          <span className="tl-grow">{t('intake.confirmHint')}</span>
          <Button size="sm" variant="ghost" disabled={body === null} onClick={() => setEditing(body ?? '')}>{t('intake.edit')}</Button>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="sm" variant="quiet" loading={busy === 'discard'} onClick={() => void discard()}>{t('intake.later')}</Button>
            </TooltipTrigger>
            <TooltipContent>{t('intake.laterHint')}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="sm" variant="primary" loading={busy === 'confirm'} onClick={() => void confirm()}>{t('intake.confirm')}</Button>
            </TooltipTrigger>
            <TooltipContent><span>{t('intake.confirm')}</span><ShortcutKbd binding={bindingFor('confirmIntake')} /></TooltipContent>
          </Tooltip>
        </div>
      )}
    </section>
  )
}
