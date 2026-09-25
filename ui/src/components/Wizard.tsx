// 项目向导：模型服务商 → 选目录 → 项目说明 → 勾角色 → 流程草稿 → 密钥 → 确认开跑。
// 2026-09-25 owner 裁决：说明先于角色——先有项目语境，AI 才能起草职责。
// ADR 0075：AI 起草不再产归属 globs（新项目无目录结构，虚构 globs 会误触发
// 越权裁决）；globs 字段收进「高级选项」折叠，留空 = 不限制写入范围。
// 快速通道在确认页，不采用流程草稿。
// 票 13：第一步没有启用的供应商、已存钥匙和 default 槽就不能进目录；已配好只显示就绪。
// 草稿存 localStorage `hexagon.wizard`，中途退出可续；缺密钥 fail-closed 不能开跑。
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, asCmdError, errText, isTauri, type BriefQA, type BriefQuestion, type DirReport, type PackDef, type ProviderView, type ProvidersView, type RoleDef, type RoleTemplate } from '../api'
import { applyCreateFailure, CREATE_STEPS, type CreateStep } from '../createProgress'
import { useUiStore } from '../store'
import { sharedSlots, slotLabel } from '../modelpick'
import { providerStepReady } from '../providerGate'
import { Icon } from './Icon'
import { EntityChips } from './EntityPicker'
import { LoadingState } from './LoadingState'

const DRAFT_KEY = 'hexagon.wizard'

interface Draft {
  dir: string
  name: string
  roles: string[]
  /// ADR 0057：按角色名存定制后的完整 RoleDef（只作用本项目，不回写模板库）。
  /// 缺项 = 用模板原定义。
  roleOverrides: Record<string, RoleDef>
  /// override 里哪些来自 AI 起草（可再起草覆盖）；人手经定制卡保存
  /// 即移出此名单——人手编辑永远不被 AI 盖掉。老草稿缺省为空，
  /// 其中 AI 落的 override 视同人手（保守，宁不多盖）。
  roleDrafted: string[]
  /// 定制卡里点过保存的角色。只有这份名单不参与 AI 起草。
  /// 不能用「有 override 但不在 roleDrafted」来推断——那份名单曾经只记下
  /// 产品策划，其余角色的旧稿就被永久跳过（owner 2026-09-25）。
  roleHandTuned: string[]
  /// 上次 AI 起草时用的项目身份（目录 + 名称 + 说明正文）。
  /// 和当前不一致则 AI 职责按模板显示，按钮回到「起草」。
  roleDraftKey?: string
  /// 目录里已有的说明文件正文。有它时起草以它为准，不用向导里留下的旧稿。
  instructionText: string
  mode: 'pack' | 'fastpath'
  packName: string
  fastRole: string
  initGit: boolean
  genAgents: boolean
  agentsMd: string
  /// 票 16：空目录项目说明的种子。优化前不落盘。
  brief: string
  /// 确认前的流程草稿。快速通道不采用它。
  flowPack: PackDef | null
  fastPath: boolean
}

// roles 步默认全选（owner 裁决 2026-09-25，取代票 08 的「只勾项目经理」）：
// 只作用全新草稿——存档草稿按保存的勾选恢复，卸掉的角色不被默认值重新勾上
// （票 08 语义不丢）。模板拉取失败时回落这里写死的「项目经理」。
const EMPTY: Draft = {
  dir: '', name: '', roles: ['项目经理'], roleOverrides: {}, roleDrafted: [], roleHandTuned: [], instructionText: '', mode: 'pack',
  packName: '', fastRole: '', initGit: false, genAgents: false, agentsMd: '', brief: '',
  flowPack: null, fastPath: false,
}

function loadDraft(): Draft {
  try {
    const stored = JSON.parse(localStorage.getItem(DRAFT_KEY) || '{}')
    const merged = { ...EMPTY, ...stored }
    // roleDrafted 引入前的老草稿：键缺省说明 override 只能经旧版起草按钮
    // 或定制卡落库。缺省按「全部 AI 起草」回填——否则老草稿按钮永久锁死、
    // 无从再起草。代价：老草稿里真手改的 override 会被下次起草覆盖——
    // 结果仍落可编辑 override，损失有界（owner 裁决 2026-09-25）。
    if (stored.roleDrafted === undefined) {
      merged.roleDrafted = Object.keys(merged.roleOverrides ?? {})
    }
    // 老存档没有起草时的项目身份：用当时的说明钉住，说明一改就视为过期。
    if (merged.roleDraftKey == null && merged.roleDrafted.length > 0) {
      merged.roleDraftKey = projectKey(merged)
    }
    return merged
  } catch {
    return { ...EMPTY }
  }
}

function projectKey(d: Pick<Draft, 'dir' | 'name' | 'instructionText' | 'agentsMd' | 'brief'>) {
  // 一句话和生成稿都算项目身份。只盯其中一份时，改了另一份不会让旧职责过期。
  const text = d.instructionText.trim() || `${d.agentsMd.trim()}\0${d.brief.trim()}`
  return `${d.dir}\0${d.name}\0${text}`
}

/// 起草用的说明。一句话改了而生成稿还是上一个项目时，以一句话为准，
/// 生成稿标成过期，避免模型只改第一个角色、其余照抄旧项目。
function draftSource(d: Pick<Draft, 'instructionText' | 'agentsMd' | 'brief'>): string {
  const file = d.instructionText.trim()
  if (file) return file
  const md = d.agentsMd.trim()
  const brief = d.brief.trim()
  if (md && brief && !md.includes(brief)) {
    return `The owner's latest instruction replaces any older description:\n${brief}`
  }
  return md || brief
}

const STEPS = ['providers', 'dir', 'brief', 'roles', 'flow', 'keys', 'confirm'] as const
type Step = (typeof STEPS)[number]

function FlowDraft({
  pack, roles, onChange,
}: {
  pack: PackDef | null
  roles: string[]
  onChange: (pack: PackDef) => void
}) {
  const { t } = useTranslation()
  if (!pack) {
    // draft_flow 在途——AI 指示同 brief 优化（动画+流光文案+计时）
    return <LoadingState label={t('wizard.flowDraft')} />
  }
  const stages = pack.stages
  const update = (next: PackDef['stages']) => onChange({ ...pack, stages: next })
  return (
    <div data-flow-draft>
      <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{pack.name}</div>
      {stages.map((st, i) => (
        <div key={i} data-stage-row style={{ display: 'grid', gap: 6, marginBottom: 10 }}>
          <input
            className="input"
            aria-label={t('wizard.stageName')}
            value={st.name}
            onChange={(e) => {
              const next = stages.slice()
              next[i] = { ...st, name: e.target.value }
              update(next)
            }}
          />
          {/* 阶段角色=第三步勾选名单里的多选（owner 名单字段裁决：chip+勾选弹窗，不手填）。
              st.roles 里名单外的名字（草稿自带/后来卸掉的角色）由 picker 合成 custom 行保留 */}
          <div role="group" aria-label={t('wizard.stageRoles')}>
            <EntityChips
              value={st.roles}
              options={roles.map((n) => ({ id: n, desc: '', badge: '' }))}
              title={t('wizard.stageRoles')}
              onChange={(ids) => {
                const next = stages.slice()
                next[i] = {
                  ...st,
                  // picker 回传点击序——落库按勾选名单序，编外名接尾
                  roles: roles.filter((r) => ids.includes(r))
                    .concat(ids.filter((id) => !roles.includes(id))),
                }
                update(next)
              }}
            />
          </div>
          <label style={{ display: 'flex', gap: 6, fontSize: 12 }}>
            <input
              type="checkbox"
              checked={st.stamp_point}
              onChange={(e) => {
                const next = stages.slice()
                next[i] = { ...st, stamp_point: e.target.checked }
                update(next)
              }}
            />
            {t('wizard.stampPoint')}
          </label>
          <div style={{ display: 'flex', gap: 6 }}>
            <button className="btn" type="button" disabled={i === 0} onClick={() => {
              const next = stages.slice()
              const [row] = next.splice(i, 1)
              next.splice(i - 1, 0, row)
              update(next)
            }}>{t('wizard.moveUp')}</button>
            <button className="btn" type="button" disabled={i === stages.length - 1} onClick={() => {
              const next = stages.slice()
              const [row] = next.splice(i, 1)
              next.splice(i + 1, 0, row)
              update(next)
            }}>{t('wizard.moveDown')}</button>
            <button className="btn" type="button" onClick={() => update(stages.filter((_, j) => j !== i))}>
              {t('wizard.removeStage')}
            </button>
          </div>
        </div>
      ))}
      <button
        className="btn"
        type="button"
        data-add-stage
        onClick={() => update([...stages, {
          name: t('wizard.newStage'),
          roles: roles.slice(0, 1),
          due: [],
          checks: [],
          reviews: [],
          stamp_point: false,
          backfill_edges: [],
          consult_wake: [],
        }])}
      >
        {t('wizard.addStage')}
      </button>
    </div>
  )
}

/** 新建角色：右侧并列弹卡（同 RoleCustomize 对位），取消整份丢弃——
 *  表单内容不暂存，弹卡关掉就没了。「AI 起草」是模型调用：onBusy 上报父级锁跳步。 */
function NewRoleForm({ names, onDone, onCancel, onBusy }: {
  names: string[]
  onDone: (def: RoleDef) => void
  onCancel: () => void
  onBusy: (b: boolean) => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [duty, setDuty] = useState('')
  const [reviewer, setReviewer] = useState('')
  const [slot, setSlot] = useState('default')
  const [globs, setGlobs] = useState('')
  const [skills, setSkills] = useState('')
  const [err, setErr] = useState('')
  const [drafting, setDrafting] = useState(false)
  // ADR 0075：归属 globs 收进高级选项——不懂技术的人不该手填，
  // 留空 = 不限制写入范围。
  const [adv, setAdv] = useState(false)
  return (
    <div
      data-new-role-form className="panel" role="dialog" aria-label={t('agent.createRole')}
      style={{
        width: 'min(400px, 90vw)', flexShrink: 0, maxHeight: '86vh',
        display: 'flex', flexDirection: 'column', padding: '14px 16px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <strong style={{ fontSize: 13 }}>{t('agent.createRole')}</strong>
        <span className="dim3" style={{ fontSize: 11 }}>{t('wizard.scopeHint')}</span>
        <span style={{ flex: 1 }} />
        <button className="btn" type="button" onClick={onCancel}>{t('agent.cancel')}</button>
      </div>
      <div style={{ flex: 1, overflowY: 'auto', minHeight: 0, marginTop: 4 }}>
        <input className="input" aria-label={t('agent.roleName')} value={name} onChange={(e) => setName(e.target.value)} placeholder={t('agent.roleName')} />
        <textarea className="input" aria-label={t('agent.duty')} value={duty} onChange={(e) => setDuty(e.target.value)} placeholder={t('agent.duty')} style={{ marginTop: 6, width: '100%', minHeight: 48 }} />
        <select className="input" aria-label={t('agent.reviewer')} value={reviewer} onChange={(e) => setReviewer(e.target.value)} style={{ marginTop: 6 }}>
          <option value="">{t('agent.noReviewer')}</option>
          {names.map((n) => <option key={n} value={n}>{n}</option>)}
        </select>
        <input className="input" aria-label={t('agent.modelSlot')} value={slot} onChange={(e) => setSlot(e.target.value)} style={{ marginTop: 6 }} />
        <input className="input" aria-label={t('agent.skills')} value={skills} onChange={(e) => setSkills(e.target.value)} placeholder={t('agent.skills')} style={{ marginTop: 6 }} />
        <button
          type="button" className="btn" data-globs-toggle aria-expanded={adv}
          style={{ fontSize: 11, marginTop: 8, padding: '2px 8px', alignSelf: 'flex-start', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          onClick={() => setAdv((v) => !v)}
        >
          <Icon name={adv ? 'chevron-down' : 'chevron-right'} size={9} />
          {t('agent.advanced')}
        </button>
        {adv && (
          <>
            <div className="dim3" style={{ fontSize: 11, marginTop: 6 }}>{t('agent.globsEmpty')}</div>
            <textarea className="input" aria-label={t('agent.globs')} value={globs} onChange={(e) => setGlobs(e.target.value)} placeholder={t('agent.globs')} style={{ marginTop: 4, width: '100%', minHeight: 40 }} />
          </>
        )}
      </div>
      {err && <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 6 }}>{err}</div>}
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 10 }}>
        <button
          className="btn primary"
          type="button"
          disabled={!name.trim() || drafting}
          onClick={() => {
            const def: RoleDef = {
              name: name.trim(),
              duty: duty.trim(),
              reviewer: reviewer || null,
              model_slot: slot.trim() || 'default',
              globs: globs.split('\n').map((s) => s.trim()).filter(Boolean),
              skills: skills.split(',').map((s) => s.trim()).filter(Boolean),
            }
            api.saveRoleTemplate(def).then(() => onDone(def)).catch((e) => setErr(errText(e)))
          }}
        >
          {t('agent.createRole')}
        </button>
        <button
          className="btn"
          type="button"
          disabled={!name.trim() || drafting}
          onClick={() => {
            setDrafting(true)
            onBusy(true)
            api.draftRoleDuty(name.trim(), duty || ' ')
              .then((text) => setDuty(text.trim()))
              .catch((e) => setErr(errText(e)))
              .finally(() => {
                setDrafting(false)
                onBusy(false)
              })
          }}
        >
          {t('agent.draftAi')}
        </button>
        {drafting && <LoadingState label={t('wizard.draftingRoles')} />}
      </div>
    </div>
  )
}

/** 答问卡（2026-09-25）：brief_questions 出的题挂右侧并列弹层（同定制弹窗对位），
 *  card-ask 视觉；起草在途（busy）整卡控件锁——敲定钮置灰是 owner 明令。
 *  题数不设上限（owner 2026-09-25）：列表滚动 + 「已答 n/N」进度 + 底部
 *  「还有题」滚动提示——题多时负责人不能看不出下面还有。 */
function BriefQaPanel({ questions, answers, busy, onAnswer, onGenerate, onSkip, onCancel }: {
  questions: BriefQuestion[]
  answers: Record<number, string>
  busy: boolean
  onAnswer: (i: number, v: string) => void
  onGenerate: () => void
  onSkip: () => void
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const allAnswered = questions.every((_, i) => !!(answers[i] ?? '').trim())
  const done = questions.reduce((n, _, i) => n + ((answers[i] ?? '').trim() ? 1 : 0), 0)
  // 底部「还有题」提示：真实溢出才出现（scrollHeight 可探时），滚动到底自动消失
  const listRef = useRef<HTMLDivElement>(null)
  const [moreBelow, setMoreBelow] = useState(false)
  useEffect(() => {
    const check = () => {
      const el = listRef.current
      if (el) setMoreBelow(el.scrollHeight - el.scrollTop - el.clientHeight > 8)
    }
    check()
    window.addEventListener('resize', check)
    return () => window.removeEventListener('resize', check)
  }, [questions.length])
  return (
    <div
      className="panel" role="dialog" aria-label={t('wizard.qaTitle')} data-qa-cards
      style={{
        width: 'min(400px, 90vw)', flexShrink: 0, maxHeight: '86vh',
        display: 'flex', flexDirection: 'column', padding: '14px 16px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <strong style={{ fontSize: 13 }}>{t('wizard.qaTitle')}</strong>
        <span style={{ flex: 1 }} />
        {/* 取消只关题卡不发起起草；在途的起草不受它影响——题卡清了，草稿照常落地 */}
        <button className="btn" type="button" onClick={onCancel}>{t('agent.cancel')}</button>
      </div>
      <div className="dim3" style={{ fontSize: 11, marginTop: 4 }}>{t('wizard.qaHint')}</div>
      <div className="dim3" data-qa-progress style={{ fontSize: 11, marginTop: 4 }}>
        {t('wizard.qaProgress', { done, total: questions.length })}
      </div>
      <div style={{ position: 'relative', flex: 1, minHeight: 0, marginTop: 8, display: 'flex' }}>
        <div
          ref={listRef}
          style={{ flex: 1, overflowY: 'auto', minHeight: 0 }}
          onScroll={() => {
            const el = listRef.current
            if (el) setMoreBelow(el.scrollHeight - el.scrollTop - el.clientHeight > 8)
          }}
        >
          {questions.map((q, i) => (
          <div key={i} className="card-ask" style={{ padding: '8px 10px', marginBottom: 8 }}>
            <div style={{ fontSize: 12, fontWeight: 560 }}>{q.question}</div>
            <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 6 }}>
              {q.options.map((opt) => (
                <button
                  key={opt}
                  type="button"
                  className={answers[i] === opt ? 'btn primary' : 'btn'}
                  style={{ fontSize: 11 }}
                  disabled={busy}
                  onClick={() => onAnswer(i, opt)}
                >
                  {opt}
                </button>
              ))}
            </div>
            <input
              className="input"
              style={{ marginTop: 6, fontSize: 11, textAlign: 'left', width: '100%' }}
              placeholder={t('wizard.qaOther')}
              disabled={busy}
              value={q.options.includes(answers[i] ?? '') ? '' : (answers[i] ?? '')}
              onChange={(e) => onAnswer(i, e.target.value)}
            />
          </div>
        ))}
        </div>
        {moreBelow && (
          <div
            data-qa-more
            aria-hidden
            className="dim3"
            style={{
              position: 'absolute', left: 0, right: 0, bottom: 0, pointerEvents: 'none',
              paddingTop: 24, paddingBottom: 2, textAlign: 'center', fontSize: 11,
              background: 'linear-gradient(transparent, var(--bg-1))',
            }}
          >
            {t('wizard.qaMore')}
          </div>
        )}
      </div>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 8 }}>
        <button
          className="btn primary"
          type="button"
          disabled={busy || !allAnswered}
          onClick={onGenerate}
        >
          {t('wizard.qaGenerate')}
        </button>
        <button className="btn" type="button" disabled={busy} onClick={onSkip}>
          {t('wizard.qaSkip')}
        </button>
        {busy && <LoadingState label={t('wizard.optimizing')} />}
      </div>
    </div>
  )
}

function Chip({ ok, warn, children }: { ok?: boolean; warn?: boolean; children: React.ReactNode }) {
  return (
    <span
      style={{
        fontSize: 11, padding: '2px 8px', borderRadius: 8,
        background: warn ? 'var(--accent-soft)' : ok ? 'var(--ok-soft)' : 'var(--bg-2)',
        color: warn ? 'var(--accent)' : ok ? 'var(--ok)' : 'var(--text-2)',
      }}
    >
      {children}
    </span>
  )
}

export function Wizard({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const [step, setStep] = useState<Step>('providers')
  const [draft, setDraft] = useState<Draft>(loadDraft)
  const [report, setReport] = useState<DirReport | null>(null)
  const [tpls, setTpls] = useState<RoleTemplate[]>([])
  const [makingRole, setMakingRole] = useState(false)
  // 正在定制的角色名（roles 步右侧弹窗——保存才落 override，取消丢弃）
  const [customizing, setCustomizing] = useState<string | null>(null)
  // 全新草稿 = localStorage 无存档；「默认全选」只盖新草稿，不盖存档勾选
  const [freshDraft] = useState(() => localStorage.getItem(DRAFT_KEY) === null)
  const [doc, setDoc] = useState<ProvidersView>({ providers: [], slots: {} })
  const [providersLoaded, setProvidersLoaded] = useState(false)
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({})
  // 票 15：密钥显隐复用 ProviderManager 的 vision 钮模式（每 provider 独立）。
  const [keyShown, setKeyShown] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  // 答问优化流（2026-09-25）：asking=出题中 / drafting=起草中；idle 时若
  // questions 非空则是等负责人答问。出题失败回落直出，答问不挡优化。
  const [briefPhase, setBriefPhase] = useState<'idle' | 'asking' | 'drafting'>('idle')
  const [questions, setQuestions] = useState<BriefQuestion[] | null>(null)
  const [answers, setAnswers] = useState<Record<number, string>>({})
  // 角色步 AI 起草（职责+归属 globs）：只补未定制的勾选角色，已定制的不覆盖。
  const [rolesBusy, setRolesBusy] = useState(false)
  // 流程步 draft_flow 落定标记（成功/失败/空都算落定）。在途=进了流程步还没草稿也没落定——
  // 派生值而非 effect 里同步 setState：set-state-in-effect 会触发级联渲染，oxlint 红线
  const [flowDone, setFlowDone] = useState(false)
  const flowBusy = step === 'flow' && !draft.flowPack && !flowDone
  const [newRoleBusy, setNewRoleBusy] = useState(false)
  // AI 调用在途即锁跳步（owner 2026-09-25：上一步/下一步/步骤轨回跳全锁），
  // 避免草稿还在生成时人已经走开。创建进度是例外：命令在阻塞池不占渲染线程，
  // 「进行中仍可点上一步」是票 14 钉死的既有裁决，busy 不进 aiBusy。
  const aiBusy = briefPhase !== 'idle' || rolesBusy || flowBusy || newRoleBusy
  // 票 14：创建步骤随核的回报点亮。失败停在该步，不进入工作台。
  const [createDone, setCreateDone] = useState<CreateStep[]>([])
  const [createFail, setCreateFail] = useState<{ step: CreateStep; reason: string } | null>(null)

  const set = useCallback(
    (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch })),
    [],
  )

  // 草稿持久化：任何字段变化即写盘，中途退出可续
  useEffect(() => {
    localStorage.setItem(DRAFT_KEY, JSON.stringify(draft))
  }, [draft])

  useEffect(() => {
    api.listRoleTemplates().then((list) => {
      setTpls(list)
      if (freshDraft) set({ roles: list.map((tp) => tp.def.name) })
    }).catch(() => {})
  }, [freshDraft, set])

  // 目录变化 → 重新体检（setTimeout 内统一处理，避免 effect 内同步 setState）
  useEffect(() => {
    const id = setTimeout(() => {
      if (!draft.dir) { setReport(null); return }
      api.inspectDir(draft.dir).then(async (r) => {
        setReport(r)
        const text = r.instructions
          ? await api.readInstructionFile(draft.dir).catch(() => '')
          : ''
        set({ instructionText: text })
      }).catch(() => setReport(null))
    }, 200)
    return () => clearTimeout(id)
  }, [draft.dir])

  // 说明文件草稿：非空目录勾了生成且还没内容时拉骨架。
  // 票 16：空目录走一句话优化，不用无模型时代的骨架冒充草稿。
  useEffect(() => {
    if (!report || report.empty || report.instructions) return
    if (draft.genAgents && !draft.agentsMd && draft.name) {
      api.agentsMdDraft(draft.name).then((md) => set({ agentsMd: md })).catch(() => {})
    }
  }, [draft.genAgents, draft.agentsMd, draft.name, report, set])

  // 说明/目录/名称和上次起草不一致 → AI 职责失效，卡片回到模板，按钮回到「起草」。
  // 人手定制（roleHandTuned）不受影响。不在 set() 里删状态，避免说明骨架
  // 晚到把刚写上的职责清掉（owner 2026-09-25：点了起草界面毫无变化）。
  const aiStale = !!draft.roleDraftKey && draft.roleDraftKey !== projectKey(draft)
  const effDef = useCallback(
    (tp: RoleTemplate) => {
      const ov = draft.roleOverrides[tp.def.name]
      if (!ov) return tp.def
      if (aiStale && draft.roleDrafted.includes(tp.def.name)) return tp.def
      return ov
    },
    [draft.roleOverrides, draft.roleDrafted, aiStale],
  )
  const pickedRoles = useMemo(
    () => tpls.filter((tp) => draft.roles.includes(tp.def.name)),
    [tpls, draft.roles],
  )
  const showRedraft = !aiStale && pickedRoles.some((tp) => draft.roleDrafted.includes(tp.def.name))
  const slots = useMemo(
    () => [...new Set(pickedRoles.map((tp) => effDef(tp).model_slot))],
    [pickedRoles, effDef],
  )

  // 供应商步 / 密钥步拉同一份 providers 文档（绑定 + key 状态）。
  // 轮询而非一次性：从设置页（hexagon:open-settings 覆盖层）返回时状态自刷新。
  // 未读完之前不渲染钥匙框——已配好的机器不该先闪一次「请填钥匙」。
  const recheckKeys = useCallback(() => {
    api.listProviders().then((d) => {
      setDoc(d)
      setProvidersLoaded(true)
    }).catch(() => setProvidersLoaded(true))
  }, [])
  useEffect(() => {
    if (step !== 'keys' && step !== 'providers') return
    const id = setInterval(recheckKeys, 2000)
    recheckKeys()
    return () => clearInterval(id)
  }, [step, recheckKeys])

  // 槽绑定解析：本槽 → default 兜底（同回合内核解析序）；返回绑定+供应商
  const bindingOf = useCallback(
    (slot: string) => {
      const b = doc.slots[slot] ?? doc.slots.default
      if (!b) return null
      const provider = doc.providers.find((p) => p.id === b.provider_id)
      return provider ? { binding: b, provider } : null
    },
    [doc],
  )
  // 槽就绪 = 绑定存在 && 供应商启用 && key 已存
  const slotReady = useCallback(
    (slot: string) => {
      const r = bindingOf(slot)
      return !!(r && r.provider.enabled && r.provider.key_set)
    },
    [bindingOf],
  )
  const unready = slots.filter((s) => !slotReady(s))

  // ADR 0060：脏树不停步（改动保留）。已有工作台状态不能再建，只能打开。
  const dirBlocked =
    !report || !report.exists || report.has_workbench || (!report.is_git && !draft.initGit)
  const canNext = useMemo(() => {
    switch (step) {
      case 'providers': return providerStepReady(doc)
      case 'dir': return !!draft.dir && !!draft.name && !dirBlocked
      case 'roles': return draft.roles.length > 0
      case 'brief': return !!report?.instructions || !!draft.agentsMd.trim() || !!draft.brief.trim()
      case 'flow': return !!draft.flowPack && draft.flowPack.stages.length > 0
      case 'confirm': return !draft.fastPath || !!draft.fastRole
      case 'keys': return unready.length === 0 && slots.length > 0
    }
  }, [step, draft, dirBlocked, unready, slots, doc, report])

  const existingRepoAlign =
    !!report?.is_git && !report?.empty && draft.roles.includes('产品策划')

  async function browse() {
    if (!isTauri) return
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({ directory: true })
    if (typeof picked === 'string') {
      const name = picked.split('/').filter(Boolean).pop() || ''
      set({ dir: picked, name: draft.name || name })
    }
  }

  /// 给已绑定供应商补 key：saveProvider 传 secret 只换 key，不改配置。
  async function saveKey(providerId: string) {
    const secret = keyInputs[providerId]?.trim()
    const provider = doc.providers.find((p) => p.id === providerId)
    if (!secret || !provider) return
    setBusy(true)
    try {
      await api.saveProvider(provider, secret)
      setKeyInputs((k) => ({ ...k, [providerId]: '' }))
      recheckKeys()
    } catch (e) {
      // ui-audit 票 04（P1-6）：saveKey 失败原先只复位 busy、
      // 无任何反馈——「按了没反应」。走 toast 出口。
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  async function openExisting() {
    setBusy(true)
    setErr('')
    try {
      await api.openRecent(draft.dir)
      localStorage.removeItem(DRAFT_KEY)
      onDone()
    } catch (e) {
      setErr(errText(e))
    } finally {
      setBusy(false)
    }
  }

  /// 确认前不写盘。已有 AGENTS.md / CLAUDE.md 时连草稿也不提交（不覆盖）。
  function agentsToWrite(): string | null {
    if (report?.instructions || !draft.agentsMd.trim()) return null
    if (report?.empty || draft.genAgents) return draft.agentsMd
    return null
  }

  /// 起草 AGENTS.md 本体。qa=答问卡收齐的答案（可空）。草稿只进文本框。
  async function runOptimize(qa: BriefQA[]) {
    const sentence = draft.brief.trim()
    if (!sentence || report?.instructions) return
    setBriefPhase('drafting')
    setErr('')
    try {
      const md = await api.optimizeAgentsMd(draft.name, sentence, qa)
      set({ agentsMd: md, genAgents: true })
      setQuestions(null)
      setAnswers({})
    } catch (e) {
      setErr(errText(e))
    } finally {
      setBriefPhase('idle')
    }
  }

  /// 优化入口：先让模型出题（grill 式答问卡），有题先答再起草；
  /// 没题可问 / 出题失败 → 直出草稿——答问是增益不是门槛。
  async function optimizeBrief() {
    const sentence = draft.brief.trim()
    // 已有说明文件不提供优化——那是覆盖入口。
    if (!sentence || report?.instructions || briefPhase !== 'idle') return
    setErr('')
    setBriefPhase('asking')
    let qs: BriefQuestion[] = []
    try {
      qs = await api.briefQuestions(draft.name, sentence)
    } catch {
      qs = []
    }
    const answerable = qs.filter((q) => q.question.trim() && q.options.length > 0)
    if (answerable.length === 0) {
      await runOptimize([])
      return
    }
    setQuestions(answerable)
    setAnswers({})
    setBriefPhase('idle')
  }

  /// 答问卡敲定：把答齐的题打包成 BriefQA 起草；未答的题不携带。
  /// 起草在途（busy 置灰在界面层已拦，这层防连点竞态）。
  function generateWithAnswers() {
    if (!questions || briefPhase !== 'idle') return
    const qa = questions.flatMap((q, i) => {
      const a = (answers[i] ?? '').trim()
      return a ? [{ question: q.question, answer: a }] : []
    })
    runOptimize(qa)
  }

  /// 角色步 AI 起草：项目说明 → 每个勾选角色在本项目的职责段落。
  /// 只有 roleHandTuned（定制卡保存、新建角色）不入参不覆盖。
  /// 卡上已有的旧职责（哪怕 roleDrafted 只记下产品策划）一律重写。
  /// 结果落 roleOverrides，与「定制」同一条可编辑通道。
  /// ADR 0075：起草只落 duty——globs 不产也不覆盖，模板生效值保留。
  async function draftRoleSeeds() {
    // 入参用模板定义。上一轮项目的职责段落不再附进提示词——附上去之后
    // 模型只改第一项，其余原样抄回（owner 2026-09-25：魂斗罗只出现在产品策划，
    // 其他角色仍是俄罗斯方块）。
    const human = (name: string) => draft.roleHandTuned.includes(name)
    const targets = pickedRoles.filter((tp) => !human(tp.def.name)).map((tp) => tp.def)
    if (rolesBusy || targets.length === 0) return
    setErr('')
    setRolesBusy(true)
    try {
      let instructionText = draft.instructionText
      let source = draftSource(draft)
      if (!source && report?.instructions && draft.dir) {
        source = await api.readInstructionFile(draft.dir).catch(() => '')
        instructionText = source
      }
      if (!source.trim()) {
        setErr(t('wizard.aiDraftNoBrief'))
        return
      }
      const oldAi = new Map(
        targets.map((tp) => [tp.name, draft.roleOverrides[tp.name]?.duty ?? '']),
      )
      let seeds = await api.draftRoleDefs(source, targets)
      const echoed = targets.filter((tp) => {
        const duty = seeds.find((s) => s.name === tp.name)?.duty.trim() ?? ''
        const prev = oldAi.get(tp.name)
        return !duty || (prev != null && prev !== '' && duty === prev)
      })
      if (echoed.length > 0) {
        const again = await api.draftRoleDefs(source, echoed)
        const seen = new Set(again.map((s) => s.name))
        seeds = [...seeds.filter((s) => !seen.has(s.name)), ...again]
      }
      // 先丢掉 AI 旧稿再写入。模型没给、或原样抄回上一项目的，不保留。
      const next = { ...draft.roleOverrides }
      for (const tp of targets) delete next[tp.name]
      const nextDrafted = new Set<string>()
      let changed = 0
      for (const s of seeds) {
        const tp = tpls.find((x) => x.def.name === s.name)
        if (!tp || !draft.roles.includes(s.name) || human(s.name)) continue
        const duty = s.duty.trim()
        const prev = oldAi.get(s.name)
        if (!duty || (prev != null && prev !== '' && duty === prev)) continue
        if (duty !== prev) changed++
        next[s.name] = { ...tp.def, duty }
        nextDrafted.add(s.name)
      }
      if (changed > 0) {
        set({
          roleOverrides: next,
          roleDrafted: [...nextDrafted],
          roleDraftKey: projectKey({ ...draft, instructionText }),
          instructionText,
        })
        setErr(t('wizard.aiDraftApplied', { n: changed }))
      } else {
        setErr(t('wizard.aiDraftSame'))
      }
    } catch (e) {
      setErr(errText(e))
    } finally {
      setRolesBusy(false)
    }
  }

  async function launch() {
    setBusy(true)
    setCreateDone([])
    setCreateFail(null)
    const seen: CreateStep[] = []
    let failed = false
    const note = (step: CreateStep) => {
      if (failed || seen.includes(step)) return
      seen.push(step)
      setCreateDone([...seen])
    }
    try {
      await api.createProject({
        dir: draft.dir,
        name: draft.name,
        roles: draft.roles,
        // 全部勾选角色传生效定义：自定义模板与定制项经 override 链物化，
        // 内置模板同名直传也无损（定义等价）。
        roleOverrides: pickedRoles.map(effDef),
        packName: null,
        pack: draft.fastPath ? null : draft.flowPack,
        fastpathRole: draft.fastPath ? draft.fastRole : null,
        initGit: draft.initGit,
        agentsMd: agentsToWrite(),
      }, note)
      localStorage.removeItem(DRAFT_KEY)
      onDone()
    } catch (e) {
      // 命令拒绝 = 没打开。已报告的「打开项目」也撤掉，避免假成功。
      failed = true
      const applied = applyCreateFailure(seen, asCmdError(e).code)
      setCreateDone(applied.done)
      setCreateFail({ step: applied.failed, reason: errText(e) })
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    if (step !== 'flow' || draft.flowPack) return
    let cancelled = false
    // draft_flow 也是 AI 调用：在途由 flowBusy 派生锁跳步，done() 在异步出口落定
    const done = () => { if (!cancelled) setFlowDone(true) }
    const apply = (sentence: string) => {
      const text = sentence.trim()
      if (!text) { done(); return }
      api.draftFlow(text).then((pack) => {
        if (!cancelled) set({ flowPack: pack })
      }).catch((e) => { if (!cancelled) setErr(errText(e)) })
        .finally(done)
    }
    const sentence = draft.agentsMd.trim() || draft.brief.trim()
    if (sentence) apply(sentence)
    else if (report?.instructions && draft.dir) {
      api.readInstructionFile(draft.dir).then((text) => {
        if (!cancelled) apply(text)
      }).catch(() => { if (!cancelled) apply(draft.name) })
    } else apply(draft.name)
    return () => { cancelled = true }
  }, [step, draft.flowPack, draft.agentsMd, draft.brief, draft.name, draft.dir, report, set])

  const idx = STEPS.indexOf(step)
  // 跳步即收起侧弹卡——未保存编辑按「取消」语义丢弃（新建角色卡同此；
  // 其表单本就随步骤卸载丢内容，这里把标志位也清掉语义一致）；
  // err 是步内局部状态（brief 优化 / roles 起草），跳步清掉不串台
  const go = (s: Step) => {
    setCustomizing(null)
    setMakingRole(false)
    setErr('')
    // 再进 flow 步要重试 draft_flow——上次落定标记清掉，flowBusy 派生跟着重立
    if (s === 'flow' && !draft.flowPack) setFlowDone(false)
    setStep(s)
  }
  // 定制弹窗只活在 roles 步；角色被卸掉或模板失踪时自动关
  const customizingTp =
    step === 'roles' && customizing && draft.roles.includes(customizing)
      ? tpls.find((x) => x.def.name === customizing)
      : undefined
  const body: Record<Step, React.ReactNode> = {
    providers: (
      <ProviderFirstStep doc={doc} loaded={providersLoaded} onRefresh={recheckKeys} />
    ),
    dir: (
      <>
        <label className="dim3" style={{ fontSize: 11 }}>{t('wizard.dirLabel')}</label>
        <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
          <input
            className="input"
            style={{ flex: 1, textAlign: 'left' }}
            value={draft.dir}
            placeholder={t('wizard.dirPlaceholder')}
            onChange={(e) => set({ dir: e.target.value })}
          />
          {isTauri && (
            <button className="btn" onClick={browse}>{t('wizard.browse')}</button>
          )}
        </div>
        <div style={{ marginTop: 10 }}>
          <label className="dim3" style={{ fontSize: 11 }}>{t('wizard.nameLabel')}</label>
          <input
            className="input"
            style={{ width: '100%', marginTop: 4, textAlign: 'left' }}
            value={draft.name}
            onChange={(e) => set({ name: e.target.value })}
          />
        </div>
        {report && (
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 10 }}>
            <Chip ok={report.is_git}>{report.is_git ? t('wizard.isGit') : t('wizard.noGit')}</Chip>
            {report.empty && <Chip>{t('wizard.emptyDir')}</Chip>}
            {report.dirty && <Chip>{t('wizard.dirty')}</Chip>}
            {report.has_workbench && <Chip>{t('wizard.hasWorkbench')}</Chip>}
            {report.instructions && <Chip ok>{report.instructions}</Chip>}
          </div>
        )}
        {report?.dirty && (
          <div className="dim3" style={{ fontSize: 12, marginTop: 8 }}>{t('wizard.dirtyHint')}</div>
        )}
        {report?.has_workbench && (
          <div style={{ marginTop: 10 }}>
            <div style={{ fontSize: 12, marginBottom: 8 }}>{t('wizard.openInstead')}</div>
            <button className="btn primary" disabled={busy} onClick={openExisting}>
              {t('wizard.openProject')}
            </button>
          </div>
        )}
        {err && step === 'dir' && (
          <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 10 }}>{err}</div>
        )}
        {report && !report.is_git && !report.has_workbench && (
          <label style={{ display: 'flex', gap: 6, alignItems: 'center', marginTop: 10, fontSize: 12 }}>
            <input
              type="checkbox"
              checked={draft.initGit}
              onChange={(e) => set({ initGit: e.target.checked })}
            />
            {t('wizard.initGit')}
          </label>
        )}
      </>
    ),
    roles: (
      <>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
          <div className="dim3" style={{ fontSize: 11 }}>{t('wizard.rolesHint')}</div>
          <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
            <button
              className="btn"
              data-ai-draft
              type="button"
              disabled={
                rolesBusy ||
                pickedRoles.every((tp) => draft.roleHandTuned.includes(tp.def.name)) ||
                !(draft.agentsMd.trim() || draft.brief.trim() || report?.instructions)
              }
              title={
                draft.agentsMd.trim() || draft.brief.trim() || report?.instructions
                  ? t('wizard.aiDraftHint')
                  : t('wizard.aiDraftNoBrief')
              }
              onClick={draftRoleSeeds}
            >
              {t(showRedraft ? 'wizard.aiRedraft' : 'wizard.aiDraft')}
            </button>
            {rolesBusy && <LoadingState label={t('wizard.draftingRoles')} />}
            <button
              className="btn" data-new-role type="button"
              onClick={() => {
                // 侧弹卡互斥：新建卡与定制卡同属右侧槽位，开一个关另一个
                setCustomizing(null)
                setMakingRole((v) => !v)
              }}
            >
              {t('agent.createRole')}
            </button>
          </div>
        </div>
        <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('wizard.allDefault')}</div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
          {tpls.map((tp) => {
            const picked = draft.roles.includes(tp.def.name)
            const customized = !!draft.roleOverrides[tp.def.name]
            // 按钮不能嵌进 <label>：label 的激活行为会把点击转发给 checkbox，
            // 「定制」会顺手把卡卸掉（jsdom 与浏览器表现不一致的坑）
            return (
              <div
                key={tp.def.name}
                className="panel"
                style={{
                  display: 'flex', gap: 8, padding: '8px 10px',
                  outline: picked ? '1px solid var(--accent)' : undefined,
                }}
              >
                <label style={{ display: 'flex', gap: 8, flex: 1, minWidth: 0, cursor: 'pointer' }}>
                  <input
                    type="checkbox"
                    checked={picked}
                    onChange={(e) =>
                      set({
                        roles: e.target.checked
                          ? [...draft.roles, tp.def.name]
                          : draft.roles.filter((x) => x !== tp.def.name),
                      })
                    }
                  />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 12, fontWeight: 510 }}>
                      {tp.def.name}
                      {tp.origin === 'custom' && (
                        <span className="chip ok" style={{ fontSize: 9, marginLeft: 6 }}>{t('teamTpl.custom')}</span>
                      )}
                      {customized && (
                        <span className="chip" style={{ fontSize: 9, marginLeft: 6 }}>{t('wizard.customized')}</span>
                      )}
                    </div>
                    {/* 职责只显两行，段落式职责（ADR 0075）全文经「定制」卡查看 */}
                    <div
                      className="dim3"
                      title={effDef(tp).duty}
                      style={{
                        fontSize: 11,
                        display: '-webkit-box',
                        WebkitLineClamp: 2,
                        WebkitBoxOrient: 'vertical',
                        overflow: 'hidden',
                      }}
                    >
                      {effDef(tp).duty}
                    </div>
                  </div>
                </label>
                {picked && (
                  <button
                    className="btn"
                    type="button"
                    style={{ fontSize: 10, alignSelf: 'flex-start' }}
                    onClick={() => {
                      // 侧弹卡互斥（同新建卡）
                      setMakingRole(false)
                      setCustomizing(customizing === tp.def.name ? null : tp.def.name)
                    }}
                  >
                    {t('wizard.customize')}
                  </button>
                )}
              </div>
            )
          })}
        </div>
        {err && step === 'roles' && (
          <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 10 }}>{err}</div>
        )}
      </>
    ),
    flow: (
      <>
        <FlowDraft
          pack={draft.flowPack}
          roles={draft.roles}
          onChange={(flowPack) => set({ flowPack })}
        />
        {err && step === 'flow' && (
          <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 10 }}>{err}</div>
        )}
      </>
    ),
    brief: (
      <>
        {report?.instructions ? (
          <div style={{ fontSize: 12 }}>
            <Chip ok>{report.instructions}</Chip>
            <div className="dim3" style={{ marginTop: 8, fontSize: 12 }}>
              {t('wizard.instructionsFound', { file: report.instructions })}
            </div>
          </div>
        ) : report?.empty ? (
          <>
            <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('wizard.briefHint')}</div>
            <textarea
              className="input"
              style={{ width: '100%', height: 72, textAlign: 'left', fontSize: 12, resize: 'vertical' }}
              placeholder={t('wizard.briefPlaceholder')}
              value={draft.brief}
              disabled={briefPhase !== 'idle'}
              onChange={(e) => {
                set({ brief: e.target.value })
                // 一句话改了，旧答案针对的题作废——重按优化重新出题
                if (questions) {
                  setQuestions(null)
                  setAnswers({})
                }
              }}
            />
            <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginTop: 8 }}>
              <button
                className="btn"
                disabled={busy || briefPhase !== 'idle' || !draft.brief.trim()}
                onClick={optimizeBrief}
              >
                {t('wizard.optimize')}
              </button>
              {briefPhase !== 'idle' && (
                <LoadingState
                  label={t(briefPhase === 'asking' ? 'wizard.asking' : 'wizard.optimizing')}
                />
              )}
            </div>
            {/* 答问卡挪右侧并列弹层（BriefQaPanel，同定制弹窗对位）——
                主面板只留一句指引，作答全部在侧卡 */}
            {questions && questions.length > 0 && (
              <div className="dim3" style={{ fontSize: 11, marginTop: 8 }}>{t('wizard.qaSide')}</div>
            )}
            {draft.agentsMd && (
              <>
                <div className="dim3" style={{ fontSize: 11, margin: '8px 0 4px' }}>
                  {t('wizard.agentsDraftHint')}
                </div>
                <textarea
                  className="input"
                  style={{ width: '100%', height: 180, textAlign: 'left', fontFamily: 'monospace', fontSize: 11, resize: 'vertical' }}
                  value={draft.agentsMd}
                  disabled={briefPhase !== 'idle'}
                  onChange={(e) => set({ agentsMd: e.target.value })}
                />
              </>
            )}
            {err && <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 8 }}>{err}</div>}
          </>
        ) : (
          <>
            <label style={{ display: 'flex', gap: 6, alignItems: 'center', fontSize: 12 }}>
              <input
                type="checkbox"
                checked={draft.genAgents}
                onChange={(e) => set({ genAgents: e.target.checked })}
              />
              {t('wizard.genAgents')}
            </label>
            {draft.genAgents && (
              <>
                <div className="dim3" style={{ fontSize: 11, margin: '8px 0 4px' }}>
                  {t('wizard.agentsDraftHint')}
                </div>
                <textarea
                  className="input"
                  style={{ width: '100%', height: 180, textAlign: 'left', fontFamily: 'monospace', fontSize: 11, resize: 'vertical' }}
                  value={draft.agentsMd}
                  onChange={(e) => set({ agentsMd: e.target.value })}
                />
              </>
            )}
          </>
        )}
      </>
    ),
    keys: (
      <>
        <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('wizard.keysHint')}</div>
        {slots.map((slot) => {
          const r = bindingOf(slot)
          const ready = slotReady(slot)
          const viaDefault = !doc.slots[slot] && !!doc.slots.default
          const pid = r?.provider.id ?? ''
          return (
            <div key={slot} style={{ padding: '8px 0', borderBottom: '1px solid var(--border)' }}>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <div style={{ flex: 1 }}>
                  <code style={{ fontSize: 12 }}>model/{slot}</code>
                  <div className="dim3" style={{ fontSize: 11 }}>
                    {pickedRoles.filter((tp) => effDef(tp).model_slot === slot).map((tp) => tp.def.name).join('、')}
                    {r && ` · ${r.provider.name} · ${r.binding.model}`}
                    {r && viaDefault && ` · ${t('wizard.viaDefault')}`}
                  </div>
                </div>
                {ready && <Chip ok>{t('wizard.keyOk')}</Chip>}
                {r && !r.provider.enabled && <Chip warn>{t('providers.disabledTag')}</Chip>}
                {r && r.provider.enabled && !r.provider.key_set && (
                  <>
                    <input
                      className="input"
                      type={keyShown.has(pid) ? 'text' : 'password'}
                      style={{ width: 180 }}
                      placeholder={t('wizard.keyPlaceholder')}
                      value={keyInputs[pid] ?? ''}
                      onChange={(e) =>
                        setKeyInputs((k) => ({ ...k, [pid]: e.target.value }))
                      }
                    />
                    <button
                      className="btn"
                      title={t(keyShown.has(pid) ? 'providers.hideKey' : 'providers.showKey')}
                      onClick={() =>
                        setKeyShown((s) => {
                          const n = new Set(s)
                          if (n.has(pid)) n.delete(pid); else n.add(pid)
                          return n
                        })
                      }
                    >
                      <Icon name={keyShown.has(pid) ? 'vision' : 'vision-off'} size={12} />
                    </button>
                    <button className="btn" disabled={busy} onClick={() => saveKey(pid)}>
                      {t('wizard.keySave')}
                    </button>
                  </>
                )}
              </div>
              {!r && (
                <div className="dim3" style={{ fontSize: 11, marginTop: 6 }}>
                  {t('wizard.noProvider', { slot })}
                </div>
              )}
            </div>
          )
        })}
        {unready.length > 0 && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginTop: 10 }}>
            <button className="btn primary" onClick={() => api.openSettings().catch(() => {})}>
              {t('wizard.goSettings')}
            </button>
            <span style={{ color: 'var(--accent)', fontSize: 12 }}>{t('wizard.keysBlocked')}</span>
          </div>
        )}
      </>
    ),
    confirm: (
      <>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, fontSize: 12 }}>
          <div><span className="dim3">{t('wizard.sumDir')}：</span>{draft.dir}</div>
          <div><span className="dim3">{t('wizard.sumName')}：</span>{draft.name}</div>
          <div>
            <span className="dim3">{t('wizard.sumRoles')}：</span>
            {draft.roles.map((n) => (draft.roleOverrides[n] ? `${n}*` : n)).join('、')}
            {Object.keys(draft.roleOverrides).some((n) => draft.roles.includes(n)) && (
              <span className="dim3" style={{ fontSize: 10 }}> {t('wizard.customMark')}</span>
            )}
          </div>
          <div>
            <span className="dim3">{t('wizard.sumMode')}：</span>
            {draft.fastPath
              ? `${t('wizard.fastMode')} · ${draft.fastRole}`
              : (draft.flowPack?.name || t('wizard.flowDraft'))}
          </div>
          <label style={{ display: 'flex', gap: 6, alignItems: 'center', marginTop: 8 }}>
            <input
              type="checkbox"
              data-fast-path
              checked={draft.fastPath}
              onChange={(e) => set({ fastPath: e.target.checked, mode: e.target.checked ? 'fastpath' : 'pack' })}
            />
            {t('wizard.fastOnConfirm')}
          </label>
          {draft.fastPath && (
            <select
              className="input"
              style={{ width: 'auto', minWidth: 200, marginTop: 6 }}
              value={draft.fastRole}
              onChange={(e) => set({ fastRole: e.target.value })}
            >
              <option value="">{t('wizard.fastPick')}</option>
              {pickedRoles.map((tp) => (
                <option key={tp.def.name} value={tp.def.name}>{tp.def.name}</option>
              ))}
            </select>
          )}
          <div>
            <span className="dim3">{t('wizard.sumAgents')}：</span>
            {report?.instructions
              ? report.instructions
              : draft.agentsMd.trim() && (report?.empty || draft.genAgents)
                ? t('wizard.sumAgentsGen')
                : t('wizard.sumAgentsNone')}
          </div>
        </div>
        {existingRepoAlign && (
          <div className="panel" style={{ marginTop: 12, padding: '8px 10px', fontSize: 12, color: 'var(--flag)' }}>
            {t('wizard.alignHint')}
          </div>
        )}
        {(busy || createDone.length > 0 || createFail) && (
          <ul
            data-testid="create-progress"
            style={{ listStyle: 'none', padding: 0, margin: '12px 0 0' }}
          >
            {CREATE_STEPS.map((s) => {
              const done = createDone.includes(s)
              const failed = createFail?.step === s
              const running = busy && !createFail && !done
                && CREATE_STEPS.find((x) => !createDone.includes(x)) === s
              const state = failed ? 'failed' : done ? 'done' : running ? 'running' : 'pending'
              return (
                <li
                  key={s}
                  data-create-step={s}
                  data-state={state}
                  style={{
                    fontSize: 12, padding: '3px 0',
                    color: failed ? 'var(--err)' : done ? 'var(--ok)' : 'var(--text-2)',
                  }}
                >
                  <span style={{ display: 'inline-block', width: 16 }}>
                    {failed ? '✗' : done ? '✓' : running ? '…' : '○'}
                  </span>
                  {t(`wizard.create.${s}`)}
                  {failed && (
                    <div style={{ marginLeft: 16, color: 'var(--err)' }}>{createFail.reason}</div>
                  )}
                </li>
              )
            })}
          </ul>
        )}
      </>
    ),
  }

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 60, display: 'flex',
        alignItems: 'center', justifyContent: 'center',
        background: 'var(--bg)',
      }}
    >
      {/* 并列对：stretch 让定制弹窗与向导同高、两者整体居中；无弹窗时向导独自居中 */}
      <div style={{ display: 'flex', gap: 12, alignItems: 'stretch', maxWidth: '96vw' }}>
      {/* 票 15：窄窗不溢出——min(560px, 92vw) */}
      <div className="panel" style={{ width: 'min(560px, 92vw)', maxHeight: '86vh', display: 'flex', flexDirection: 'column', padding: '20px 22px' }}>
        <div style={{ fontWeight: 600, fontSize: 15 }}>{t('wizard.title')}</div>
        {/* 步骤轨（票 15：已完成步可点回跳；未来步不可点——跳步会绕过 canNext 校验） */}
        <div style={{ display: 'flex', gap: 4, margin: '12px 0 16px' }}>
          {STEPS.map((s, i) => {
            // AI 在途（aiBusy）时回跳也锁——与底部 Back 同一闸
            const jumpable = i < idx && !aiBusy
            return (
            <div
              key={s}
              role={jumpable ? 'button' : undefined}
              tabIndex={jumpable ? 0 : undefined}
              title={t(`wizard.step.${s}`)}
              onClick={jumpable ? () => go(s) : undefined}
              onKeyDown={jumpable ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(s) } } : undefined}
              style={{
                flex: 1, height: 3, borderRadius: 2,
                background: i <= idx ? 'var(--accent)' : 'var(--bg-2)',
                cursor: jumpable ? 'pointer' : 'default',
              }}
            />
            )
          })}
        </div>
        <div style={{ fontSize: 12, fontWeight: 510, marginBottom: 8 }}>
          {t(`wizard.step.${step}`)}
        </div>
        {/* 滚动体留 4px 呼吸位：focus 环(2+2)与选中描边在滚口边缘不被裁 */}
        <div style={{ flex: 1, overflowY: 'auto', minHeight: 0, padding: 4, margin: -4 }}>{body[step]}</div>
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 16 }}>
          {idx > 0 && (
            <button className="btn" disabled={aiBusy} onClick={() => go(STEPS[idx - 1])}>
              {t('wizard.back')}
            </button>
          )}
          {step === 'confirm' ? (
            <button className="btn primary" disabled={busy} onClick={launch}>
              {t('wizard.launch')}
            </button>
          ) : (
            <button
              className="btn primary"
              disabled={!canNext || aiBusy}
              onClick={() => go(STEPS[idx + 1])}
            >
              {t('wizard.next')}
            </button>
          )}
        </div>
      </div>
      {/* 侧弹槽：定制/新建角色/答问卡共用右侧并列位，与向导整体居中；
          保存/取消关掉后向导自动回中。互斥由开关各自保证（开一个关另一个） */}
      {customizingTp && (
        <RoleCustomize
          key={customizingTp.def.name}
          def={effDef(customizingTp)}
          tplDef={customizingTp.def}
          names={tpls.map((x) => x.def.name)}
          doc={doc}
          onSave={(def) => {
            const next = { ...draft.roleOverrides }
            // 与模板同形不写 override——「已定制」chip 只标真实差异
            if (JSON.stringify(def) === JSON.stringify(customizingTp.def)) delete next[def.name]
            else next[def.name] = def
            // 人手保存即转人手：移出 AI 起草名单，之后起草不再覆盖它
            const tuned = draft.roleHandTuned.filter((n) => n !== def.name)
            set({
              roleOverrides: next,
              roleDrafted: draft.roleDrafted.filter((n) => n !== def.name),
              roleHandTuned: next[def.name] ? [...tuned, def.name] : tuned,
            })
            setCustomizing(null)
          }}
          onCancel={() => setCustomizing(null)}
        />
      )}
      {step === 'roles' && makingRole && (
        <NewRoleForm
          names={tpls.map((tp) => tp.def.name)}
          onBusy={setNewRoleBusy}
          onCancel={() => setMakingRole(false)}
          onDone={(def) => {
            setTpls((list) => [...list.filter((tp) => tp.def.name !== def.name), { def, origin: 'custom' }])
            set({
              roles: draft.roles.includes(def.name) ? draft.roles : [...draft.roles, def.name],
              roleOverrides: { ...draft.roleOverrides, [def.name]: def },
              // 新建卡落的 override 是人手定义，不入 AI 起草名单
              roleDrafted: draft.roleDrafted.filter((n) => n !== def.name),
              roleHandTuned: draft.roleHandTuned.includes(def.name)
                ? draft.roleHandTuned
                : [...draft.roleHandTuned, def.name],
            })
            setMakingRole(false)
          }}
        />
      )}
      {step === 'brief' && questions && questions.length > 0 && (
        <BriefQaPanel
          questions={questions}
          answers={answers}
          busy={briefPhase !== 'idle'}
          onAnswer={(i, v) => setAnswers((a) => ({ ...a, [i]: v }))}
          onGenerate={generateWithAnswers}
          onSkip={() => runOptimize([])}
          onCancel={() => {
            setQuestions(null)
            setAnswers({})
          }}
        />
      )}
      </div>
    </div>
  )
}

/** 向导内角色定制（ADR 0057）：右侧弹窗与向导并列居中。编辑走本地副本——
 *  「保存」才经 onSave 写 roleOverrides（与模板同形由调用方清掉）、「取消」整份丢弃。
 *  改出的 RoleDef 只进本项目，不回写模板库。字段：职责/上级/模型槽/归属路径/技能。 */
function RoleCustomize({ def, tplDef, names, doc, onSave, onCancel }: {
  def: RoleDef
  tplDef: RoleDef
  names: string[]
  doc: ProvidersView
  onSave: (def: RoleDef) => void
  onCancel: () => void
}) {
  const { t } = useTranslation()
  // 表单控件走 .input 原语——此前手搓了一份同款内联样式，收编后 focus/disabled 态随原语走
  const inputSm: React.CSSProperties = { fontSize: 12 }
  const lbl: React.CSSProperties = { fontSize: 11, fontWeight: 560, color: 'var(--text-3)', marginTop: 10 }
  const [edit, setEdit] = useState(def)
  const upd = (patch: Partial<RoleDef>) => setEdit((d) => ({ ...d, ...patch }))
  // ADR 0075：归属 globs 收进高级选项——留空 = 不限制写入范围。
  const [adv, setAdv] = useState(false)

  return (
    <div
      className="panel" role="dialog" aria-label={def.name} data-role-customize
      style={{
        width: 'min(400px, 90vw)', flexShrink: 0, maxHeight: '86vh',
        display: 'flex', flexDirection: 'column', padding: '14px 16px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <strong style={{ fontSize: 13 }}>{def.name}</strong>
        <span className="dim3" style={{ fontSize: 11 }}>{t('wizard.scopeHint')}</span>
      </div>
      <div style={{ flex: 1, overflowY: 'auto', minHeight: 0, marginTop: 4 }}>
        <div style={lbl}>{t('agent.duty')}</div>
        <textarea className="input" value={edit.duty} rows={10} style={{ ...inputSm, resize: 'vertical' }}
          onChange={(e) => upd({ duty: e.target.value })} />
        <div style={{ display: 'flex', gap: 8 }}>
          <div style={{ flex: 1 }}>
            <div style={lbl}>{t('agent.reviewer')}</div>
            <select className="input" value={edit.reviewer ?? ''} style={inputSm}
              onChange={(e) => upd({ reviewer: e.target.value || null })}>
              <option value="">{t('agent.noReviewer')}</option>
              {names.filter((n) => n !== def.name).map((n) => (
                <option key={n} value={n}>{n}</option>
              ))}
            </select>
          </div>
          <div style={{ flex: 1 }}>
            <div style={lbl}>{t('agent.modelSlot')}</div>
            <select className="input" value={edit.model_slot} style={inputSm}
              onChange={(e) => upd({ model_slot: e.target.value })}>
              {sharedSlots(doc.slots, edit.model_slot).map((s) => (
                <option key={s} value={s}>{slotLabel(s, doc, t('agent.dedicatedTag'))}</option>
              ))}
            </select>
          </div>
        </div>
        <button
          type="button" className="btn" data-globs-toggle aria-expanded={adv}
          style={{ fontSize: 11, marginTop: 10, padding: '2px 8px', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          onClick={() => setAdv((v) => !v)}
        >
          <Icon name={adv ? 'chevron-down' : 'chevron-right'} size={9} />
          {t('agent.advanced')}
        </button>
        {adv && (
          <>
            <div className="dim3" style={{ fontSize: 11, marginTop: 4 }}>{t('agent.globsEmpty')}</div>
            <div style={{ ...lbl, marginTop: 4 }}>{t('agent.globs')}</div>
            <textarea className="input" value={edit.globs.join('\n')} rows={8} placeholder="src/**"
              style={{ ...inputSm, fontFamily: 'monospace', resize: 'vertical' }}
              onChange={(e) => upd({ globs: e.target.value.split('\n').map((s) => s.trim()).filter(Boolean) })} />
          </>
        )}
        <div style={lbl}>{t('agent.skills')}</div>
        <EntityChips value={edit.skills} onChange={(ids) => upd({ skills: ids })} source="skills" />
      </div>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 12 }}>
        <button className="btn" type="button" style={{ fontSize: 11 }} onClick={() => setEdit(tplDef)}>
          {t('wizard.resetTpl')}
        </button>
        <span style={{ flex: 1 }} />
        <button className="btn" type="button" onClick={onCancel}>{t('agent.cancel')}</button>
        <button className="btn primary" type="button" onClick={() => onSave(edit)}>{t('agent.saveRole')}</button>
      </div>
    </div>
  )
}

// 与 ProviderManager 同一套 slug，避免两个界面对同一名字写出不同 id。
const providerSlug = (s: string) =>
  s.trim().toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, '-').replace(/^-+|-+$/g, '') || `p${Date.now()}`

function suggestedModel(doc: ProvidersView, p?: ProviderView): string {
  if (!p) return ''
  const bound = doc.slots.default
  if (bound?.provider_id === p.id && bound.model.trim()) return bound.model
  return p.models[0]?.id ?? ''
}

function initialProviderId(doc: ProvidersView): string {
  const bound = doc.slots.default?.provider_id
  if (bound && doc.providers.some((p) => p.id === bound)) return bound
  return doc.providers.find((p) => p.enabled && p.key_set)?.id ?? doc.providers[0]?.id ?? ''
}

/** 票 13：向导第一步。数据只走 list/save/set_slot_binding，不另起一份供应商存储。
 *  已就绪不渲染钥匙框；缺钥匙才要输入。 */
function ProviderFirstStep({ doc, loaded, onRefresh }: {
  doc: ProvidersView
  loaded: boolean
  onRefresh: () => void
}) {
  const { t } = useTranslation()
  const ready = loaded && providerStepReady(doc)
  const binding = doc.slots.default
  const bound = binding ? doc.providers.find((p) => p.id === binding.provider_id) : undefined
  return (
    <>
      <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('wizard.providersHint')}</div>
      {!loaded ? null : ready && bound && binding ? (
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Chip ok>{t('wizard.providersReady')}</Chip>
          <span style={{ fontSize: 12 }}>{bound.name} · {binding.model}</span>
        </div>
      ) : (
        <>
          <div style={{ color: 'var(--accent)', fontSize: 12, marginBottom: 8 }}>{t('wizard.providersBlocked')}</div>
          {doc.providers.length === 0
            ? <NewProviderForm onRefresh={onRefresh} />
            : <FixProviderForm doc={doc} onRefresh={onRefresh} />}
          <div style={{ marginTop: 10 }}>
            <button className="btn" onClick={() => api.openSettings().catch(() => {})}>
              {t('wizard.goSettings')}
            </button>
          </div>
        </>
      )}
    </>
  )
}

function NewProviderForm({ onRefresh }: { onRefresh: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [kind, setKind] = useState<ProviderView['kind']>('openai')
  const [baseUrl, setBaseUrl] = useState('')
  const [secret, setSecret] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [model, setModel] = useState('')
  const [busy, setBusy] = useState(false)
  const canApply = !!name.trim() && !!baseUrl.trim() && !!secret.trim() && !!model.trim()
  const input: React.CSSProperties = { width: '100%', marginTop: 4, textAlign: 'left' }

  async function apply() {
    if (!canApply) return
    const id = providerSlug(name)
    setBusy(true)
    try {
      await api.saveProvider({
        id,
        name: name.trim(),
        kind,
        base_url: baseUrl.trim(),
        models: [],
        enabled: true,
      }, secret.trim())
      await api.setSlotBinding('default', id, model.trim())
      onRefresh()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <label className="dim3" style={{ fontSize: 11 }}>{t('providers.name')}</label>
      <input className="input" style={input} placeholder={t('providers.name')} value={name}
        onChange={(e) => setName(e.target.value)} />
      <label className="dim3" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>{t('providers.kind')}</label>
      <select className="input" style={input} value={kind}
        onChange={(e) => setKind(e.target.value as ProviderView['kind'])}>
        <option value="openai">openai</option>
        <option value="anthropic">anthropic</option>
      </select>
      <label className="dim3" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>{t('providers.baseUrl')}</label>
      <input className="input" style={input} placeholder={t('providers.baseUrl')} value={baseUrl}
        onChange={(e) => setBaseUrl(e.target.value)} />
      <label className="dim3" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>{t('providers.key')}</label>
      <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
        <input className="input" type={showKey ? 'text' : 'password'} style={{ flex: 1, textAlign: 'left' }}
          placeholder={t('wizard.keyPlaceholder')} value={secret}
          onChange={(e) => setSecret(e.target.value)} />
        <button className="btn" title={t(showKey ? 'providers.hideKey' : 'providers.showKey')}
          onClick={() => setShowKey((s) => !s)}>
          <Icon name={showKey ? 'vision' : 'vision-off'} size={12} />
        </button>
      </div>
      <label className="dim3" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>{t('providers.model')}</label>
      <input className="input" style={input} placeholder={t('providers.pickModel')} value={model}
        onChange={(e) => setModel(e.target.value)} />
      <button className="btn primary" style={{ marginTop: 10 }} disabled={busy || !canApply} onClick={apply}>
        {t('wizard.providersApply')}
      </button>
    </>
  )
}

function FixProviderForm({ doc, onRefresh }: { doc: ProvidersView; onRefresh: () => void }) {
  const { t } = useTranslation()
  const [pid, setPid] = useState(() => initialProviderId(doc))
  const provider = doc.providers.find((p) => p.id === pid) ?? doc.providers[0]
  const [model, setModel] = useState(() => suggestedModel(doc, provider))
  const [secret, setSecret] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [busy, setBusy] = useState(false)
  if (!provider) return null
  const needsKey = !provider.key_set
  const canApply = !!model.trim() && (!needsKey || !!secret.trim())
  const input: React.CSSProperties = { width: '100%', marginTop: 4, textAlign: 'left' }

  async function apply() {
    if (!canApply) return
    const modelId = model.trim()
    setBusy(true)
    try {
      // 钥匙已存就不要再送 secret——空 secret 后端会留下原钥匙，但这一步根本不该再问。
      await api.saveProvider(
        { ...provider, enabled: true },
        needsKey ? secret.trim() : undefined,
      )
      await api.setSlotBinding('default', provider.id, modelId)
      setSecret('')
      onRefresh()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <label className="dim3" style={{ fontSize: 11 }}>{t('wizard.providersPick')}</label>
      <select className="input" style={input} value={provider.id}
        onChange={(e) => {
          const id = e.target.value
          setPid(id)
          setModel(suggestedModel(doc, doc.providers.find((p) => p.id === id)))
          setSecret('')
          setShowKey(false)
        }}>
        {doc.providers.map((p) => (
          <option key={p.id} value={p.id}>{p.name}</option>
        ))}
      </select>
      {needsKey && (
        <>
          <label className="dim3" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>{t('providers.key')}</label>
          <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
            <input className="input" type={showKey ? 'text' : 'password'} style={{ flex: 1, textAlign: 'left' }}
              placeholder={t('wizard.keyPlaceholder')} value={secret}
              onChange={(e) => setSecret(e.target.value)} />
            <button className="btn" title={t(showKey ? 'providers.hideKey' : 'providers.showKey')}
              onClick={() => setShowKey((s) => !s)}>
              <Icon name={showKey ? 'vision' : 'vision-off'} size={12} />
            </button>
          </div>
        </>
      )}
      <label className="dim3" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>{t('providers.model')}</label>
      <input className="input" style={input} placeholder={t('providers.pickModel')} value={model}
        list="wizard-default-models" onChange={(e) => setModel(e.target.value)} />
      <datalist id="wizard-default-models">
        {provider.models.map((m) => <option key={m.id} value={m.id} />)}
      </datalist>
      <button className="btn primary" style={{ marginTop: 10 }} disabled={busy || !canApply} onClick={apply}>
        {t('wizard.providersApply')}
      </button>
    </>
  )
}
