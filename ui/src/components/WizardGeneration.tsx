import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { Button } from '@/components/ui/button'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Icon } from './Icon'

// MR-07 (test3 11): wizard generation failures stay on the step that asked.
// Codes come from core `failure_code` as `model_<code>`; only these have a
// settings remedy. Transient codes and stop simply offer the same retry.
// ui-redesign 票 10：换皮成 D11「向导与设置内」形——inset 块 + C5 头 +「重试 alt+mod+Y」，
// 「停止 alt+mod+X」在向导底栏；行为不变。
const SETTINGS_CODES = new Set(['model_missing_credential', 'model_credential_store', 'model_authentication', 'model_configuration', 'model_request_rejected', 'model_quota'])

export type GenerationFailure = { step: string; code: string; retry: () => void }

function useBinding(action: 'retryWizardGeneration' | 'stopWizardGeneration', active: boolean, run: () => void) {
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || e.defaultPrevented || !matches(e, bindingFor(action))) return
      e.preventDefault(); run()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
}

/** Retry the failed request with the retained input; never auto-confirms. */
export function GenerationRetry({ failure, step }: { failure: GenerationFailure | null; step: string }) {
  const { t } = useTranslation()
  const shown = failure?.step === step ? failure : null
  useBinding('retryWizardGeneration', !!shown, () => shown?.retry())
  if (!shown) return null
  return (
    <div
      data-generation-failure={shown.code}
      className="mt-3 flex flex-wrap items-center gap-2 rounded-[10px] bg-inset px-3 py-2.5 text-[12.5px] shadow-[inset_0_0_0_1px_var(--line-soft)]"
    >
      <Icon name="warn" size={14} className="text-orange" />
      <span className="min-w-0 flex-1 text-ink-2">{t('wizard.generationKept')}</span>
      <Button size="xs" title={[t('wizard.retryGeneration'), formatBinding(bindingFor('retryWizardGeneration'))].join(' · ')} onClick={shown.retry}>
        {t('wizard.retryGeneration')}<ShortcutKbd binding={bindingFor('retryWizardGeneration')} />
      </Button>
      {SETTINGS_CODES.has(shown.code) && (
        <Button size="xs" variant="ghost" title={t('modelRecovery.openModels')} onClick={() => { api.openSettings().catch(() => {}) }}>
          {t('modelRecovery.openModels')}
        </Button>
      )}
    </div>
  )
}

/** Stop the in-flight generation; its late result is discarded by core. */
export function StopGeneration({ busy }: { busy: boolean }) {
  const { t } = useTranslation()
  const stop = () => { api.stopWizardGeneration().catch(() => {}) }
  useBinding('stopWizardGeneration', busy, stop)
  if (!busy) return null
  return (
    <Button size="sm" variant="ghost" data-stop-generation
      title={[t('wizard.stopGenerationHint'), formatBinding(bindingFor('stopWizardGeneration'))].join(' · ')}
      onClick={stop}>
      {t('wizard.stopGeneration')}<ShortcutKbd binding={bindingFor('stopWizardGeneration')} />
    </Button>
  )
}
