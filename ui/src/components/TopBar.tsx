import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api } from '../api'
import { capReached, centsToMc, fmtTok } from '../usage'
import { bindingFor, formatBinding } from '../keymap'
import { Icon } from './Icon'

interface Recent {
  dir: string
  name: string
  mode: string
}

export function TopBar({ onSettings, onProjectClosed }: { onSettings: () => void; onProjectClosed: () => void }) {
  const { t } = useTranslation()
  const { projectName, packName, mode, autonomy, usageTotal, pending, refresh, setRailOpen, setSideTab, railOpen } = useUiStore()
  const [menuOpen, setMenuOpen] = useState(false)
  const [recents, setRecents] = useState<Recent[]>([])
  const menuRef = useRef<HTMLDivElement>(null)
  const fmt = (mc: number) => `¥${(mc / 100000).toFixed(1)}`
  const limitMc = usageTotal?.limit_cents != null ? centsToMc(usageTotal.limit_cents) : null
  const capped = capReached(usageTotal?.spent_mc ?? 0, usageTotal?.limit_cents)

  useEffect(() => {
    if (!menuOpen) return
    api.recentProjects().then(setRecents).catch(() => {})
    const h = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false)
    }
    window.addEventListener('mousedown', h)
    return () => window.removeEventListener('mousedown', h)
  }, [menuOpen])

  const locatePending = () => {
    const zone = document.getElementById('pending-zone')
    zone?.scrollTo({ top: 0, behavior: 'smooth' })
    zone?.classList.add('flash-zone')
    setTimeout(() => zone?.classList.remove('flash-zone'), 1400)
  }

  const switchTo = async (dir: string) => {
    setMenuOpen(false)
    await api.openRecent(dir)
    await refresh()
  }

  const backToLauncher = async () => {
    setMenuOpen(false)
    await api.closeProject().catch(() => {})
    onProjectClosed()
  }

  const others = recents.filter((r) => r.name !== projectName)

  return (
    <header
      className="row-line"
      style={{
        display: 'flex', alignItems: 'center', gap: 10,
        padding: '8px 14px 8px 84px', // macOS overlay 红绿灯让位
        background: 'var(--bg-1)',
      }}
    >
      {/* 项目名 + 切换下拉（ADR 0051：切换 = 重载整个投影） */}
      <div ref={menuRef} style={{ position: 'relative' }}>
        <button
          className="btn"
          style={{ border: 'none', background: 'transparent', fontWeight: 510, fontSize: 13, padding: '2px 6px', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          onClick={() => setMenuOpen(!menuOpen)}
          title={t('topbar.projectMenu')}
        >
          {projectName} <Icon name="chevron-down" size={10} />
        </button>
        {menuOpen && (
          <div className="panel" style={{ position: 'absolute', top: '100%', left: 0, zIndex: 40, minWidth: 240, padding: 6, boxShadow: '0 8px 30px rgba(0,0,0,.35)' }}>
            {others.length > 0 && (
              <>
                <div className="dim3" style={{ fontSize: 10, padding: '4px 8px', textTransform: 'uppercase' }}>{t('topbar.recentProjects')}</div>
                {others.map((r) => (
                  <div
                    key={r.dir}
                    onClick={() => void switchTo(r.dir)}
                    style={{ padding: '6px 8px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}
                    className="recent-row"
                  >
                    {r.name}
                    <span className="dim3 mono" style={{ fontSize: 10, marginLeft: 6 }}>{r.dir}</span>
                  </div>
                ))}
                <div style={{ borderTop: '1px solid var(--border)', margin: '4px 0' }} />
              </>
            )}
            <div
              onClick={() => void backToLauncher()}
              style={{ padding: '6px 8px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}
              className="recent-row"
            >
              {t('topbar.backToLauncher')}
            </div>
          </div>
        )}
      </div>
      <span className="chip mono">{mode === 'fastpath' ? t('topbar.fastpath') : packName ?? t('topbar.pack')}</span>
      <span className="chip">{t(`autonomy.${autonomy}`)}</span>
      <button
        className={`chip ${capped ? 'err' : 'mono'}`}
        style={{ cursor: 'pointer', fontSize: 11, ...(capped ? { animation: 'pulse-amber 1.6s infinite' } : {}) }}
        title={t('usage.openDashboard')}
        onClick={() => {
          setSideTab('usage')
          if (!railOpen) setRailOpen(true)
        }}
      >
        {capped ? <><Icon name="warn" size={10} /> {t('usage.capHit')}</> : `${t('topbar.usage')} ${usageTotal ? fmtTok(usageTotal.tokens) : '—'}`}
        {usageTotal && limitMc != null && ` · ${fmt(usageTotal.spent_mc)}/¥${usageTotal.limit_cents! / 100}`}
      </button>
      <div data-tauri-drag-region style={{ flex: 1, alignSelf: 'stretch' }} />
      {pending.length > 0 && (
        <button
          className="chip amber"
          style={{ cursor: 'pointer', border: '1px solid var(--accent-border)', animation: 'pulse-amber 1.6s infinite' }}
          onClick={locatePending}
          title={`${formatBinding(bindingFor('approve'))} / ${formatBinding(bindingFor('reject'))}`}
        >
          <Icon name="warn" size={11} /> {t('cards.pending', { count: pending.length })}
        </button>
      )}
      <button
        className="btn danger"
        style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}
        title={t('topbar.sleepAllHint')}
        onClick={async () => {
          if (confirm(t('topbar.sleepAllConfirm'))) {
            await api.sleepAll()
            await refresh()
          }
        }}
      >
        <Icon name="sleep" size={11} /> {t('topbar.sleepAll')}
      </button>
      <button
        className="btn"
        title={`${t('topbar.settings')} ${formatBinding(bindingFor('settings'))}`}
        onClick={onSettings}
      >
        {t('topbar.settings')}
      </button>
    </header>
  )
}
