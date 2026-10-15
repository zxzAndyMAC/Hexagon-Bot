// 项目向导：模型服务商 → 选目录 → 项目说明 → 勾角色 → 流程草稿 → 密钥 → 确认开跑。
// 2026-09-25 owner 裁决：说明先于角色——先有项目语境，AI 才能起草职责。
// ADR 0075：AI 起草不再产归属 globs（新项目无目录结构，虚构 globs 会误触发
// 越权裁决）；globs 字段收进「高级选项」折叠，留空 = 不限制写入范围。
// 快速通道在确认页，不采用流程草稿。
// 票 13：第一步没有启用的供应商、已存钥匙和 default 槽就不能进目录；已配好只显示就绪。
// 草稿存 localStorage `hexagon.wizard`，中途退出可续；缺密钥 fail-closed 不能开跑。
// ui-redesign 票 10（ADR 0091，interaction §11）：880 宽卡片 + 竖向步骤轨（wizard/WizardShell）；
// 答问卡内逐题（wizard/BriefStep）；角色「列表 | 详情」即改即生效 +「恢复模板」（wizard/RolesStep）；
// 流程行可拖拽、可键盘移动（wizard/FlowStep）；创建进度 Task Rows（wizard/CreateProgress）。
// 本文件只留状态与流程编排，草稿规则在 wizard/draft.ts。
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, asCmdError, errText, isTauri, type BriefQA, type BriefQuestion, type DirReport, type ProviderView, type ProvidersView, type RoleDef, type RoleTemplate } from '../api'
import { applyCreateFailure, type CreateStep } from '../createProgress'
import { useUiStore } from '../store'
import { providerStepReady } from '../providerGate'
import { Button } from '@/components/ui/button'
import { Icon } from './Icon'
import { GenerationRetry, StopGeneration, type GenerationFailure } from './WizardGeneration'
import type { QualityCategory } from '../gen/QualityCategory'
import {
  DRAFT_KEY, STEPS, applyRoleEdit, dirName, draftSource, flowKey, loadDraft, projectKey, restoreRoleTemplate,
  type Draft, type DraftSeed, type Step,
} from './wizard/draft'
import { StepHeading, WizardShell } from './wizard/WizardShell'
import { ProviderFirstStep } from './wizard/ProviderStep'
import { BriefStep, type BriefPhase } from './wizard/BriefStep'
import { RolesStep } from './wizard/RolesStep'
import { FlowDraft, QualityCommandsForm } from './wizard/FlowStep'
import { ConfirmSummary, DirStep, KeysStep } from './wizard/SetupSteps'
import { CreateProgress } from './wizard/CreateProgress'

export function Wizard({ onDone, onCancel, seed }: {
  onDone: () => void
  /** 第一步的「取消」：回启动页，草稿保留（entry#wz-provider 底栏）。不传则不显示。 */
  onCancel?: () => void
  /** 启动页一句话开局带入的种子（票 10）。 */
  seed?: DraftSeed
}) {
  const { t } = useTranslation()
  const [step, setStep] = useState<Step>('providers')
  // 切过步才把焦点交给新步骤标题；打开向导时不抢焦点
  const [moved, setMoved] = useState(false)
  // 全新草稿 = localStorage 无存档；「默认全选」只盖新草稿，不盖存档勾选
  const [freshDraft] = useState(() => localStorage.getItem(DRAFT_KEY) === null)
  const [draft, setDraft] = useState<Draft>(() => loadDraft(seed))
  const [report, setReport] = useState<DirReport | null>(null)
  const [tpls, setTpls] = useState<RoleTemplate[]>([])
  const [makingRole, setMakingRole] = useState(false)
  // 角色步详情区当前选中的角色（点整行定制；勾选框只决定要不要）
  const [selectedRole, setSelectedRole] = useState<string | null>(null)
  // 本次打开向导后在详情区编辑过的角色：详情头部显示「改动已存入向导草稿」
  const [editedRoles, setEditedRoles] = useState<Set<string>>(new Set())
  const [doc, setDoc] = useState<ProvidersView>({ providers: [], slots: {} })
  const [providersLoaded, setProvidersLoaded] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  // 非错误的结果提示（「已更新 N 个角色」「模型回了相同职责」）：不用红字
  const [note, setNote] = useState('')
  // 答问优化流（2026-09-25）：asking=出题中 / drafting=起草中；idle 时若
  // questions 非空则是等负责人答问。出题失败回落直出，答问不挡优化。
  const [briefPhase, setBriefPhase] = useState<BriefPhase>('idle')
  const [questions, setQuestions] = useState<BriefQuestion[] | null>(null)
  const [answers, setAnswers] = useState<Record<number, string>>({})
  const [qIndex, setQIndex] = useState(0)
  // 撰写态开关：「修改」一句话时为真；优化发起后回到折叠
  const [briefEditing, setBriefEditing] = useState(false)
  // 上一次起草用到的答问（「已答 N 题 · 用于起草」「重新起草」）
  const [lastQa, setLastQa] = useState<BriefQA[] | null>(null)
  // 角色步 AI 起草（职责）：只补未经人工编辑的勾选角色，人手的不覆盖。
  const [rolesBusy, setRolesBusy] = useState(false)
  // 流程步 draft_flow 落定标记（成功/失败/空都算落定）。在途=进了流程步还没草稿也没落定——
  // 派生值而非 effect 里同步 setState：set-state-in-effect 会触发级联渲染，oxlint 红线
  const [flowDone, setFlowDone] = useState(false)
  // MR-07: the failed generation of the current step and its retry; a retry
  // reuses the retained input and never confirms a draft on its own.
  const [genFail, setGenFail] = useState<GenerationFailure | null>(null)
  const [flowAttempt, setFlowAttempt] = useState(0)
  const noteGenFail = (failedStep: string, e: unknown, retry: () => void) => {
    const { code } = asCmdError(e)
    setGenFail(code.startsWith('model_') ? { step: failedStep, code, retry: () => { setGenFail(null); retry() } } : null)
  }
  // Live acceptance 2026-10-01 / ADR 0069: the removed PackEditor is not an
  // approval surface. Only this mount's visible owner input may supply runners;
  // never hydrate it from a model pack or persisted draft. Changing the project
  // invalidates it. A false negative costs re-entry; a false positive runs an
  // unreviewed command with owner authority.
  const [ownerQuality, setOwnerQuality] = useState<{ scope: string; commands: Partial<Record<QualityCategory, string>> }>({ scope: '', commands: {} })
  const qualityCommands = ownerQuality.scope === flowKey(draft) ? ownerQuality.commands : {}
  const approvedQuality = Object.fromEntries(Object.entries(qualityCommands).filter(([, command]) => command.trim()))
  const flowStale = !!draft.flowPack && draft.flowDraftKey !== flowKey(draft)
  const flowBusy = step === 'flow' && !flowDone && (!draft.flowPack || flowStale)
  const [newRoleBusy, setNewRoleBusy] = useState(false)
  // AI 调用在途即锁跳步（owner 2026-09-25：上一步/下一步/步骤轨回跳全锁），
  // 避免草稿还在生成时人已经走开。创建进度是例外：命令在阻塞池不占渲染线程，
  // 「进行中仍可点上一步」是票 14 钉死的既有裁决，busy 不进 aiBusy。
  const aiBusy = briefPhase !== 'idle' || rolesBusy || flowBusy || newRoleBusy
  // entry#wz-brief「答问期间先锁定」（components E9）：答问卡在场时步骤轨与上一步 / 下一步也锁，
  // 出口是答问卡里的「继续 / 跳过」或一句话的「修改」。
  const qaActive = step === 'brief' && !!questions && questions.length > 0
  const locked = aiBusy || qaActive
  // 票 14：创建步骤随核的回报点亮。失败停在该步，不进入工作台。
  const [createDone, setCreateDone] = useState<CreateStep[]>([])
  const [createFail, setCreateFail] = useState<{ step: CreateStep; reason: string } | null>(null)
  const [createMs, setCreateMs] = useState<Partial<Record<CreateStep, number>>>({})

  // MR-07: a late draft for an older one-line brief never overwrites newer input.
  const briefRef = useRef(draft.brief)
  briefRef.current = draft.brief
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
  }, [draft.dir, set])

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
  const aiDraftedNames = useMemo(
    () => new Set(aiStale ? [] : draft.roleDrafted.filter((n) => !!draft.roleOverrides[n])),
    [aiStale, draft.roleDrafted, draft.roleOverrides],
  )
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
  // 角色步的模型槽下拉也要 providers 文档（槽名 + 绑定模型）
  useEffect(() => {
    if (step === 'roles' && !providersLoaded) recheckKeys()
  }, [step, providersLoaded, recheckKeys])

  // 槽就绪 = 绑定存在 && 供应商启用 && key 已存（本槽 → default 兜底，同回合内核解析序）
  const slotReady = useCallback(
    (slot: string) => {
      const b = doc.slots[slot] ?? doc.slots.default
      const provider = b ? doc.providers.find((p) => p.id === b.provider_id) : undefined
      return !!(provider && provider.enabled && provider.key_set)
    },
    [doc],
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
      case 'flow': return !flowStale && !!draft.flowPack && draft.flowPack.stages.length > 0
      case 'confirm': return !draft.fastPath || !!draft.fastRole
      case 'keys': return unready.length === 0 && slots.length > 0
    }
  }, [step, draft, dirBlocked, unready, slots, doc, report, flowStale])

  const existingRepoAlign =
    !!report?.is_git && !report?.empty && draft.roles.includes('产品策划')

  async function browse() {
    if (!isTauri) return
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({ directory: true })
    if (typeof picked === 'string') set({ dir: picked, name: draft.name || dirName(picked) })
  }

  /// 给已绑定供应商补 key：saveProvider 传 secret 只换 key，不改配置。
  async function saveKey(provider: ProviderView, secret: string): Promise<boolean> {
    if (!secret) return false
    setBusy(true)
    try {
      await api.saveProvider(provider, secret)
      recheckKeys()
      return true
    } catch (e) {
      // ui-audit 票 04（P1-6）：saveKey 失败原先只复位 busy、
      // 无任何反馈——「按了没反应」。走 toast 出口。
      useUiStore.getState().pushToast(errText(e), 'err')
      return false
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
  async function runOptimize(qa: BriefQA[]): Promise<boolean> {
    const sentence = draft.brief.trim()
    if (!sentence || report?.instructions) return false
    setBriefPhase('drafting')
    setBriefEditing(false)
    setErr('')
    setGenFail(null)
    try {
      const md = await api.optimizeAgentsMd(draft.name, sentence, qa)
      if (briefRef.current.trim() !== sentence) return false
      set({ agentsMd: md, genAgents: true })
      setQuestions(null)
      setAnswers({})
      setQIndex(0)
      setLastQa(qa)
      return true
    } catch (e) {
      setErr(errText(e))
      noteGenFail('brief', e, () => { void runOptimize(qa) })
      return false
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
    setBriefEditing(false)
    setBriefPhase('asking')
    let qs: BriefQuestion[] = []
    // MR-07: questions are a bonus, not a gate, but their failure is shown
    // instead of silently skipped.
    let skipped = ''
    try {
      qs = await api.briefQuestions(draft.name, sentence)
    } catch (e) {
      qs = []
      skipped = errText(e)
    }
    const answerable = qs.filter((q) => q.question.trim() && q.options.length > 0)
    if (answerable.length === 0) {
      if (await runOptimize([]) && skipped) setNote(t('wizard.questionsSkipped', { reason: skipped }))
      return
    }
    setQuestions(answerable)
    setAnswers({})
    setQIndex(0)
    setBriefPhase('idle')
  }

  /// 答问卡敲定：把答过的题打包成 BriefQA 起草；未答的题不携带。
  /// 起草在途（busy 已锁），这层防连点竞态。
  function generateWithAnswers(all: Record<number, string>) {
    if (!questions || briefPhase !== 'idle') return
    const qa = questions.flatMap((q, i) => {
      const a = (all[i] ?? '').trim()
      // 2026-10-03 fullstack QA #01：只送选中的“以上都要”曾丢失所指，
      // 起草模型随即把已敲定的统计需求写成未知。自填“前两项”也需要原选项。
      const question = `${q.question}\n${q.options.map((option, index) => `${index + 1}. ${option}`).join('\n')}`
      return a ? [{ question, answer: a }] : []
    })
    void runOptimize(qa)
  }

  /// 「继续」：跳到下一道未答题；全部答完即起草（components E10 补设计）。
  function continueQa() {
    if (!questions) return
    const open = (i: number) => !(answers[i] ?? '').trim()
    const order = [...questions.keys()].map((k) => (qIndex + 1 + k) % questions.length)
    const nextOpen = order.find(open)
    if (nextOpen === undefined) generateWithAnswers(answers)
    else setQIndex(nextOpen)
  }

  /// 角色步 AI 起草：项目说明 → 每个勾选角色在本项目的职责段落。
  /// 只有 roleHandTuned（详情区人工编辑过、新建角色，ADR 0091）不入参不覆盖。
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
    setNote('')
    setGenFail(null)
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
        setNote(t('wizard.aiDraftApplied', { n: changed }))
      } else {
        setNote(t('wizard.aiDraftSame'))
      }
    } catch (e) {
      setErr(errText(e))
      noteGenFail('roles', e, () => { void draftRoleSeeds() })
    } finally {
      setRolesBusy(false)
    }
  }

  async function launch() {
    setBusy(true)
    setCreateDone([])
    setCreateFail(null)
    setCreateMs({})
    const seen: CreateStep[] = []
    let failed = false
    let last = Date.now()
    const markStep = (s: CreateStep) => {
      if (failed || seen.includes(s)) return
      seen.push(s)
      const now = Date.now()
      setCreateMs((m) => ({ ...m, [s]: now - last }))
      last = now
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
        // Ticket 04: hidden model/legacy commands stay stripped. The separately
        // displayed owner inputs are the only runner authority at confirmation.
        pack: draft.fastPath || !draft.flowPack ? null : {
          ...draft.flowPack,
          stages: draft.flowPack.stages.map((stage, index) => ({ ...stage, checks: [],
            quality_checks: index === draft.flowPack!.stages.length - 1 ? approvedQuality : {},
          })),
        },
        fastpathRole: draft.fastPath ? draft.fastRole : null,
        initGit: draft.initGit,
        agentsMd: agentsToWrite(),
      }, markStep)
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
    if (step !== 'flow') return
    if (draft.flowPack && draft.flowDraftKey === flowKey(draft)) return
    let cancelled = false
    // draft_flow 也是 AI 调用：在途由 flowBusy 派生锁跳步，done() 在异步出口落定
    const done = () => { if (!cancelled) setFlowDone(true) }
    const apply = (sentence: string) => {
      const text = sentence.trim()
      if (!text) { done(); return }
      api.draftFlow(text, draft.roles).then((pack) => {
        if (!cancelled) set({ flowPack: pack, flowDraftKey: flowKey(draft) })
      }).catch((e) => {
        if (cancelled) return
        setErr(errText(e))
        noteGenFail('flow', e, () => { setErr(''); setFlowDone(false); setFlowAttempt((n) => n + 1) })
      })
        .finally(done)
    }
    // 一句话换了项目、生成稿还是上一个时，以一句话为准。否则模型按旧稿
    // 起草，确认页流程名仍是上一个项目（owner 2026-09-25：魂斗罗确认页写着计算器）。
    const sentence = draftSource(draft)
    if (sentence) apply(sentence)
    else if (report?.instructions && draft.dir) {
      api.readInstructionFile(draft.dir).then((text) => {
        if (!cancelled) apply(text)
      }).catch(() => { if (!cancelled) apply(draft.name) })
    } else apply(draft.name)
    return () => { cancelled = true }
    // flowAttempt: an explicit owner retry after a failed draft_flow.
  }, [step, draft, report, set, flowAttempt])

  const idx = STEPS.indexOf(step)
  // 跳步即收起新建角色表单（表单本就随步骤卸载丢内容，这里把标志位也清掉语义一致）；
  // err/note 是步内局部状态（brief 优化 / roles 起草），跳步清掉不串台
  const go = (s: Step) => {
    setMakingRole(false)
    setErr('')
    setNote('')
    // 再进 flow 步要重试 draft_flow——上次落定标记清掉，flowBusy 派生跟着重立
    if (s === 'flow' && draft.flowDraftKey !== flowKey(draft)) setFlowDone(false)
    setMoved(true)
    setStep(s)
  }

  const selectedName = selectedRole && tpls.some((tp) => tp.def.name === selectedRole)
    ? selectedRole
    : (pickedRoles[0]?.def.name ?? tpls[0]?.def.name ?? null)

  const messages = (
    <>
      {note && <div role="status" className="mt-3 text-[12.5px] text-ink-2">{note}</div>}
      {err && <div role="alert" className="mt-3 text-[12.5px] text-red">{err}</div>}
      <GenerationRetry failure={genFail} step={step} />
    </>
  )

  const heading = (key: Step, lead?: React.ReactNode) => (
    <StepHeading focus={moved} title={t(`wizard.heading.${key}`)} lead={lead} />
  )
  const padded = (node: React.ReactNode) => <div className="px-8 py-7">{node}</div>
  const hasBriefDraft = !!report?.empty && !!draft.agentsMd.trim() && !questions

  const body: Record<Step, React.ReactNode> = {
    providers: padded(<>
      {heading('providers', t('wizard.providersHint'))}
      <ProviderFirstStep doc={doc} loaded={providersLoaded} onRefresh={recheckKeys} />
    </>),
    dir: padded(<>
      {heading('dir', t('wizard.lead.dir'))}
      <DirStep draft={draft} set={set} report={report} busy={busy} err={err} onBrowse={browse} onOpenExisting={openExisting} />
    </>),
    brief: padded(<>
      {heading('brief', report?.empty
        ? t(questions ? 'wizard.lead.briefAsk' : hasBriefDraft ? 'wizard.lead.briefDraft' : 'wizard.briefHint')
        : undefined)}
      <BriefStep
        draft={draft}
        report={report}
        set={set}
        phase={briefPhase}
        questions={questions}
        answers={answers}
        qIndex={qIndex}
        lastQa={lastQa}
        editing={briefEditing}
        onEdit={() => {
          // 「修改」一句话：关掉答问回到撰写（旧答案针对的题作废，重按优化重新出题）
          setQuestions(null)
          setAnswers({})
          setQIndex(0)
          setBriefEditing(true)
        }}
        onBriefChange={(v) => {
          set({ brief: v })
          if (questions) {
            setQuestions(null)
            setAnswers({})
          }
        }}
        onOptimize={optimizeBrief}
        onIndex={setQIndex}
        onAnswer={(i, v) => setAnswers((a) => ({ ...a, [i]: v }))}
        onContinue={continueQa}
        onSkip={() => { void runOptimize([]) }}
        onRedraft={() => {
          if (lastQa && lastQa.length > 0) void runOptimize(lastQa)
          else void optimizeBrief()
        }}
        footer={messages}
      />
    </>),
    roles: (
      <RolesStep
        tpls={tpls}
        draft={draft}
        effDef={effDef}
        aiDraftedNames={aiDraftedNames}
        selected={selectedName}
        onSelect={(name) => { setMakingRole(false); setSelectedRole(name) }}
        onToggle={(name, on) => {
          // 勾选不改详情区：默认选中是派生值，先钉住它，免得勾上排在前面的角色时详情跳走
          if (!selectedRole) setSelectedRole(selectedName)
          set({ roles: on ? [...draft.roles.filter((x) => x !== name), name] : draft.roles.filter((x) => x !== name) })
        }}
        onSelectAll={() => set({ roles: tpls.map((tp) => tp.def.name) })}
        makingRole={makingRole}
        onNewRole={() => setMakingRole((v) => !v)}
        edited={editedRoles}
        onEdit={(tp, def) => {
          // ADR 0091：即改即生效写入草稿；人工编辑即转人手（roleHandTuned）
          setDraft((d) => applyRoleEdit(d, tp.def, def))
          setEditedRoles((s) => new Set(s).add(tp.def.name))
        }}
        onRestore={(name) => {
          setDraft((d) => restoreRoleTemplate(d, name))
          setEditedRoles((s) => { const n = new Set(s); n.delete(name); return n })
        }}
        rolesBusy={rolesBusy}
        doc={doc}
        aiDraft={{
          label: t(showRedraft ? 'wizard.aiRedraft' : 'wizard.aiDraft'),
          title: draft.agentsMd.trim() || draft.brief.trim() || report?.instructions ? t('wizard.aiDraftHint') : t('wizard.aiDraftNoBrief'),
          disabled: rolesBusy
            || pickedRoles.every((tp) => draft.roleHandTuned.includes(tp.def.name))
            || !(draft.agentsMd.trim() || draft.brief.trim() || report?.instructions),
          onClick: () => { void draftRoleSeeds() },
        }}
        newRole={{
          onBusy: setNewRoleBusy,
          onCancel: () => setMakingRole(false),
          onDone: (def: RoleDef) => {
            setTpls((list) => [...list.filter((tp) => tp.def.name !== def.name), { def, origin: 'custom' }])
            set({
              roles: draft.roles.includes(def.name) ? draft.roles : [...draft.roles, def.name],
              roleOverrides: { ...draft.roleOverrides, [def.name]: def },
              // 新建的是人手定义，不入 AI 起草名单
              roleDrafted: draft.roleDrafted.filter((n) => n !== def.name),
              roleHandTuned: draft.roleHandTuned.includes(def.name) ? draft.roleHandTuned : [...draft.roleHandTuned, def.name],
            })
            setMakingRole(false)
            setSelectedRole(def.name)
          },
        }}
        footer={messages}
      />
    ),
    flow: padded(<>
      {heading('flow', t('wizard.lead.flow'))}
      <FlowDraft pack={draft.flowPack} roles={draft.roles} busy={flowBusy} onChange={(flowPack) => set({ flowPack })} />
      {!flowBusy && draft.flowPack && (
        <QualityCommandsForm
          commands={qualityCommands}
          onChange={(category, value) => setOwnerQuality({ scope: flowKey(draft), commands: { ...qualityCommands, [category]: value } })}
        />
      )}
      {messages}
    </>),
    keys: padded(<>
      {heading('keys', t('wizard.keysHint'))}
      <KeysStep slots={slots} doc={doc} pickedRoles={pickedRoles} effDef={effDef} busy={busy} onSaveKey={saveKey} />
    </>),
    confirm: padded(<>
      {heading('confirm', t('wizard.lead.confirm'))}
      <ConfirmSummary draft={draft} set={set} pickedRoles={pickedRoles} approvedQuality={approvedQuality} report={report} existingRepoAlign={existingRepoAlign} />
      {(busy || createDone.length > 0 || createFail) && (
        <CreateProgress name={draft.name} busy={busy} done={createDone} fail={createFail} durations={createMs} roleCount={draft.roles.length} />
      )}
    </>),
  }

  const status = aiBusy
    ? t('wizard.shell.aiLocked')
    : qaActive
      ? t('wizard.shell.qaLocked')
      : step === 'roles'
        ? t('wizard.shell.picked', { count: draft.roles.length })
        : t('wizard.shell.autosave')

  const foot = (
    <>
      {idx > 0 ? (
        <Button size="sm" variant="ghost" disabled={locked} onClick={() => go(STEPS[idx - 1])}>
          <Icon name="arrow-left" />{t('wizard.back')}
        </Button>
      ) : onCancel ? (
        <Button size="sm" variant="ghost" onClick={onCancel}>{t('wizard.shell.cancel')}</Button>
      ) : null}
      <span className="flex-1" />
      <span data-wizard-status className="text-[12px] text-ink-3">{status}</span>
      <StopGeneration busy={aiBusy} />
      {step === 'confirm' ? (
        <Button size="sm" variant="primary" disabled={busy} onClick={launch}>
          {t('wizard.launch')}
        </Button>
      ) : (
        <Button size="sm" variant="primary" disabled={!canNext || locked} onClick={() => go(STEPS[idx + 1])}>
          {t('wizard.next')}<Icon name="arrow-right" />
        </Button>
      )}
    </>
  )

  return (
    <WizardShell steps={STEPS} step={step} locked={locked} onJump={go} foot={foot}>
      {body[step]}
    </WizardShell>
  )
}
