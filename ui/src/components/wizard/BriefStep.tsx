// ③ 项目说明步（interaction §11，entry#wz-brief，components E10 补设计「三态」）：
// 撰写（一句话 + 优化）→ 答问（卡内逐题）→ 草稿（同一位置换成可编辑草稿）。
// 已有 AGENTS.md / CLAUDE.md：只显示检出 chip，不提供优化或覆盖（票 16）。
// 非空且无说明文件：勾选生成骨架后核对（Owner QA 2026-10-06 / issue 06）。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { BriefQA, BriefQuestion, DirReport } from '../../api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Textarea } from '@/components/ui/input'
import { Icon } from '../Icon'
import { AnsweredRow, BriefQaCard } from './BriefQa'
import { CardLoading } from './CardLoading'
import type { Draft } from './draft'

export type BriefPhase = 'idle' | 'asking' | 'drafting'

export function BriefStep(p: {
  draft: Draft
  report: DirReport | null
  set: (patch: Partial<Draft>) => void
  phase: BriefPhase
  questions: BriefQuestion[] | null
  answers: Record<number, string>
  qIndex: number
  lastQa: BriefQA[] | null
  editing: boolean
  onEdit: () => void
  onBriefChange: (v: string) => void
  onOptimize: () => void
  onIndex: (i: number) => void
  onAnswer: (i: number, v: string) => void
  onContinue: () => void
  onSkip: () => void
  onRedraft: () => void
  footer: React.ReactNode
}) {
  const { t } = useTranslation()
  const { draft, report, set, phase } = p
  const [showAnswered, setShowAnswered] = useState(false)
  const busy = phase !== 'idle'

  if (report?.instructions) {
    return (
      <div className="flex flex-col gap-2 text-[13px]">
        <Badge variant="ok" className="self-start"><Icon name="check" />{report.instructions}</Badge>
        <div className="text-ink-2">{t('wizard.instructionsFound', { file: report.instructions })}</div>
      </div>
    )
  }

  if (!report?.empty) {
    // Owner QA 2026-10-06 / issue 06: a folder with files but no instructions
    // otherwise showed only a checkbox and disabled Next, leaving the owner no
    // explanation of how to continue.
    return (
      <div className="flex flex-col gap-3 text-[13px]">
        {!draft.agentsMd.trim() && !draft.brief.trim() && (
          <p className="text-ink-2">{t('wizard.instructionsRequired')}</p>
        )}
        <label className="flex items-center gap-2">
          <Checkbox data-gen-agents checked={draft.genAgents} onCheckedChange={(v) => set({ genAgents: v === true })} />
          {t('wizard.genAgents')}
        </label>
        {draft.genAgents && <AgentsDraft value={draft.agentsMd} onChange={(agentsMd) => set({ agentsMd })} />}
      </div>
    )
  }

  const qa = p.questions && p.questions.length > 0 ? p.questions : null
  const sentence = draft.brief.trim()
  const composing = (p.editing || (!qa && !draft.agentsMd.trim())) && !busy
  const answered = qa ? qa.flatMap((q, i) => ((p.answers[i] ?? '').trim() && i !== p.qIndex ? [{ i, q, a: p.answers[i] }] : [])) : []

  return (
    <div className="flex flex-col gap-2">
      {composing ? (
        <div data-brief-compose className="flex flex-col gap-2">
          <Textarea
            aria-label={t('wizard.briefPlaceholder')}
            placeholder={t('wizard.briefPlaceholder')}
            value={draft.brief}
            onChange={(e) => p.onBriefChange(e.target.value)}
          />
          <div className="flex items-center gap-2">
            <Button size="sm" disabled={!sentence} onClick={p.onOptimize}>
              <Icon name="sparkle" />{t('wizard.optimize')}
            </Button>
          </div>
        </div>
      ) : (
        <AnsweredRow icon="message" text={`「${sentence}」`}>
          <Button size="xs" variant="ghost" disabled={busy} onClick={p.onEdit}>{t('wizard.qa.edit')}</Button>
        </AnsweredRow>
      )}

      {phase === 'asking' && <CardLoading label={t('wizard.asking')} />}
      {phase === 'drafting' && <CardLoading label={t('wizard.optimizing')} />}

      {!busy && qa && !composing && (
        <>
          {answered.length > 0 && (
            <AnsweredRow
              icon="circle-check"
              tone="ok"
              text={t('wizard.qa.answered', { count: answered.length, list: answered.map((x) => `${x.q.question} = ${x.a}`).join(' · ') })}
            >
              <Button size="xs" variant="ghost" aria-expanded={showAnswered} onClick={() => setShowAnswered((v) => !v)}>
                {t(showAnswered ? 'wizard.qa.collapse' : 'wizard.qa.expand')}
                <Icon name={showAnswered ? 'chevron-down' : 'chevron-right'} />
              </Button>
            </AnsweredRow>
          )}
          {showAnswered && answered.length > 0 && (
            <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
              {answered.map((x) => (
                <li key={x.i}>
                  <button
                    type="button"
                    onClick={() => p.onIndex(x.i)}
                    className="hx-press hx-focus flex w-full cursor-pointer gap-2 rounded-[8px] px-2.5 py-1.5 text-left text-[12.5px] live:hover:bg-hover"
                  >
                    <span className="min-w-0 flex-1 truncate text-ink-2">{x.q.question}</span>
                    <span className="truncate font-medium">{x.a}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="mt-1">
            <BriefQaCard
              key={p.qIndex}
              questions={qa}
              index={p.qIndex}
              answers={p.answers}
              onIndex={p.onIndex}
              onAnswer={p.onAnswer}
              onContinue={p.onContinue}
              onSkip={p.onSkip}
            />
          </div>
        </>
      )}

      {!busy && !qa && draft.agentsMd.trim() !== '' && (
        <>
          {!composing && p.lastQa && p.lastQa.length > 0 && (
            <AnsweredRow icon="circle-check" tone="ok" text={t('wizard.qa.usedForDraft', { count: p.lastQa.length })}>
              <Button size="xs" variant="ghost" aria-expanded={showAnswered} onClick={() => setShowAnswered((v) => !v)}>
                {t('wizard.qa.view')}
              </Button>
              <Button size="xs" variant="ghost" onClick={p.onRedraft}><Icon name="refresh" />{t('wizard.qa.redraft')}</Button>
            </AnsweredRow>
          )}
          {!composing && showAnswered && p.lastQa && (
            <ul className="m-0 flex list-none flex-col gap-0.5 p-0 text-[12.5px]">
              {p.lastQa.map((x, i) => (
                <li key={i} className="flex gap-2 px-2.5 py-1">
                  <span className="min-w-0 flex-1 truncate text-ink-2">{x.question.split('\n')[0]}</span>
                  <span className="truncate font-medium">{x.answer}</span>
                </li>
              ))}
            </ul>
          )}
          {!composing && !(p.lastQa && p.lastQa.length > 0) && (
            <div className="flex justify-end">
              <Button size="xs" variant="ghost" onClick={p.onRedraft}><Icon name="refresh" />{t('wizard.qa.redraft')}</Button>
            </div>
          )}
          <AgentsDraft value={draft.agentsMd} onChange={(agentsMd) => set({ agentsMd })} />
        </>
      )}
      {p.footer}
    </div>
  )
}

function AgentsDraft({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation()
  return (
    <div data-agents-draft className="mt-2 flex flex-col">
      <div className="mb-2 flex items-center gap-2">
        <b className="flex-1 text-[12px] font-medium">{t('wizard.qa.draftTitle')}</b>
        <Badge variant="warn">{t('wizard.qa.notWritten')}</Badge>
      </div>
      <Textarea
        aria-label={t('wizard.qa.draftTitle')}
        title={t('wizard.agentsDraftHint')}
        className="font-mono text-[12px] [&_textarea]:min-h-[12lh] [&_textarea]:leading-[1.6]"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  )
}
