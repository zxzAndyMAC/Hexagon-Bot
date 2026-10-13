// ⑤ 流程草稿步（interaction §11，entry#wz-flow，components E10 流程行 + 补设计）：
// 阶段行可拖拽排序，同时提供键盘上下移（WCAG dragging-alternative，accessibility §3）。
// 可改阶段名、激活角色（名单 = 角色步勾上的角色，名单外已有名以 custom 行保留）与盖章点；
// 检验命令、产物清单、回填边不出现在编辑面上。质量检查命令单独填写（ADR 0069：只有负责人在
// 这块可见表单里手填的命令才是执行授权）。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { PackDef } from '../../api'
import type { QualityCategory } from '../../gen/QualityCategory'
import { bindingFor, formatBinding, matches } from '../../keymap'
import { cn } from '@/lib/utils'
import { Avatar, AvatarStack } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { Input } from '@/components/ui/input'
import { EntityPicker } from '../EntityPicker'
import { Icon } from '../Icon'
import { CardLoading } from './CardLoading'
import { QUALITY_CATEGORIES, moveStage } from './draft'

type Stage = PackDef['stages'][number]

export function FlowDraft({ pack, roles, busy, onChange }: {
  pack: PackDef | null
  roles: string[]
  busy: boolean
  onChange: (pack: PackDef) => void
}) {
  const { t } = useTranslation()
  const [dragFrom, setDragFrom] = useState<number | null>(null)
  const [picking, setPicking] = useState<number | null>(null)
  const handles = useRef<(HTMLButtonElement | null)[]>([])
  // 键盘移动后焦点跟随该行（补设计：移动后焦点跟随）。用 ref 而非 state：只在移动后的那次提交生效，
  // 之后改阶段名等编辑不会把焦点抢回拖动柄。
  const focusRow = useRef<number | null>(null)
  useEffect(() => {
    if (focusRow.current === null) return
    handles.current[focusRow.current]?.focus()
    focusRow.current = null
  })

  if (!pack || busy) {
    // draft_flow 在途——旧稿还在时也要换成等待态，否则下一步锁着、页面仍像能编辑
    return <CardLoading label={t(pack ? 'wizard.flowRedraft' : 'wizard.flowDraft')} />
  }
  const stages = pack.stages
  const update = (next: Stage[]) => onChange({ ...pack, stages: next })
  const patch = (i: number, p: Partial<Stage>) => {
    const next = stages.slice()
    next[i] = { ...stages[i], ...p }
    update(next)
  }
  const move = (from: number, to: number) => {
    const next = moveStage(stages, from, to)
    if (next === stages) return
    update(next)
    focusRow.current = Math.max(0, Math.min(stages.length - 1, to))
  }
  const moveHint = `${formatBinding(bindingFor('moveStageUp'))} / ${formatBinding(bindingFor('moveStageDown'))}`

  return (
    <div data-flow-draft>
      <div className="mb-2 text-[12px] text-ink-3">{pack.name}</div>
      <div className="flex flex-col gap-1.5">
        {stages.map((st, i) => (
          <div
            key={i}
            data-stage-row
            draggable
            onDragStart={(e) => { setDragFrom(i); e.dataTransfer.effectAllowed = 'move' }}
            onDragEnd={() => setDragFrom(null)}
            onDragOver={(e) => { if (dragFrom !== null) e.preventDefault() }}
            onDrop={(e) => {
              e.preventDefault()
              if (dragFrom !== null) move(dragFrom, i)
              setDragFrom(null)
            }}
            onKeyDown={(e) => {
              if (matches(e, bindingFor('moveStageUp'))) { e.preventDefault(); move(i, i - 1) }
              else if (matches(e, bindingFor('moveStageDown'))) { e.preventDefault(); move(i, i + 1) }
            }}
            className={cn(
              'grid grid-cols-[20px_120px_1fr_64px_28px] items-center gap-2.5 rounded-[10px] px-2.5 py-2 text-[13px] shadow-[inset_0_0_0_1px_var(--line)]',
              dragFrom === i && 'bg-surface shadow-[var(--shadow-raised)]',
            )}
          >
            <button
              ref={(el) => { handles.current[i] = el }}
              type="button"
              aria-label={t('wizard.flow.drag')}
              title={`${t('wizard.flow.drag')} · ${moveHint}`}
              aria-keyshortcuts={`${bindingFor('moveStageUp')} ${bindingFor('moveStageDown')}`.replaceAll('alt', 'Alt')}
              className="hx-focus grid h-6 w-5 cursor-grab place-items-center rounded-[6px] text-ink-3 live:hover:bg-hover"
            >
              <Icon name="more-vertical" size={12} />
            </button>
            <input
              aria-label={t('wizard.stageName')}
              value={st.name}
              onChange={(e) => patch(i, { name: e.target.value })}
              className="hx-focus h-7 min-w-0 rounded-[6px] bg-transparent px-1.5 text-[13px] font-semibold text-ink outline-none focus:bg-field"
            />
            <button
              type="button"
              data-stage-roles
              aria-label={`${t('wizard.stageRoles')}: ${st.roles.join(', ') || '—'}`}
              title={st.roles.join('、') || t('wizard.stageRoles')}
              onClick={() => setPicking(i)}
              className="hx-press hx-focus flex h-7 min-w-0 cursor-pointer items-center gap-1 justify-self-start rounded-[6px] px-1 live:hover:bg-hover"
            >
              {st.roles.length > 0 ? (
                <AvatarStack>{st.roles.map((r) => <Avatar key={r} name={r} size={18} />)}</AvatarStack>
              ) : (
                <Icon name="plus" size={12} className="text-ink-3" />
              )}
            </button>
            <button
              type="button"
              data-stamp-toggle
              aria-pressed={st.stamp_point}
              aria-label={t('wizard.stampPoint')}
              onClick={() => patch(i, { stamp_point: !st.stamp_point })}
              className="hx-press hx-focus cursor-pointer justify-self-start rounded-full"
            >
              {st.stamp_point
                ? <Badge variant="warn"><Icon name="stamp" />{t('wizard.flow.stamp')}</Badge>
                : <Badge>{t('wizard.flow.auto')}</Badge>}
            </button>
            <IconButton size="sm" label={t('wizard.removeStage')} onClick={() => update(stages.filter((_, j) => j !== i))}>
              <Icon name="close" />
            </IconButton>
          </div>
        ))}
      </div>
      <Button
        size="sm"
        variant="ghost"
        className="mt-2"
        data-add-stage
        onClick={() => update([...stages, {
          name: t('wizard.newStage'),
          roles: roles.slice(0, 1),
          due: [],
          checks: [],
          quality_checks: {},
          reviews: [],
          stamp_point: false,
          backfill_edges: [],
          consult_wake: [],
        }])}
      >
        <Icon name="plus" />{t('wizard.addStage')}
      </Button>
      {picking !== null && stages[picking] && (
        <EntityPicker
          options={roles.map((n) => ({ id: n, desc: '', badge: '' }))}
          title={t('wizard.stageRoles')}
          selected={stages[picking].roles}
          onClose={() => setPicking(null)}
          onConfirm={(ids) => {
            // picker 回传点击序——落库按勾选名单序，编外名接尾
            patch(picking, { roles: roles.filter((r) => ids.includes(r)).concat(ids.filter((id) => !roles.includes(id))) })
            setPicking(null)
          }}
        />
      )}
    </div>
  )
}

export function QualityCommandsForm({ commands, onChange }: {
  commands: Partial<Record<QualityCategory, string>>
  onChange: (category: QualityCategory, value: string) => void
}) {
  const { t } = useTranslation()
  return (
    <fieldset className="m-0 mt-5 border-0 p-0">
      <legend className="mb-1 text-[12px] font-medium">{t('quality.setupTitle')}</legend>
      <p className="mb-2 text-[12px] text-ink-3">{t('quality.setupHint')}</p>
      <div className="grid grid-cols-2 gap-2">
        {QUALITY_CATEGORIES.map((category) => (
          <Input
            key={category}
            data-quality-command={category}
            aria-label={t(`quality.category.${category}`)}
            placeholder={t(`quality.category.${category}`)}
            className="font-mono text-[12px]"
            value={commands[category] ?? ''}
            onChange={(e) => onChange(category, e.target.value)}
          />
        ))}
      </div>
    </fieldset>
  )
}
