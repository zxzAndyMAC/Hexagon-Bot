import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type PendingQuestion } from '../api'
import { useUiStore } from '../store'
import { extractDiffBlock } from '../diff'
import { bindingFor, formatBinding } from '../keymap'
import { severityOf } from '../decisions'
import { Icon, type IconName } from './Icon'

function CardShell({ tone, icon, title, children }: {
  tone: 'ask' | 'stamp' | 'flag' | 'danger'
  icon: IconName
  title: React.ReactNode
  children: React.ReactNode
}) {
  const cls = { ask: 'card-ask', stamp: 'card-stamp', flag: 'card-flag', danger: 'card-danger' }[tone]
  const color = { ask: 'var(--accent)', stamp: 'var(--accent)', flag: 'var(--flag)', danger: 'var(--err)' }[tone]
  return (
    <div className={cls} style={{ padding: '10px 14px', margin: '6px 14px 0', ...(tone === 'danger' ? { borderColor: 'var(--err)' } : {}) }}>
      <div style={{ fontWeight: 510, color, marginBottom: 4, display: 'flex', alignItems: 'center', gap: 6 }}>
        <Icon name={icon} size={13} /> {title}
      </div>
      {children}
    </div>
  )
}

function Btn({ onClick, primary, danger, children }: {
  onClick: () => Promise<unknown>; primary?: boolean; danger?: boolean; children: React.ReactNode
}) {
  const [busy, setBusy] = useState(false)
  const refresh = useUiStore((s) => s.refresh)
  return (
    <button
      className={`btn ${primary ? 'primary' : ''} ${danger ? 'danger' : ''}`}
      disabled={busy}
      onClick={async () => {
        setBusy(true)
        try { await onClick(); await refresh() } finally { setBusy(false) }
      }}
    >
      {children}
    </button>
  )
}

export function PendingCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const [shape, setShape] = useState('')
  const p = q.payload
  const approveTip = formatBinding(bindingFor('approve'))
  const rejectTip = formatBinding(bindingFor('reject'))

  if (q.kind === 'permission') {
    const tool = String(p.tool ?? '')
    const input = p.input as Record<string, unknown> | undefined
    const summary = input ? Object.entries(input).map(([k, v]) => `${k}=${typeof v === 'string' ? v.slice(0, 60) : JSON.stringify(v)}`).join(' ') : ''
    const safety = Boolean(p.safety_net)
    return (
      <CardShell tone="ask" icon="warn" title={t('cards.ask')}>
        <div className="mono" style={{ fontSize: 12 }}>
          <strong>{tool}</strong> <span className="dim">{summary}</span>
        </div>
        <div className="dim3" style={{ fontSize: 11, margin: '4px 0 8px' }}>
          {String(p.reason ?? '')}
          {safety && <span className="chip err" style={{ marginLeft: 8 }}>{t('cards.askSafetyNet')}</span>}
        </div>
        {!safety && (
          <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
            <input
              value={shape}
              onChange={(e) => setShape(e.target.value)}
              placeholder={t('cards.shapeHint')}
              className="mono"
              style={{ flex: 1, fontSize: 11, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', outline: 'none' }}
            />
            <Btn onClick={() => api.answerPermission(q.id, true, shape || undefined)}>{t('cards.remember')}</Btn>
          </div>
        )}
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn primary onClick={() => api.answerPermission(q.id, true)}>{t('cards.allowOnce')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.answerPermission(q.id, false)}>{t('cards.deny')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'stamp' && !p.proposal_id) {
    return (
      <CardShell tone="stamp" icon="stamp" title={`${t('cards.stageStamp')} · ${String(p.stage ?? '')}`}>
        <StageArtifacts runId={String(p.run_id ?? '')} />
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.stamp()}>{t('cards.confirm')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.rejectStamp()}>{t('cards.reject')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'stamp' && p.proposal_id) {
    const warnings = (p.warnings as string[] | undefined) ?? []
    return (
      <CardShell tone="stamp" icon="hex" title={`${t('cards.proposalStamp')} · ${String(p.proposal_id)}`}>
        <div className="dim" style={{ fontSize: 12 }}>
          {t('cards.proposal')} · {String(p.surface ?? '')}
        </div>
        {warnings.length > 0 && (
          <div className="accent" style={{ fontSize: 12, margin: '4px 0', display: 'flex', alignItems: 'center', gap: 5 }}>
            <Icon name="warn" size={11} /> {String(p.warning_text ?? warnings.join(' + '))}
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.confirmProposal(q.id)}>{t('cards.confirm')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.rejectProposal(q.id, 'owner rejected')}>{t('cards.reject')}{top && ` ${rejectTip}`}</Btn>
          <Btn onClick={async () => {
            const props = await api.proposals()
            const pr = props.find((x) => String(x.id) === String(p.proposal_id))
            const ap = pr?.artifact_path ? String(pr.artifact_path) : null
            if (!ap) return
            const body = await api.artifactContent(ap)
            const diff = extractDiffBlock(body)
            if (diff) {
              openTab({ id: `patch:${String(p.proposal_id)}`, kind: 'diff', title: `${String(p.proposal_id)} diff`, patchText: diff })
            } else {
              openTab({ id: `art:${ap}`, kind: 'artifact', title: ap, path: ap })
            }
          }}>{t('cards.viewDiff')}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'publish') {
    return (
      <CardShell tone="danger" icon="publish" title={t('cards.publish')}>
        <div className="mono" style={{ fontSize: 12 }}>
          {String(p.baseline ?? 'main')} → <strong>{String(p.remote ?? 'origin')}</strong>
        </div>
        <div style={{ color: 'var(--err)', fontSize: 12, margin: '4px 0 8px' }}>
          {String(p.warning ?? t('cards.publishWarn'))}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn primary danger onClick={() => api.confirmPublish(q.id)}>{t('cards.publishConfirm')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.rejectPublish(q.id)}>{t('cards.reject')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'recovery') {
    return (
      <CardShell tone="ask" icon="warn" title={`${t('cards.recovery')} · ${String(p.stage ?? '')}`}>
        <div className="dim3" style={{ fontSize: 11, margin: '4px 0 8px' }}>
          {t('cards.recoveryHint')}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn primary onClick={() => api.recoverRun(String(p.run_id))}>{t('cards.recover')}{top && ` ${approveTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'install') {
    const net = p.net === true
    const creds = p.creds === true
    return (
      <CardShell tone="flag" icon="install" title={`${t('cards.install')} · ${String(p.name ?? '')}`}>
        <div className="dim" style={{ fontSize: 12 }}>
          <div className="mono">{String(p.source ?? '')}</div>
          <div className="mono dim3" style={{ marginTop: 2 }}>
            {String(p.command ?? '')}
            {Array.isArray(p.args) ? ` ${(p.args as unknown[]).join(' ')}` : ''}
          </div>
          <div style={{ marginTop: 4, display: 'flex', gap: 8 }}>
            <span className="chip">{net ? t('cards.installNet') : t('cards.installLocal')}</span>
            {creds && <span className="chip">{t('cards.installCreds')}</span>}
          </div>
          <div className="dim3" style={{ marginTop: 4 }}>{t('cards.installHint')}</div>
        </div>
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.resolveInstall(q.id, true)}>{t('cards.installRun')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.resolveInstall(q.id, false)}>{t('cards.rejectContinue')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'escalation') {
    const isContext = p.sub === 'context_overflow'
    return (
      <CardShell tone="flag" icon="escalate" title={isContext ? t('cards.contextOverflow') : t('cards.escalation')}>
        {isContext ? (
          <div className="dim" style={{ fontSize: 12 }}>
            <span className="mono">{String(p.role ?? '')}</span> · ~{String(p.est_tokens ?? '?')}/{String(p.cap ?? '?')} tok
            <div style={{ marginTop: 4 }}>{t('cards.contextHint')}</div>
          </div>
        ) : (
          <div className="dim" style={{ fontSize: 12 }}>
            flag <span className="mono">{String(p.flag_id ?? '')}</span> → <span className="mono">{String(p.target ?? '')}</span>
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.adjudicateFlag(q.id, true)}>{t('cards.agreeContinue')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.adjudicateFlag(q.id, false)}>{t('cards.rejectContinue')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  return (
    <CardShell tone="ask" icon="help" title={q.kind}>
      <pre className="mono dim" style={{ fontSize: 11, margin: 0 }}>{JSON.stringify(p)}</pre>
    </CardShell>
  )
}

function StageArtifacts({ runId }: { runId: string }) {
  const artifacts = useUiStore((s) => s.artifacts)
  const list = artifacts.filter((a) => a.stage_run_id === runId)
  if (!list.length) return null
  return (
    <div style={{ fontSize: 12 }}>
      {list.map((a) => (
        <div key={a.id} className="mono dim" style={{ padding: '1px 0' }}>
          {a.path} <span className="dim3">v{a.version}</span>
        </div>
      ))}
    </div>
  )
}

export function PendingCards() {
  const pending = useUiStore((s) => s.pending)
  const sorted = [...pending].sort((a, b) => severityOf(a) - severityOf(b))
  return (
    <>
      {sorted.map((q, i) => (
        <PendingCard key={q.id} q={q} top={i === 0} />
      ))}
    </>
  )
}
