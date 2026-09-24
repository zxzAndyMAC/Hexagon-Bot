import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { PromptEntry } from '../gen/PromptEntry'
import type { PromptTranslation } from '../gen/PromptTranslation'

// ADR 0071：发给模型的固定提示词是英文。这一页给负责人看原文和参考译文，
// 只读——提示词不在这里改（工作台约束不进改进提案的面，ADR 0045）。
const GROUPS = ['workbench', 'subagent', 'runtime', 'tools', 'judges', 'wizard', 'translator'] as const

export function PromptsSection({ onGoModels }: { onGoModels: () => void }) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const english = lang === 'en'
  const [entries, setEntries] = useState<PromptEntry[]>([])
  const [byId, setById] = useState<Record<string, PromptTranslation>>({})
  const [busy, setBusy] = useState(false)
  const [noModel, setNoModel] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
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

  // 没有可用模型时整页退回英文单栏（spec 票 11），只留提示条。
  const twoCols = !english && !noModel
  const reason = (code: string) => t(`settings.prompts_err_${code}`, { defaultValue: code })

  return (
    <div data-testid="prompts-section">
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10, lineHeight: 1.6 }}>{t('settings.prompts_intro')}</div>
      {twoCols && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12 }}>
          <button className="btn" disabled={busy} onClick={() => void translate(true)}>
            {busy ? t('settings.prompts_translating') : t('settings.prompts_retranslate')}
          </button>
          <span className="dim3" style={{ fontSize: 11 }}>{t('settings.prompts_reference')}</span>
        </div>
      )}
      {noModel && (
        <div className="row-line" role="alert" style={{ padding: '8px 10px', marginBottom: 12, fontSize: 12, display: 'flex', gap: 10, alignItems: 'center' }}>
          <span>{t('settings.prompts_noModel')}</span>
          <button className="btn" onClick={onGoModels}>{t('settings.prompts_goModels')}</button>
        </div>
      )}
      {failure && <div role="alert" style={{ color: 'var(--err)', fontSize: 12, marginBottom: 12 }}>{t('settings.prompts_failed', { error: failure })}</div>}
      {GROUPS.map((g) => {
        const rows = entries.filter((e) => e.group === g)
        if (rows.length === 0) return null
        const groupError = twoCols ? rows.map((r) => byId[r.id]?.error).find(Boolean) : undefined
        return (
          <section key={g} style={{ marginBottom: 18 }}>
            <div style={{ fontWeight: 560, fontSize: 13, marginBottom: 6 }}>{t(`settings.prompts_group_${g}`)}</div>
            {groupError && <div role="alert" style={{ color: 'var(--err)', fontSize: 11, marginBottom: 6 }}>{t('settings.prompts_failed', { error: reason(groupError) })}</div>}
            {rows.map((r) => (
              <div key={r.id} data-prompt-id={r.id} style={{ display: 'grid', gridTemplateColumns: twoCols ? '1fr 1fr' : '1fr', gap: 10, marginBottom: 10 }}>
                <div>
                  <div className="dim3" style={{ fontSize: 11, marginBottom: 2 }}>{r.id} · {t('settings.prompts_original')}</div>
                  <pre className="mono" style={{ whiteSpace: 'pre-wrap', fontSize: 11, margin: 0, padding: 8, background: 'var(--bg-1)', borderRadius: 4 }}>{r.text}</pre>
                </div>
                {twoCols && (
                  <div>
                    <div className="dim3" style={{ fontSize: 11, marginBottom: 2 }}>{t('settings.prompts_reference')}</div>
                    <pre data-translation style={{ whiteSpace: 'pre-wrap', fontSize: 11, margin: 0, padding: 8, background: 'var(--bg-1)', borderRadius: 4 }}>{byId[r.id]?.text ?? ''}</pre>
                  </div>
                )}
              </div>
            ))}
          </section>
        )
      })}
    </div>
  )
}
