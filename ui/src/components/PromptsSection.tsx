import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { PromptEntry } from '../gen/PromptEntry'
import type { PromptTranslation } from '../gen/PromptTranslation'

// ADR 0071：发给模型的固定提示词是英文。这一页给负责人看原文和参考译文，
// 只读——提示词不在这里改（工作台约束不进改进提案的面，ADR 0045）。
// 布局（2026-09 owner 报告）：左侧按组列条目、右侧详情——原先全量平铺 +
// 原文|译文并排双列，长文被砍半宽还两列各滚各的。改清单-详情后译文走页签，
// 页签态跨条目保持（审译文的人逐条过不用每行点一次）。
const GROUPS = ['workbench', 'subagent', 'runtime', 'tools', 'judges', 'wizard', 'translator'] as const

type Tab = 'original' | 'tr'

const tabBtn = (active: boolean): CSSProperties => ({
  border: 'none',
  background: 'transparent',
  borderRadius: 0,
  padding: '7px 2px',
  marginBottom: -1,
  borderBottom: `2px solid ${active ? 'var(--accent)' : 'transparent'}`,
  color: active ? 'var(--text)' : 'var(--text-3)',
  fontWeight: active ? 560 : 400,
})

const preStyle: CSSProperties = {
  whiteSpace: 'pre-wrap',
  fontSize: 11,
  margin: 0,
  padding: 10,
  background: 'var(--bg)',
  borderRadius: 4,
}

export function PromptsSection({ onGoModels }: { onGoModels: () => void }) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const english = lang === 'en'
  const [entries, setEntries] = useState<PromptEntry[]>([])
  const [byId, setById] = useState<Record<string, PromptTranslation>>({})
  const [busy, setBusy] = useState(false)
  const [noModel, setNoModel] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const [sel, setSel] = useState<string | null>(null)
  const [tab, setTab] = useState<Tab>('original')
  // 切语言时上一种语言的请求可能还没回；序号对不上的结果丢弃，免得旧译文写进新语言。
  const seq = useRef(0)

  useEffect(() => {
    api.promptCatalog().then(setEntries).catch((e) => setFailure(errText(e)))
  }, [])

  const translate = async (force: boolean) => {
    const mine = ++seq.current
    setBusy(true)
    setFailure(null)
    try {
      const out = await api.translatePrompts(lang, force)
      if (mine !== seq.current) return
      setNoModel(out.no_model)
      setById(Object.fromEntries(out.entries.map((e) => [e.id, e])))
    } catch (e) {
      if (mine === seq.current) setFailure(errText(e))
    } finally {
      if (mine === seq.current) setBusy(false)
    }
  }

  useEffect(() => {
    seq.current += 1
    setById({})
    setNoModel(false)
    setBusy(false)
    if (!english) void translate(false)
    // 语言切换才重取；translate 每次渲染新建，列进依赖会反复请求。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lang])

  // 没有可用模型时不出译文页签（spec 票 11），只留提示条。
  const hasTr = !english && !noModel
  const effTab: Tab = hasTr ? tab : 'original'
  const cur = entries.find((e) => e.id === sel) ?? entries[0]
  const tr = cur ? byId[cur.id] : undefined
  const reason = (code: string) => t(`settings.prompts_err_${code}`, { defaultValue: code })

  return (
    <div data-testid="prompts-section" style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10, lineHeight: 1.6, flexShrink: 0 }}>{t('settings.prompts_intro')}</div>
      {hasTr && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12, flexShrink: 0 }}>
          <button className="btn" disabled={busy} onClick={() => void translate(true)}>
            {busy ? t('settings.prompts_translating') : t('settings.prompts_retranslate')}
          </button>
          <span className="dim3" style={{ fontSize: 11 }}>{t('settings.prompts_reference')}</span>
        </div>
      )}
      {noModel && (
        <div className="row-line" role="alert" style={{ padding: '8px 10px', marginBottom: 12, fontSize: 12, display: 'flex', gap: 10, alignItems: 'center', flexShrink: 0 }}>
          <span>{t('settings.prompts_noModel')}</span>
          <button className="btn" onClick={onGoModels}>{t('settings.prompts_goModels')}</button>
        </div>
      )}
      {failure && <div role="alert" style={{ color: 'var(--err)', fontSize: 12, marginBottom: 12, flexShrink: 0 }}>{t('settings.prompts_failed', { error: failure })}</div>}
      <div style={{ flex: 1, minHeight: 0, display: 'flex', border: '1px solid var(--border)', borderRadius: 8, background: 'var(--bg-1)', overflow: 'hidden' }}>
        <div style={{ width: 232, flexShrink: 0, borderRight: '1px solid var(--border)', overflowY: 'auto', padding: '4px' }}>
          {GROUPS.map((g) => {
            const rows = entries.filter((e) => e.group === g)
            if (rows.length === 0) return null
            return (
              <div key={g}>
                <div className="dim3" style={{ fontSize: 11, fontWeight: 560, padding: '8px 8px 3px' }}>{t(`settings.prompts_group_${g}`)}</div>
                {rows.map((r) => {
                  const active = cur?.id === r.id
                  const errCode = hasTr ? byId[r.id]?.error : undefined
                  return (
                    <button
                      key={r.id}
                      data-prompt-id={r.id}
                      className="btn"
                      onClick={() => setSel(r.id)}
                      style={{
                        display: 'flex',
                        width: '100%',
                        alignItems: 'center',
                        gap: 6,
                        border: 'none',
                        borderRadius: 5,
                        background: active ? 'var(--bg-2)' : 'transparent',
                        color: active ? 'var(--text)' : 'var(--text-2)',
                        padding: '5px 8px',
                        textAlign: 'left',
                      }}
                    >
                      <span className="mono" style={{ fontSize: 11, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{r.id}</span>
                      {errCode && <span className="dot" style={{ background: 'var(--err)', flexShrink: 0 }} title={reason(errCode)} />}
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          {cur && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '10px 14px 8px', flexShrink: 0 }}>
                <span className="mono" style={{ fontSize: 12, fontWeight: 560 }}>{cur.id}</span>
                <span className="chip" style={{ fontSize: 10 }}>{t(`settings.prompts_group_${cur.group}`)}</span>
              </div>
              <div role="tablist" style={{ display: 'flex', gap: 14, padding: '0 14px', borderBottom: '1px solid var(--border)', flexShrink: 0 }}>
                <button role="tab" aria-selected={effTab === 'original'} className="btn" style={tabBtn(effTab === 'original')} onClick={() => setTab('original')}>
                  {t('settings.prompts_original')}
                </button>
                {hasTr && (
                  <button role="tab" aria-selected={effTab === 'tr'} className="btn" style={tabBtn(effTab === 'tr')} onClick={() => setTab('tr')}>
                    {t('settings.prompts_reference')}
                  </button>
                )}
              </div>
              <div style={{ flex: 1, minHeight: 0, overflowY: 'auto', padding: '10px 14px 14px' }}>
                {effTab === 'original' ? (
                  <pre className="mono" style={preStyle}>{cur.text}</pre>
                ) : tr?.error ? (
                  <div role="alert" style={{ color: 'var(--err)', fontSize: 12 }}>{t('settings.prompts_failed', { error: reason(tr.error) })}</div>
                ) : busy && !tr?.text ? (
                  <div className="dim3" style={{ fontSize: 12 }}>{t('settings.prompts_translating')}</div>
                ) : (
                  <pre data-translation className="mono" style={preStyle}>{tr?.text ?? ''}</pre>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
