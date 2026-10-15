import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { PromptEntry } from '../gen/PromptEntry'
import type { PromptTranslation } from '../gen/PromptTranslation'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { cn } from '@/lib/utils'
import { ModelRecoveryInline } from './ModelRecoveryInline'
import { Icon } from './Icon'
import { InlineError, ListDetail } from './settings/kit'

// ADR 0071：发给模型的固定提示词是英文。这一页给负责人看原文和参考译文，
// 只读——提示词不在这里改（工作台约束不进改进提案的面，ADR 0045）。
// 布局（2026-09 owner 报告）：左侧按组列条目、右侧详情——原先全量平铺 +
// 原文|译文并排双列，长文被砍半宽还两列各滚各的。改清单-详情后译文走页签，
// 页签态跨条目保持（审译文的人逐条过不用每行点一次）。
// ui-redesign 票 11（components E8，settings#logs 右半）：E2 列表 | 详情（分组），详情头右侧 A7 形页签
// 「原文 / 参考译文」（界面英文或没有可用模型时不出译文页签），inset 等宽正文，脚注 + 重新翻译 xs。
// 译文失败（D11 设置形）：原地红字写原因，保留缓存译文并标明是缓存，凭据类失败给「打开模型服务」。
const GROUPS = ['workbench', 'subagent', 'runtime', 'tools', 'judges', 'wizard', 'translator'] as const

type Tab = 'original' | 'tr'

/** A7 分段控件外形的页签（A22「卡片内页签用 A7 形态」）：role=tab，←→ 切换。 */
function PromptTabs({ tab, hasTr, onTab }: { tab: Tab; hasTr: boolean; onTab: (t: Tab) => void }) {
  const { t } = useTranslation()
  const item = (value: Tab, label: string) => (
    <button
      key={value}
      type="button"
      role="tab"
      aria-selected={tab === value}
      tabIndex={tab === value ? 0 : -1}
      onClick={() => onTab(value)}
      onKeyDown={(e) => {
        if (!hasTr || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight')) return
        e.preventDefault()
        onTab(tab === 'original' ? 'tr' : 'original')
      }}
      className={cn(
        'hx-press hx-focus inline-flex h-6 cursor-pointer items-center rounded-full px-2.5 text-[12px] whitespace-nowrap text-ink-2 live:hover:text-ink',
        tab === value && 'bg-surface font-medium text-ink [--hx-shadow:var(--shadow-btn)]',
      )}
    >
      {label}
    </button>
  )
  return (
    <div role="tablist" className="inline-flex w-fit flex-none items-center rounded-full bg-canvas p-0.5 shadow-[inset_0_0_0_1px_var(--line-soft)]">
      {item('original', t('settings.prompts_original'))}
      {hasTr && item('tr', t('settings.prompts_reference'))}
    </div>
  )
}

const preCls = 'm-0 whitespace-pre-wrap rounded-[var(--r-card)] bg-inset p-3 font-mono text-[12px] leading-[1.7] text-ink-2 shadow-[inset_0_0_0_1px_var(--line-soft)]'

export function PromptsSection({ onGoModels }: { onGoModels: () => void }) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const english = lang === 'en'
  const [entries, setEntries] = useState<PromptEntry[]>([])
  const [loaded, setLoaded] = useState(false)
  const [byId, setById] = useState<Record<string, PromptTranslation>>({})
  const [busy, setBusy] = useState(false)
  const [noModel, setNoModel] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const [sel, setSel] = useState<string | null>(null)
  const [tab, setTab] = useState<Tab>('original')
  // 切语言时上一种语言的请求可能还没回；序号对不上的结果丢弃，免得旧译文写进新语言。
  const seq = useRef(0)

  useEffect(() => {
    api.promptCatalog().then(setEntries).catch((e) => setFailure(errText(e))).finally(() => setLoaded(true))
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
  const reason = (code: string) => code.startsWith('model_') ? t(`errors.${code}`, { defaultValue: code }) : t(`settings.prompts_err_${code}`, { defaultValue: code })
  const settingsCode = (code?: string | null) => !!code && ['model_missing_credential', 'model_credential_store', 'model_authentication', 'model_configuration', 'model_request_rejected', 'model_quota'].includes(code)
  const ordered = GROUPS.flatMap((g) => entries.filter((e) => e.group === g))

  return (
    <div data-testid="prompts-section" className="flex min-h-0 flex-1 flex-col">
      <ListDetail<PromptEntry>
        label={t('settings.nav_prompts')}
        items={ordered}
        loading={!loaded}
        itemKey={(e) => e.id}
        filterText={(e) => `${e.id} ${e.text}`}
        groupOf={(e) => e.group}
        groupLabel={(g) => t(`settings.prompts_group_${g}`)}
        selected={cur?.id ?? null}
        onSelect={(k) => setSel(k)}
        renderItem={(r) => {
          const errCode = hasTr ? byId[r.id]?.error : undefined
          return (
            <span data-prompt-id={r.id} className="flex min-w-0 flex-1 items-center gap-2">
              <span className="min-w-0 flex-1 truncate font-mono text-[12px]">{r.id}</span>
              {errCode && <span title={reason(errCode)} className="inline-flex"><Dot tone="err" /></span>}
            </span>
          )
        }}
      >
        <p className="mb-4 text-[12.5px] leading-[1.6] text-ink-3">{t('settings.prompts_intro')}</p>
        {noModel && (
          <div role="alert" className="mb-4 flex items-center gap-2.5 rounded-[var(--r-card)] bg-orange-tint px-3 py-2 text-[12.5px] text-ink">
            <Icon name="warn" size={14} className="flex-none text-orange" />
            <span className="min-w-0 flex-1">{t('settings.prompts_noModel')}</span>
            <Button size="xs" onClick={onGoModels}>{t('settings.prompts_goModels')}</Button>
          </div>
        )}
        {failure && <InlineError text={t('settings.prompts_failed', { error: failure })} />}
        {cur && (
          <div data-prompt-detail={cur.id}>
            <div className="flex items-center gap-2">
              <b className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold text-ink">{cur.id}</b>
              <PromptTabs tab={effTab} hasTr={hasTr} onTab={setTab} />
            </div>
            <div className="mt-3">
              {effTab === 'original' ? (
                <pre className={preCls}>{cur.text}</pre>
              ) : tr?.error ? (
                <>
                  {/* 票 08（D11 向导与设置内）：与向导生成失败同组件，页签内红字形 */}
                  <ModelRecoveryInline form="text" text={t(tr.cached ? 'settings.prompts_failedCached' : 'settings.prompts_failed', { error: reason(tr.error) })}
                    actions={settingsCode(tr.error) && <Button size="xs" onClick={onGoModels}>{t('settings.prompts_goModels')}</Button>} />
                  {tr.cached && tr.text && <pre data-translation data-cached className={cn(preCls, 'mt-2')}>{tr.text}</pre>}
                </>
              ) : busy && !tr?.text ? (
                <div className="text-[12.5px] text-ink-3">{t('settings.prompts_translating')}</div>
              ) : (
                <pre data-translation className={preCls}>{tr?.text ?? ''}</pre>
              )}
            </div>
            {hasTr && (
              <div className="mt-3 flex items-center gap-2">
                <span className="min-w-0 flex-1 text-[12px] text-ink-3">{t('settingsUi.promptsReferenceNote')}</span>
                <Button size="xs" loading={busy} onClick={() => void translate(true)}>
                  {!busy && <Icon name="refresh" size={14} />}{busy ? t('settings.prompts_translating') : t('settings.prompts_retranslate')}
                </Button>
              </div>
            )}
          </div>
        )}
      </ListDetail>
    </div>
  )
}
