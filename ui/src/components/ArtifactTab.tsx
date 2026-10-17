// 产物视图 ArtifactView（components D13，canvas#artifact；interaction §6「产物」）。ui-redesign 票 09 换皮。
// 子头：路径 13 等宽 + 元信息「类型 · 作者 · 交接说明 1 段」+ 状态胶囊 + A7「内容 / 对比」（单版本时「对比」禁用并写原因）；
// 版本行：「版本」+ 版本链胶囊（对比时两端高亮：起点 --hover-2、终点 info，中间 arrow-right）+ 「+N −N」+ 复制；
// 正文 D15 代码视图（Markdown 默认预览，可切源码）；底部交接说明（inset，上线 --line-soft）。
// 选中正文出选区浮条（D14 selbar），「改写」只填进输入条。
// 语义沿用旧 ArtifactTab：当前版本读工作树、历史版本读不可变快照（Reliability 16）；复审证据、
// 待恢复提示、回滚提案入口（ui-audit-2 票 08）都保留。
import { bindingFor, formatBinding, matches } from '../keymap'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ArtifactRow } from '../api'
import { useUiStore } from '../store'
import { diffLines, diffStats, type DiffOp } from '../diff'
import { langFor } from '../highlight'
import { Md } from './Md'
import { Icon } from './Icon'
import { CodeView, DiffCode } from './canvas/CodeView'
import { SelectionBar } from './canvas/SelectionBar'
import { useDomSelection, type SelectionAnchor } from './canvas/useSelection'
import { artifactTone, handoffNote } from './canvas/artifactMeta'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'

const unifiedText = (ops: DiffOp[]) => ops.map((o) => `${o.type === 'ins' ? '+' : o.type === 'del' ? '-' : ' '}${o.text}`).join('\n')

export function ArtifactTab({ path }: { path: string }) {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const team = useUiStore((s) => s.team)
  const timeline = useUiStore((s) => s.timeline)
  const { askConfirm, pushToast } = useUiStore()
  // ui-audit-2 票 08：本路径若有 active 提案 → 露「回滚提案」入口
  // （此前 rollbackProposal 命令死接线，cards.rollback 是死 i18n 键）。
  const rollbackButton = useRef<HTMLButtonElement>(null)
  const [experienceProposal, setExperienceProposal] = useState(false)
  const [policyRecovery, setPolicyRecovery] = useState(false)
  const [activeProposal, setActiveProposal] = useState<string | null>(null)
  useEffect(() => {
    let current = true
    api.proposals()
      .then(async (all) => {
        if (!current) return
        setPolicyRecovery(all.some((r) => r.artifact_path === path && r.recovery_pending))
        const hit = all.find((r) => r.status === 'active' && r.artifact_path === path)
        const experience = hit?.surface === 'skill' ? await api.experienceProposal(hit.id) : null
        if (!current) return
        setExperienceProposal(!!experience)
        setActiveProposal(hit?.id ?? null)
      })
      .catch(() => { if (current) { setActiveProposal(null); setPolicyRecovery(false); setExperienceProposal(false) } })
    return () => { current = false }
  }, [path, artifacts])
  // 同路径版本链（升序）
  const versions = useMemo(
    () => artifacts.filter((a) => a.path === path).sort((a, b) => a.version - b.version),
    [artifacts, path],
  )
  const latest = versions[versions.length - 1]
  const [vA, setVA] = useState<number | null>(null) // 对比起点
  const [vB, setVB] = useState<number | null>(null) // 查看 / 对比终点
  const [mode, setMode] = useState<'content' | 'diff'>('content')
  const [textA, setTextA] = useState<string | null>(null)
  const [textB, setTextB] = useState<string | null>(null)

  const isMd = path.toLowerCase().endsWith('.md')
  const [mdView, setMdView] = useState<'preview' | 'source'>('preview')

  const selB = vB ?? latest?.version ?? 1
  const older = versions.filter((a) => a.version < selB)
  const selA = vA != null && vA < selB ? vA : older[older.length - 1]?.version ?? versions[0]?.version ?? 1

  const selected = versions.find((a) => a.version === selB)
  const review = selected?.review
  const canCompare = versions.length > 1

  const currentWorktree = mode === 'content' && selB === latest?.version
  useEffect(() => {
    let live = true
    // Reliability 16: current delivery is the worktree, historical versions are
    // immutable evidence. A registered snapshot cannot hide an external edit.
    const read = currentWorktree ? api.artifactContent(path) : api.artifactContentAt(path, selB)
    read.then((c) => { if (live) setTextB(c) }).catch(() => { if (live) setTextB(null) })
    return () => { live = false }
  }, [path, selB, currentWorktree, artifacts])
  useEffect(() => {
    if (mode !== 'diff') return
    let live = true
    api.artifactContentAt(path, selA).then((c) => { if (live) setTextA(c) }).catch(() => { if (live) setTextA(null) })
    return () => { live = false }
  }, [path, selA, mode])

  const ops = useMemo(
    () => (mode === 'diff' && textA != null && textB != null
      ? diffLines(textA.split('\n'), textB.split('\n'))
      : []),
    [mode, textA, textB],
  )
  const stats = useMemo(() => diffStats(ops), [ops])

  /** 版本胶囊：内容态选查看版本；对比态点更早的设起点、更晚的设终点（终点不低于起点）。 */
  const pickVersion = (v: number) => {
    if (mode === 'content' || v > selB) setVB(v)
    else if (v < selB) setVA(v)
  }

  const author = latest?.author ? team.find((m) => m.id === latest.author)?.role ?? latest.author : null
  const note = handoffNote(timeline, path, selB, textB)
  const meta = [latest?.kind, author, note ? t('canvas.handoffCount') : null].filter(Boolean).join(' · ')

  const copy = async () => {
    const body = mode === 'diff' ? unifiedText(ops) : textB
    if (body == null) return
    try {
      await navigator.clipboard.writeText(body)
      pushToast(t('canvas.copied'), 'ok')
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }

  const bodyRef = useRef<HTMLDivElement>(null)
  const [anchor, setAnchor] = useState<SelectionAnchor | null>(null)
  const closeBar = useCallback(() => setAnchor(null), [])
  useDomSelection(bodyRef, path, setAnchor)

  const tip = (label: string, binding: string) => [label, binding && formatBinding(binding)].filter(Boolean).join(' · ')

  return (
    <div data-artifact-view onKeyDown={(e) => {
      const state = useUiStore.getState()
      if (experienceProposal && !e.repeat && state.modalScope === 'workbench' && !state.confirmReq && matches(e.nativeEvent, bindingFor('rollbackExperience'))) { e.preventDefault(); e.stopPropagation(); rollbackButton.current?.click() }
      if (!e.repeat && bindingFor('copyArtifact') && matches(e.nativeEvent, bindingFor('copyArtifact'))) { e.preventDefault(); void copy() }
    }} className="flex min-h-0 flex-1 flex-col">
      {/* 子头（cv-sub） */}
      <div className="flex flex-wrap items-center gap-2 border-b border-line-soft px-3.5 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="truncate font-mono text-[13px] text-ink" title={path}>{path}</div>
          {meta && <div className="truncate text-[12px] text-ink-3">{meta}</div>}
        </div>
        {latest && (
          <Badge variant={artifactTone(latest)} data-artifact-status>
            {latest.materialization === 'pending_recovery' ? t('side.pendingRecovery') : t(`side.${latest.status}`, latest.status)}
          </Badge>
        )}
        {policyRecovery && <Badge variant="warn" role="alert">{t('policy.recovery')}</Badge>}
        {activeProposal && (
          <Button
            ref={rollbackButton}
            variant="ghost"
            size="xs"
            className="text-red"
            disabled={policyRecovery}
            title={experienceProposal ? tip(t('experience.withdraw'), bindingFor('rollbackExperience')) : t('cards.rollbackHint')}
            onClick={() =>
              askConfirm({
                title: t('cards.rollbackTitle', { id: activeProposal }),
                body: t(experienceProposal ? 'experience.withdrawBody' : 'cards.rollbackBody'),
                danger: true,
                confirmLabel: t('cards.rollback'),
                run: () =>
                  api
                    .rollbackProposal(activeProposal)
                    .then(() => setActiveProposal(null))
                    .catch((e) => pushToast(errText(e), 'err')),
              })
            }
          >
            {t(experienceProposal ? 'experience.withdraw' : 'cards.rollback')}
          </Button>
        )}
        {isMd && mode === 'content' && (
          <Segmented value={mdView} onValueChange={(v) => setMdView(v as 'preview' | 'source')} aria-label={t('art.preview')}>
            <SegmentedItem value="preview">{t('art.preview')}</SegmentedItem>
            <SegmentedItem value="source">{t('art.source')}</SegmentedItem>
          </Segmented>
        )}
        <Segmented value={mode} onValueChange={(v) => setMode(v as 'content' | 'diff')} aria-label={t('art.compare')}>
          <SegmentedItem value="content">{t('art.content')}</SegmentedItem>
          <SegmentedItem value="diff" disabled={!canCompare} disabledReason={t('canvas.singleVersion')}>{t('art.compare')}</SegmentedItem>
        </Segmented>
      </div>
      {/* 版本行 */}
      <div className="flex flex-wrap items-center gap-2 border-b border-line-soft px-3.5 py-2">
        <span className="text-[12px] text-ink-3">{t('canvas.versions')}</span>
        <VersionChain versions={versions} mode={mode} a={selA} b={selB} onPick={pickVersion} />
        {currentWorktree && <span className="text-[12px] text-ink-3">{t('art.currentWorktree')}</span>}
        <span className="flex-1" />
        {mode === 'diff' && textA != null && textB != null && (
          <span className="font-mono text-[12px]" data-diff-stats>
            <span className="text-green">+{stats.add}</span> <span className="text-red">−{stats.del}</span>
          </span>
        )}
        <Button variant="ghost" size="xs" title={tip(t('canvas.copy'), bindingFor('copyArtifact'))} data-artifact-copy
          disabled={mode === 'diff' ? textA == null || textB == null : textB == null} onClick={() => void copy()}>
          <Icon name="copy" size={12} />{t('canvas.copy')}
        </Button>
      </div>
      {review && (
        <div className="flex items-center gap-2 px-3.5 pt-2.5">
          <Badge variant={review.status === 'passed' ? 'ok' : 'warn'}>{t(review.status === 'passed' ? 'art.reviewPassed' : review.status === 'rejected' ? 'art.reviewRejected' : 'art.reviewStale')}</Badge>
          <span className="text-[12px] text-ink-3">{review.reviewer}{review.version != null ? ` · v${review.version}` : ''}</span>
        </div>
      )}
      {latest?.materialization === 'pending_recovery' && <div className="px-3.5 pt-2.5 text-[12.5px] text-ink-2">{t('art.pendingRecovery')}</div>}
      <div ref={bodyRef} className="relative flex min-h-0 flex-1 flex-col">
        {mode === 'content'
          ? (textB != null
              ? (isMd && mdView === 'preview'
                  ? <div className="msg-body md-doc min-h-0 flex-1 overflow-y-auto"><Md>{textB}</Md></div>
                  : <CodeView text={textB} lang={langFor(path)} label={path} />)
              : <div className="p-3.5 text-[12.5px] text-ink-3">{t('art.unavailable')}</div>)
          : (textA != null && textB != null
              ? <DiffCode ops={ops} before={textA} after={textB} lang={langFor(path)} label={path} />
              : <div className="p-3.5 text-[12.5px] text-ink-3">{t('art.unavailable')}</div>)}
        <SelectionBar anchor={anchor} onClose={closeBar} />
      </div>
      {note && (
        <div data-handoff className="mx-3.5 mb-3.5 rounded-[var(--r-card)] bg-inset px-3 py-2.5 shadow-[inset_0_0_0_1px_var(--line-soft)]">
          <div className="mb-2 flex items-center gap-1.5 text-[12px] text-ink-3">
            <Icon name="message" size={12} />{author ? t('canvas.handoffBy', { who: author }) : t('canvas.handoff')}
          </div>
          <div className="text-[13px] whitespace-pre-wrap text-ink">{note}</div>
        </div>
      )}
    </div>
  )
}

function VersionChain({ versions, mode, a, b, onPick }: {
  versions: ArtifactRow[]
  mode: 'content' | 'diff'
  a: number
  b: number
  onPick: (v: number) => void
}) {
  const { t } = useTranslation()
  return (
    <div className="flex flex-wrap items-center gap-1" role="group" aria-label={t('canvas.versions')}>
      {versions.map((row) => {
        const end = row.version === b
        const start = mode === 'diff' && row.version === a
        return (
          <span key={row.id} className="contents">
            {mode === 'diff' && end && <Icon name="arrow-right" size={12} className="text-ink-3" />}
            <button type="button" aria-pressed={end || start} data-version={row.version}
              onClick={() => onPick(row.version)}
              className={cn(
                'hx-press hx-focus inline-flex h-[22px] cursor-pointer items-center rounded-full px-2 text-[12px] font-medium',
                end ? 'bg-accent-tint text-accent-ink'
                  : start ? 'bg-hover-2 text-ink'
                  : 'text-ink-2 shadow-[inset_0_0_0_1px_var(--line-strong)] live:hover:bg-hover',
              )}>
              {t('art.version', { n: row.version })}
            </button>
          </span>
        )
      })}
    </div>
  )
}
