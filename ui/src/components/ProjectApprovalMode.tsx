import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ApprovalModeStatus } from '../gen/ApprovalModeStatus'
import { ApprovalModeSelector } from './ApprovalModeSelector'

/** Mounted afresh with each workspace. Never inherit another project's broad mode. */
export function ProjectApprovalMode({ placement = 'above' }: { placement?: 'above' | 'below' }) {
  const { t } = useTranslation()
  const [status, setStatus] = useState<ApprovalModeStatus | null>(null)
  const [failed, setFailed] = useState(false)
  const [retry, setRetry] = useState(0)
  const revision = useUiStore(s => s.timelineFacts?.approval_mode_revision ?? 0)
  const container = useRef<HTMLDivElement>(null)
  const generation = useRef(0)
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    const current = ++generation.current
    void api.approvalMode().then(value => {
      if (mounted.current && current === generation.current) { setStatus(value); setFailed(false) }
    }).catch(() => { if (mounted.current && current === generation.current) setFailed(true) })
  }, [revision, retry])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.defaultPrevented && matches(event, bindingFor('approvalMode'))) {
        event.preventDefault(); container.current?.querySelector('button')?.click()
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])
  return <div ref={container}>
    {status && !failed ? <ApprovalModeSelector key={status.project_root} value={status.mode} placement={placement}
      shortcut={formatBinding(bindingFor('approvalMode'))}
      onChange={async mode => {
        // Supersede any stale read before waiting for the host's persisted result.
        if (!mounted.current || !status.project_root) throw new Error('Workspace identity unavailable')
        const expectedRoot = status.project_root
        const current = ++generation.current
        const saved = await api.setApprovalMode(mode, expectedRoot)
        if (!mounted.current || current !== generation.current || saved.project_root !== expectedRoot) return
        setStatus(saved)
        void useUiStore.getState().refreshFast().catch(e => useUiStore.getState().pushToast(errText(e), 'err'))
      }} /> : <button type="button" className="approval-mode-trigger" disabled={!failed} onClick={() => { setFailed(false); setRetry(v => v + 1) }} title={t(failed ? 'approvalMode.readFailed' : 'approvalMode.loading')}>{t(failed ? 'approvalMode.readFailed' : 'approvalMode.loading')}</button>}
  </div>
}
