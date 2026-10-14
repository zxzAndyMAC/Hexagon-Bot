// 设置「键盘」分区（ui-redesign 票 11；settings#keys，components E7）。
// 全部动作从 keymap ACTIONS 自动列出（按 keyGroups 归组），所以 02、03 新增和改名的动作不用改这里。
// 点绑定进入录制：绑定位换成 A9 聚焦形「按下新键位…」+ 取消；Esc 取消；冲突就地红字「已被『…』占用」不写入；
// 系统保留组合不可录。旁边是练习场（PracticeGround）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ACTIONS, bindingFor, formatBinding, normalizeEvent, resetBinding, setBinding, type ActionId } from '../../keymap'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Icon } from '../Icon'
import { PracticeGround } from '../PracticeGround'
import { GroupTitle, SetHead, SetWrap } from './kit'
import { findConflict, groupActions, RESERVED_BINDINGS } from './keyGroups'

type Problem = { kind: 'conflict'; action: ActionId } | { kind: 'reserved'; combo: string } | null

function KeyRow({ id, label, onChanged }: { id: ActionId; label: string; onChanged: () => void }) {
  const { t } = useTranslation()
  const [capturing, setCapturing] = useState(false)
  const [problem, setProblem] = useState<Problem>(null)

  useEffect(() => {
    if (!capturing) return
    const h = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') { setCapturing(false); return }
      const b = normalizeEvent(e)
      if (!b) return // 纯修饰键，继续等
      if (RESERVED_BINDINGS.has(b.toLowerCase())) { setProblem({ kind: 'reserved', combo: b }); setCapturing(false); return }
      const c = findConflict(id, b)
      if (c) { setProblem({ kind: 'conflict', action: c }); setCapturing(false); return }
      setBinding(id, b)
      setProblem(null)
      setCapturing(false)
      onChanged()
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [capturing, id, onChanged])

  const binding = bindingFor(id)
  const conflictLabel = problem?.kind === 'conflict'
    ? t(ACTIONS.find((a) => a.id === problem.action)?.labelKey ?? problem.action)
    : ''
  return (
    <div data-key-row={id} className="border-t border-line-soft px-4 py-2.5 text-[13px] first:border-t-0 live:hover:bg-hover">
      <div className="grid grid-cols-[1fr_auto_auto] items-center gap-2.5">
        <span className="min-w-0 truncate text-ink">{label}</span>
        {capturing ? (
          <span
            data-recording
            role="status"
            className="inline-flex h-6 items-center rounded-[6px] bg-surface px-2 font-mono text-[12px] text-accent-ink shadow-[var(--focus-ring)]"
          >
            {t('settings.pressKey')}
          </span>
        ) : (
          <button
            type="button"
            aria-label={t('settingsUi.recordKey', { action: label })}
            onClick={() => { setProblem(null); setCapturing(true) }}
            className="hx-press hx-focus inline-flex h-6 min-w-12 cursor-pointer items-center justify-center rounded-[6px] bg-canvas px-2 font-mono text-[11.5px] font-medium text-ink-2 shadow-[inset_0_0_0_1px_var(--line)] live:hover:shadow-[inset_0_0_0_1px_var(--line-strong)]"
          >
            {binding ? formatBinding(binding) : t('settingsUi.unbound')}
          </button>
        )}
        {capturing ? (
          <Button size="xs" variant="ghost" onClick={() => setCapturing(false)}>{t('agent.cancel')}</Button>
        ) : (
          <Button size="xs" variant="ghost" onClick={() => { resetBinding(id); setProblem(null); onChanged() }}>
            {t('settings.resetKey')}
          </Button>
        )}
      </div>
      {problem && (
        <div role="alert" className="mt-1 text-[12px] text-red">
          {problem.kind === 'conflict'
            ? t('settingsUi.keyTaken', { action: conflictLabel })
            : t('settingsUi.keyReserved', { key: formatBinding(problem.combo) })}
        </div>
      )}
    </div>
  )
}

export function KeysSection() {
  const { t } = useTranslation()
  const [q, setQ] = useState('')
  const [, setTick] = useState(0)
  const bump = () => setTick((n) => n + 1)
  const labelled = ACTIONS.map((a) => ({ ...a, label: t(a.labelKey) }))
  const shown = labelled.filter((a) => !q || `${a.label} ${a.id}`.toLowerCase().includes(q.toLowerCase()))
  return (
    <SetWrap>
      <SetHead
        title={t('settings.nav_keys')}
        desc={t('settingsUi.keysDesc')}
        actions={
          <Input
            className="w-[200px]"
            leading={<Icon name="search" size={14} />}
            placeholder={t('settingsUi.searchActions')}
            aria-label={t('settingsUi.searchActions')}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        }
      />
      {groupActions(shown).map((g) => (
        <section key={g.group} aria-label={t(`settingsUi.keyGroup_${g.group}`)} className="mb-5">
          <GroupTitle>{t(`settingsUi.keyGroup_${g.group}`)}</GroupTitle>
          <div className="rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
            {g.actions.map((a) => <KeyRow key={a.id} id={a.id} label={a.label} onChanged={bump} />)}
          </div>
        </section>
      ))}
      {shown.length === 0 && <div className="py-6 text-center text-[12.5px] text-ink-3">{t('settingsUi.noMatches')}</div>}
      <GroupTitle className="mt-5">{t('settings.practiceTitle')}</GroupTitle>
      <PracticeGround />
    </SetWrap>
  )
}
