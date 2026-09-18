// 项目向导（票 24）：选目录 → 勾角色 → 选流程包/快速通道 → 说明文件 → 密钥 → 开跑。
// 草稿存 localStorage `hexagon.wizard`，中途退出可续；缺密钥 fail-closed 不能开跑。
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, isTauri, type DirReport, type PackDef, type RoleDef } from '../api'

const DRAFT_KEY = 'hexagon.wizard'

interface Draft {
  dir: string
  name: string
  roles: string[]
  mode: 'pack' | 'fastpath'
  packName: string
  fastRole: string
  initGit: boolean
  genAgents: boolean
  agentsMd: string
}

const EMPTY: Draft = {
  dir: '', name: '', roles: [], mode: 'pack', packName: '规格驱动',
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
        background: warn ? 'rgba(245,158,11,.15)' : ok ? 'rgba(16,185,129,.15)' : 'var(--bg-2)',
        color: warn ? '#f59e0b' : ok ? '#10b981' : 'var(--fg-2)',
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
  const [roles, setRoles] = useState<RoleDef[]>([])
  const [packs, setPacks] = useState<PackDef[]>([])
  const [missingKeys, setMissingKeys] = useState<string[]>([])
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({})
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
    api.presetRoles().then(setRoles).catch(() => {})
    api.presetPacks().then(setPacks).catch(() => {})
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

  const pickedRoles = useMemo(
    () => roles.filter((r) => draft.roles.includes(r.name)),
    [roles, draft.roles],
  )
  const slots = useMemo(
    () => [...new Set(pickedRoles.map((r) => r.model_slot))],
    [pickedRoles],
  )

  // 进入密钥步 / 所选角色变化 → 查缺钥
  const recheckKeys = useCallback(() => {
    if (!slots.length) { setMissingKeys([]); return }
    api.checkModelKeys(slots).then(setMissingKeys).catch(() => {})
  }, [slots])
  useEffect(() => {
    if (step !== 'keys') return
    const id = setTimeout(recheckKeys, 0)
    return () => clearTimeout(id)
  }, [step, recheckKeys])

  const dirBlocked =
    !report || !report.exists || report.dirty || (!report.is_git && !draft.initGit)
  const canNext = useMemo(() => {
    switch (step) {
      case 'dir': return !!draft.dir && !!draft.name && !dirBlocked
      case 'roles': return draft.roles.length > 0
      case 'mode':
        return draft.mode === 'pack' ? !!draft.packName : !!draft.fastRole
      case 'instructions':
      case 'confirm': return true
      case 'keys': return missingKeys.length === 0 && slots.length > 0
    }
  }, [step, draft, dirBlocked, missingKeys, slots])

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

  async function saveKey(slot: string) {
    const secret = keyInputs[slot]?.trim()
    if (!secret) return
    setBusy(true)
    try {
      await api.setModelKey(slot, secret)
      setKeyInputs((k) => ({ ...k, [slot]: '' }))
      recheckKeys()
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
        packName: draft.mode === 'pack' ? draft.packName : null,
        fastpathRole: draft.mode === 'fastpath' ? draft.fastRole : null,
        initGit: draft.initGit,
        agentsMd:
          draft.genAgents && !report?.instructions ? draft.agentsMd : null,
      })
      localStorage.removeItem(DRAFT_KEY)
      onDone()
    } catch (e) {
      setErr(String(e))
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
            {report.dirty && <Chip warn>{t('wizard.dirty')}</Chip>}
            {report.instructions && <Chip ok>{report.instructions}</Chip>}
          </div>
        )}
        {report?.dirty && (
          <div style={{ color: '#f59e0b', fontSize: 12, marginTop: 8 }}>{t('wizard.dirtyHint')}</div>
        )}
        {report && !report.is_git && (
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
          {roles.map((r) => (
            <label
              key={r.name}
              className="panel"
              style={{
                display: 'flex', gap: 8, padding: '8px 10px', cursor: 'pointer',
                outline: draft.roles.includes(r.name) ? '1px solid var(--accent)' : undefined,
              }}
            >
              <input
                type="checkbox"
                checked={draft.roles.includes(r.name)}
                onChange={(e) =>
                  set({
                    roles: e.target.checked
                      ? [...draft.roles, r.name]
                      : draft.roles.filter((x) => x !== r.name),
                  })
                }
              />
              <div>
                <div style={{ fontSize: 12, fontWeight: 510 }}>{r.name}</div>
                <div className="dim3" style={{ fontSize: 11 }}>{r.duty}</div>
              </div>
            </label>
          ))}
        </div>
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
            {pickedRoles.map((r) => (
              <option key={r.name} value={r.name}>{r.name}</option>
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
          const keyName = `model/${slot}`
          const missing = missingKeys.includes(keyName)
          return (
            <div key={slot} style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '6px 0' }}>
              <div style={{ flex: 1 }}>
                <code style={{ fontSize: 12 }}>{keyName}</code>
                <div className="dim3" style={{ fontSize: 11 }}>
                  {pickedRoles.filter((r) => r.model_slot === slot).map((r) => r.name).join('、')}
                </div>
              </div>
              {missing ? (
                <>
                  <input
                    className="btn"
                    type="password"
                    style={{ width: 180 }}
                    placeholder={t('wizard.keyPlaceholder')}
                    value={keyInputs[slot] ?? ''}
                    onChange={(e) =>
                      setKeyInputs((k) => ({ ...k, [slot]: e.target.value }))
                    }
                  />
                  <button className="btn" disabled={busy} onClick={() => saveKey(slot)}>
                    {t('wizard.keySave')}
                  </button>
                </>
              ) : (
                <Chip ok>{t('wizard.keyOk')}</Chip>
              )}
            </div>
          )
        })}
        {missingKeys.length > 0 && (
          <div style={{ color: '#f59e0b', fontSize: 12, marginTop: 8 }}>{t('wizard.keysBlocked')}</div>
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
            {draft.roles.join('、')}
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
          <div className="panel" style={{ marginTop: 12, padding: '8px 10px', fontSize: 12, color: '#9d8cff' }}>
            {t('wizard.alignHint')}
          </div>
        )}
        {err && <div style={{ color: '#f87171', fontSize: 12, marginTop: 10 }}>{err}</div>}
      </>
    ),
  }

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 60, display: 'flex',
        alignItems: 'center', justifyContent: 'center',
        background: 'var(--bg-0)',
      }}
    >
      <div className="panel" style={{ width: 560, maxHeight: '86vh', display: 'flex', flexDirection: 'column', padding: '20px 22px' }}>
        <div style={{ fontWeight: 600, fontSize: 15 }}>{t('wizard.title')}</div>
        {/* 步骤轨 */}
        <div style={{ display: 'flex', gap: 4, margin: '12px 0 16px' }}>
          {STEPS.map((s, i) => (
            <div
              key={s}
              style={{
                flex: 1, height: 3, borderRadius: 2,
                background: i <= idx ? 'var(--accent)' : 'var(--bg-2)',
              }}
            />
          ))}
        </div>
        <div style={{ fontSize: 12, fontWeight: 510, marginBottom: 8 }}>
          {t(`wizard.step.${step}`)}
        </div>
        <div style={{ flex: 1, overflowY: 'auto', minHeight: 0 }}>{body[step]}</div>
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
