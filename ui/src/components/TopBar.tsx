import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api, errText } from '../api'
import type { SandboxStatus } from '../gen/SandboxStatus'
import { capReached, centsToMc, fmtTok, perAgentSeries } from '../usage'
import { MultiLine } from './UsageTab'
import { bindingFor, formatBinding, isMac } from '../keymap'
import { runStageOp } from '../stageops'
import { Icon } from './Icon'

interface Recent {
  dir: string
  name: string
  mode: string
}

export function TopBar({ onSettings, onProjectClosed }: { onSettings: () => void; onProjectClosed: () => void }) {
  const { t } = useTranslation()
  const { projectName, packName, mode, autonomy, usageTotal, usageSeries7d, team, pending, reviewRows, refresh, refreshSlow, setRailOpen, setSideTab, railOpen, away, markAway, markBack, openPendingDialog, pushToast } = useUiStore()
  // 请求在飞时先显示所选档；成功后以 store 读回为准，失败则丢掉乐观值。
  const [autonomyPending, setAutonomyPending] = useState<string | null>(null)
  const autonomyShown = autonomyPending ?? autonomy
  const [menuOpen, setMenuOpen] = useState(false)
  // 票 17（方向卡 2）：用量 chip 悬停 300ms 出 sparkline 浮层——
  // 数据全部走 store 缓存（usageSeries7d/usageTotal），零新增 IPC。
  const [sparkOpen, setSparkOpen] = useState(false)
  const sparkTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const sparkShow = () => { sparkTimer.current = setTimeout(() => setSparkOpen(true), 300) }
  const sparkHide = () => { clearTimeout(sparkTimer.current); setSparkOpen(false) }
  const [recents, setRecents] = useState<Recent[]>([])
  // 票 06：沙箱实况徽章——纯平台探测，挂载即取（同项目内不变）。
  const [sandbox, setSandbox] = useState<SandboxStatus | null>(null)
  useEffect(() => {
    api.sandboxStatus().then(setSandbox).catch(() => setSandbox(null))
  }, [projectName])
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

  const changeAutonomy = async (lv: string) => {
    if (lv === autonomy) return
    setAutonomyPending(lv)
    try {
      await api.setAutonomy(lv)
      await refreshSlow(['info'])
    } catch (e) {
      pushToast(errText(e), 'err')
    } finally {
      setAutonomyPending(null)
    }
  }

  return (
    <header
      className="row-line"
      style={{
        display: 'flex', alignItems: 'center', gap: 10,
        // macOS overlay 红绿灯让位；非 macOS 不留 84px 空档（票 11 / P2-17）
        padding: `8px 14px 8px ${isMac ? 84 : 14}px`,
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
          {projectName || t('app.untitledProject')} <Icon name="chevron-down" size={10} />
        </button>
        {menuOpen && (
          /* 浮层必须 .panel-float 不透明底（workbench-polish 01 设计规范）——.panel 的 bg-1 在暗色主题是透明的，叠在内容上透字 */
          <div className="panel panel-float" style={{ position: 'absolute', top: '100%', left: 0, zIndex: 40, minWidth: 240, padding: 6 }}>
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
      <select
        className="chip"
        aria-label={t('topbar.autonomyMenu')}
        title={t('topbar.autonomyMenu')}
        value={autonomyShown}
        onChange={(e) => void changeAutonomy(e.target.value)}
        style={{ cursor: 'pointer', maxWidth: 240 }}
      >
        {(['L0', 'L1', 'L2', 'L3', 'L4'] as const).map((lv) => (
          <option key={lv} value={lv}>{t(`autonomy.${lv}`)}</option>
        ))}
      </select>
      {sandbox && (
        <span
          className={`chip ${sandbox.available ? 'ok' : 'amber'}`}
          title={`${t(sandbox.available ? 'topbar.sandboxOnTip' : 'topbar.sandboxOffTip')} · ${sandbox.note}`}
          style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}
        >
          <Icon name="shield" size={10} /> {t(sandbox.available ? 'topbar.sandboxOn' : 'topbar.sandboxOff')}
        </span>
      )}
      {/* 票 16（方向卡 1）：值守切换——away 时琥珀脉动，与待决指示同色系 */}
      <button
        className={`chip chip-btn ${away ? 'amber' : ''}`}
        style={{ display: 'inline-flex', alignItems: 'center', gap: 4, ...(away ? { animation: 'pulse-amber 1.6s infinite' } : {}) }}
        title={t('topbar.awayHint')}
        onClick={() => void (away ? markBack() : markAway())}
      >
        <Icon name="sleep" size={10} /> {t(away ? 'topbar.awayActive' : 'topbar.away')}
      </button>
      <div style={{ position: 'relative' }} onMouseEnter={sparkShow} onMouseLeave={sparkHide}>
        <button
          className={`chip chip-btn ${capped ? 'err' : 'mono'}`}
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
        {sparkOpen && (
          <div
            className="panel panel-float"
            style={{
              position: 'absolute', top: '100%', left: 0, marginTop: 6, zIndex: 50,
              width: 240, padding: '8px 10px',
            }}
          >
            <div className="dim3" style={{ fontSize: 10, marginBottom: 4 }}>{t('usage.spark7d')}</div>
            <MultiLine
              labels={perAgentSeries(usageSeries7d).labels}
              series={perAgentSeries(usageSeries7d).series}
              roleOf={(id) => team.find((m) => m.id === id)?.role ?? id ?? '—'}
            />
            <div className="dim3" style={{ fontSize: 10, margin: '6px 0 3px' }}>{t('usage.sparkToday')}</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <div style={{ flex: 1, height: 4, borderRadius: 2, background: 'var(--bg-2)', overflow: 'hidden' }}>
                <div
                  style={{
                    height: '100%',
                    width: `${limitMc ? Math.min(100, ((usageTotal?.spent_mc ?? 0) / limitMc) * 100) : 0}%`,
                    background: capped ? 'var(--err)' : 'var(--accent)',
                  }}
                />
              </div>
              <span className="mono dim3" style={{ fontSize: 10 }}>
                {usageTotal ? fmt(usageTotal.spent_mc) : '—'}{limitMc != null ? ` / ¥${usageTotal!.limit_cents! / 100}` : ''}
              </span>
            </div>
          </div>
        )}
      </div>
      <div data-tauri-drag-region style={{ flex: 1, alignSelf: 'stretch' }} />
      {(pending.length + reviewRows.length) > 0 && (
        <button
          className="chip chip-btn amber"
          data-pending-count={pending.length + reviewRows.length}
          style={{ border: '1px solid var(--accent-border)', animation: 'pulse-amber 1.6s infinite' }}
          onClick={openPendingDialog}
          title={t('cards.pendingReopen', {
            approve: formatBinding(bindingFor('approve')),
            reject: formatBinding(bindingFor('reject')),
          })}
        >
          <Icon name="warn" size={11} /> {t('cards.pending', { count: pending.length + reviewRows.length })}
        </button>
      )}
      <button
        className="btn danger"
        style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}
        title={t('topbar.sleepAllHint')}
        onClick={() => {
          // L1 可逆（团队列表随时唤醒）——ADR 0056 确认分级：无确认直执行，
          // 修倒挂（原生 confirm 曾挡在可逆操作上而 rewind 裸跑）。
          runStageOp('sleepAll')
        }}
      >
        <Icon name="sleep" size={11} /> {t('topbar.sleepAll')}
      </button>
      <button
        className="btn"
        style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}
        title={`${t('topbar.settings')} ${formatBinding(bindingFor('settings'))}`}
        onClick={onSettings}
      >
        <Icon name="settings" size={11} /> {t('topbar.settings')}
      </button>
    </header>
  )
}
