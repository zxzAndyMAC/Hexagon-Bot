import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'

// MR-07 (test3 11): wizard generation failures stay on the step that asked.
// Codes come from core `failure_code` as `model_<code>`; only these have a
// settings remedy. Transient codes and stop simply offer the same retry.
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
  return <div data-generation-failure={shown.code} style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 6 }}>
    <span className="dim3" style={{ fontSize: 11 }}>{t('wizard.generationKept')}</span>
    <button type="button" className="btn" title={[t('wizard.retryGeneration'), formatBinding(bindingFor('retryWizardGeneration'))].join(' · ')}
      onClick={shown.retry}>{t('wizard.retryGeneration')}</button>
    {SETTINGS_CODES.has(shown.code) && <button type="button" className="btn" title={t('modelRecovery.openModels')}
      onClick={() => { api.openSettings().catch(() => {}) }}>{t('modelRecovery.openModels')}</button>}
  </div>
}

/** Stop the in-flight generation; its late result is discarded by core. */
export function StopGeneration({ busy }: { busy: boolean }) {
  const { t } = useTranslation()
  const stop = () => { api.stopWizardGeneration().catch(() => {}) }
  useBinding('stopWizardGeneration', busy, stop)
  if (!busy) return null
  return <button type="button" className="btn" data-stop-generation
    title={[t('wizard.stopGenerationHint'), formatBinding(bindingFor('stopWizardGeneration'))].join(' · ')}
    onClick={stop}>{t('wizard.stopGeneration')}</button>
}
