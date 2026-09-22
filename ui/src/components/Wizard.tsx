// 项目向导（票 24）：选目录 → 勾角色 → 选流程包/快速通道 → 说明文件 → 密钥 → 开跑。
// 草稿存 localStorage `hexagon.wizard`，中途退出可续；缺密钥 fail-closed 不能开跑。
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, isTauri, type DirReport, type PackDef, type ProvidersView, type RoleDef, type RoleTemplate } from '../api'
import { useUiStore } from '../store'
import { sharedSlots, slotLabel } from '../modelpick'
import { Icon } from './Icon'
import { EntityChips } from './EntityPicker'

const DRAFT_KEY = 'hexagon.wizard'

interface Draft {
  dir: string
  name: string
  roles: string[]
  /// ADR 0057：按角色名存定制后的完整 RoleDef（只作用本项目，不回写模板库）。
  /// 缺项 = 用模板原定义。
  roleOverrides: Record<string, RoleDef>
  mode: 'pack' | 'fastpath'
  packName: string
  fastRole: string
  initGit: boolean
  genAgents: boolean
  agentsMd: string
}

const EMPTY: Draft = {
  dir: '', name: '', roles: [], roleOverrides: {}, mode: 'pack', packName: '规格驱动',
  fastRole: '', initGit: false, genAgents: false, agentsMd: '',
}

function loadDraft(): Draft {
  try {
    return { ...EMPTY, ...JSON.parse(localStorage.getItem(DRAFT_KEY) || '{}') }
  } catch {
    return { ...EMPTY }
  }
}

const STEPS = ['dir', 'roles', 'mode', 'instructions', 'keys', 'confirm'] as const
type Step = (typeof STEPS)[number]

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
  const [step, setStep] = useState<Step>('dir')
  const [draft, setDraft] = useState<Draft>(loadDraft)
  const [report, setReport] = useState<DirReport | null>(null)
  const [tpls, setTpls] = useState<RoleTemplate[]>([])
  const [packs, setPacks] = useState<PackDef[]>([])
  // 正在展开定制的角色名（roles 步内联编辑面板）
  const [customizing, setCustomizing] = useState<string | null>(null)
  const [doc, setDoc] = useState<ProvidersView>({ providers: [], slots: {} })
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({})
  // 票 15：密钥显隐复用 ProviderManager 的 vision 钮模式（每 provider 独立）。
  const [keyShown, setKeyShown] = useState<Set<string>>(new Set())
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  const set = useCallback(
    (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch })),
    [],
  )

  // 草稿持久化：任何字段变化即写盘，中途退出可续
  useEffect(() => {
    localStorage.setItem(DRAFT_KEY, JSON.stringify(draft))
  }, [draft])

  useEffect(() => {
    api.listRoleTemplates().then(setTpls).catch(() => {})
    api.presetPacks().then(setPacks).catch(() => {})
    // roles 步的模型槽下拉也要供应商文档——挂载即拉，不必等 keys 步轮询
    api.listProviders().then(setDoc).catch(() => {})
  }, [])

  // 目录变化 → 重新体检（setTimeout 内统一处理，避免 effect 内同步 setState）
  useEffect(() => {
    const id = setTimeout(() => {
      if (!draft.dir) { setReport(null); return }
      api.inspectDir(draft.dir).then(setReport).catch(() => setReport(null))
    }, 200)
    return () => clearTimeout(id)
  }, [draft.dir])

  // 说明文件草稿：选了生成且还没内容时拉模板
  useEffect(() => {
    if (draft.genAgents && !draft.agentsMd && draft.name) {
      api.agentsMdDraft(draft.name).then((md) => set({ agentsMd: md })).catch(() => {})
    }
  }, [draft.genAgents, draft.agentsMd, draft.name, set])

  // 生效定义 = 向导定制 override ?? 模板原定义（模板含内置∪自定义两层）
  const effDef = useCallback(
    (tp: RoleTemplate) => draft.roleOverrides[tp.def.name] ?? tp.def,
    [draft.roleOverrides],
  )
  const pickedRoles = useMemo(
    () => tpls.filter((tp) => draft.roles.includes(tp.def.name)),
    [tpls, draft.roles],
  )
  const slots = useMemo(
    () => [...new Set(pickedRoles.map((tp) => effDef(tp).model_slot))],
    [pickedRoles, effDef],
  )

  // 进入密钥步 / 所选角色变化 → 拉供应商文档（绑定 + key 状态）
  const recheckKeys = useCallback(() => {
    api.listProviders().then(setDoc).catch(() => {})
  }, [])
  useEffect(() => {
    if (step !== 'keys') return
    // 轮询而非一次性：从设置页（hexagon:open-settings 覆盖层）返回时状态自刷新
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
      case 'dir': return !!draft.dir && !!draft.name && !dirBlocked
      case 'roles': return draft.roles.length > 0
      case 'mode':
        return draft.mode === 'pack' ? !!draft.packName : !!draft.fastRole
      case 'instructions':
      case 'confirm': return true
      case 'keys': return unready.length === 0 && slots.length > 0
    }
  }, [step, draft, dirBlocked, unready, slots])

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

  async function launch() {
    setBusy(true)
    setErr('')
    try {
      await api.createProject({
        dir: draft.dir,
        name: draft.name,
        roles: draft.roles,
        // 全部勾选角色传生效定义：自定义模板与定制项经 override 链物化，
        // 内置模板同名直传也无损（定义等价）。
        roleOverrides: pickedRoles.map(effDef),
        packName: draft.mode === 'pack' ? draft.packName : null,
        fastpathRole: draft.mode === 'fastpath' ? draft.fastRole : null,
        initGit: draft.initGit,
        agentsMd:
          draft.genAgents && !report?.instructions ? draft.agentsMd : null,
      })
      localStorage.removeItem(DRAFT_KEY)
      onDone()
    } catch (e) {
      setErr(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const idx = STEPS.indexOf(step)
  const body: Record<Step, React.ReactNode> = {
    dir: (
      <>
        <label className="dim3" style={{ fontSize: 11 }}>{t('wizard.dirLabel')}</label>
        <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
          <input
            className="btn"
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
            className="btn"
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
        <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('wizard.rolesHint')}</div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
          {tpls.map((tp) => {
            const picked = draft.roles.includes(tp.def.name)
            const customized = !!draft.roleOverrides[tp.def.name]
            return (
              <label
                key={tp.def.name}
                className="panel"
                style={{
                  display: 'flex', gap: 8, padding: '8px 10px', cursor: 'pointer',
                  outline: picked ? '1px solid var(--accent)' : undefined,
                }}
              >
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
                  <div className="dim3" style={{ fontSize: 11 }}>{effDef(tp).duty}</div>
                </div>
                {picked && (
                  <button
                    className="btn"
                    style={{ fontSize: 10, alignSelf: 'flex-start' }}
                    onClick={(e) => {
                      e.preventDefault()
                      setCustomizing(customizing === tp.def.name ? null : tp.def.name)
                    }}
                  >
                    {t('wizard.customize')}
                  </button>
                )}
              </label>
            )
          })}
        </div>
        {customizing && draft.roles.includes(customizing) && (
          <RoleCustomize
            key={customizing}
            def={effDef(tpls.find((x) => x.def.name === customizing)!)}
            names={tpls.map((x) => x.def.name)}
            doc={doc}
            onChange={(def) =>
              set({ roleOverrides: { ...draft.roleOverrides, [customizing]: def } })
            }
            onReset={() => {
              const next = { ...draft.roleOverrides }
              delete next[customizing]
              set({ roleOverrides: next })
            }}
            onClose={() => setCustomizing(null)}
          />
        )}
      </>
    ),
    mode: (
      <>
        <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('wizard.modeHint')}</div>
        <label style={{ display: 'flex', gap: 6, alignItems: 'center', fontSize: 12 }}>
          <input
            type="radio"
            checked={draft.mode === 'pack'}
            onChange={() => set({ mode: 'pack' })}
          />
          {t('wizard.packMode')}
        </label>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4, margin: '8px 0 12px 22px' }}>
          {packs.map((p) => (
            <label key={p.name} style={{ display: 'flex', gap: 6, fontSize: 12 }}>
              <input
                type="radio"
                disabled={draft.mode !== 'pack'}
                checked={draft.mode === 'pack' && draft.packName === p.name}
                onChange={() => set({ packName: p.name })}
              />
              {p.name}
              <span className="dim3">（{p.stages.length} {t('wizard.stages')}）</span>
            </label>
          ))}
        </div>
        <label style={{ display: 'flex', gap: 6, alignItems: 'center', fontSize: 12 }}>
          <input
            type="radio"
            checked={draft.mode === 'fastpath'}
            onChange={() => set({ mode: 'fastpath' })}
          />
          {t('wizard.fastMode')}
        </label>
        <div style={{ margin: '8px 0 0 22px' }}>
          <select
            className="btn"
            disabled={draft.mode !== 'fastpath'}
            value={draft.fastRole}
            onChange={(e) => set({ fastRole: e.target.value })}
          >
            <option value="">{t('wizard.fastPick')}</option>
            {pickedRoles.map((tp) => (
              <option key={tp.def.name} value={tp.def.name}>{tp.def.name}</option>
            ))}
          </select>
        </div>
      </>
    ),
    instructions: (
      <>
        {report?.instructions ? (
          <div style={{ fontSize: 12 }}>
            <Chip ok>{report.instructions}</Chip>
            <div className="dim3" style={{ marginTop: 8, fontSize: 12 }}>
              {t('wizard.instructionsFound', { file: report.instructions })}
            </div>
          </div>
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
                  className="btn"
                  style={{ width: '100%', height: 180, textAlign: 'left', fontFamily: 'monospace', fontSize: 11 }}
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
                      className="btn"
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
            {draft.mode === 'pack' ? draft.packName : `${t('wizard.fastMode')} · ${draft.fastRole}`}
          </div>
          <div>
            <span className="dim3">{t('wizard.sumAgents')}：</span>
            {report?.instructions
              ? report.instructions
              : draft.genAgents
                ? t('wizard.sumAgentsGen')
                : t('wizard.sumAgentsNone')}
          </div>
        </div>
        {existingRepoAlign && (
          <div className="panel" style={{ marginTop: 12, padding: '8px 10px', fontSize: 12, color: 'var(--flag)' }}>
            {t('wizard.alignHint')}
          </div>
        )}
        {err && <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 10 }}>{err}</div>}
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
      {/* 票 15：窄窗不溢出——min(560px, 92vw) */}
      <div className="panel" style={{ width: 'min(560px, 92vw)', maxHeight: '86vh', display: 'flex', flexDirection: 'column', padding: '20px 22px' }}>
        <div style={{ fontWeight: 600, fontSize: 15 }}>{t('wizard.title')}</div>
        {/* 步骤轨（票 15：已完成步可点回跳；未来步不可点——跳步会绕过 canNext 校验） */}
        <div style={{ display: 'flex', gap: 4, margin: '12px 0 16px' }}>
          {STEPS.map((s, i) => (
            <div
              key={s}
              role={i < idx ? 'button' : undefined}
              tabIndex={i < idx ? 0 : undefined}
              title={t(`wizard.step.${s}`)}
              onClick={i < idx ? () => setStep(s) : undefined}
              onKeyDown={i < idx ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setStep(s) } } : undefined}
              style={{
                flex: 1, height: 3, borderRadius: 2,
                background: i <= idx ? 'var(--accent)' : 'var(--bg-2)',
                cursor: i < idx ? 'pointer' : 'default',
              }}
            />
          ))}
        </div>
        <div style={{ fontSize: 12, fontWeight: 510, marginBottom: 8 }}>
          {t(`wizard.step.${step}`)}
        </div>
        {/* 滚动体留 4px 呼吸位：focus 环(2+2)与选中描边在滚口边缘不被裁 */}
        <div style={{ flex: 1, overflowY: 'auto', minHeight: 0, padding: 4, margin: -4 }}>{body[step]}</div>
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 16 }}>
          {idx > 0 && (
            <button className="btn" onClick={() => setStep(STEPS[idx - 1])}>
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
              disabled={!canNext}
              onClick={() => setStep(STEPS[idx + 1])}
            >
              {t('wizard.next')}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

/** 向导内角色定制（ADR 0057）：改出的 RoleDef 只经 roleOverrides 进本项目，
 *  不回写模板库。字段与模板编辑面一致：职责/上级/模型槽/归属路径/技能。 */
function RoleCustomize({ def, names, doc, onChange, onReset, onClose }: {
  def: RoleDef
  names: string[]
  doc: ProvidersView
  onChange: (def: RoleDef) => void
  onReset: () => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const input: React.CSSProperties = {
    width: '100%', padding: '4px 8px', fontSize: 12,
    background: 'var(--bg)', border: '1px solid var(--border-strong)', borderRadius: 6,
    color: 'var(--text)', fontFamily: 'inherit',
  }
  const lbl: React.CSSProperties = { fontSize: 10, fontWeight: 560, color: 'var(--text-3)', marginTop: 6 }
  const upd = (patch: Partial<RoleDef>) => onChange({ ...def, ...patch })

  return (
    <div className="panel" style={{ marginTop: 8, padding: '10px 12px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <strong style={{ fontSize: 12 }}>{def.name}</strong>
        <span className="dim3" style={{ fontSize: 10 }}>{t('wizard.scopeHint')}</span>
        <button className="btn" style={{ marginLeft: 'auto', fontSize: 10 }} onClick={onReset}>
          {t('wizard.resetTpl')}
        </button>
        <button className="btn" style={{ fontSize: 10 }} onClick={onClose}>×</button>
      </div>
      <div style={lbl}>{t('agent.duty')}</div>
      <textarea value={def.duty} rows={2} style={{ ...input, resize: 'vertical' }}
        onChange={(e) => upd({ duty: e.target.value })} />
      <div style={{ display: 'flex', gap: 8 }}>
        <div style={{ flex: 1 }}>
          <div style={lbl}>{t('agent.reviewer')}</div>
          <select value={def.reviewer ?? ''} style={input}
            onChange={(e) => upd({ reviewer: e.target.value || null })}>
            <option value="">{t('agent.noReviewer')}</option>
            {names.filter((n) => n !== def.name).map((n) => (
              <option key={n} value={n}>{n}</option>
            ))}
          </select>
        </div>
        <div style={{ flex: 1 }}>
          <div style={lbl}>{t('agent.modelSlot')}</div>
          <select value={def.model_slot} style={input}
            onChange={(e) => upd({ model_slot: e.target.value })}>
            {sharedSlots(doc.slots, def.model_slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, doc, t('agent.dedicatedTag'))}</option>
            ))}
          </select>
        </div>
      </div>
      <div style={lbl}>{t('agent.globs')}</div>
      <textarea value={def.globs.join('\n')} rows={2} placeholder="src/**"
        style={{ ...input, fontFamily: 'monospace', resize: 'vertical' }}
        onChange={(e) => upd({ globs: e.target.value.split('\n').map((s) => s.trim()).filter(Boolean) })} />
      <div style={lbl}>{t('agent.skills')}</div>
      <EntityChips value={def.skills} onChange={(ids) => upd({ skills: ids })} source="skills" />
    </div>
  )
}
