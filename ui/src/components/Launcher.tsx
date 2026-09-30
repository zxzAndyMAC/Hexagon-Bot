// 启动页（票 29）：最近项目列表 + 打开目录/新建项目入口。
// 未开项目时的全屏界面；「新建项目」→ 项目向导 Wizard。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, isTauri } from '../api'
import type { RecentProject } from '../gen/RecentProject'
import { Wizard } from './Wizard'
import { Icon } from './Icon'
import { SettingsPage } from './SettingsPage'
import { Row } from './Row'
import { bindingFor, formatBinding } from '../keymap'

export function Launcher({ onOpen }: { onOpen: () => void }) {
  const { t } = useTranslation()
  const [recents, setRecents] = useState<RecentProject[]>([])
  const [wizard, setWizard] = useState(false)
  const [settings, setSettings] = useState(false)
  const [err, setErr] = useState('')

  useEffect(() => {
    api.recentProjects().then(setRecents).catch(() => {})
    // 向导 keys 步等处的「去设置」与未开项目时的 ⌘, 都走 hexagon:open-settings
    // 广播（App.tsx 在项目未开时改发此事件）；收成 toggle 与工作台同键语义一致。
    const h = () => setSettings((v) => !v)
    window.addEventListener('hexagon:open-settings', h)
    return () => window.removeEventListener('hexagon:open-settings', h)
  }, [])

  const openDir = async (dir?: string) => {
    setErr('')
    try {
      let d = dir
      if (!d) {
        if (!isTauri) return
        const { open } = await import('@tauri-apps/plugin-dialog')
        const picked = await open({ directory: true })
        if (typeof picked !== 'string') return
        d = picked
      }
      await api.openRecent(d)
      onOpen()
    } catch (e) {
      setErr(errText(e))
    }
  }

  // 设置与工作台内同一个 SettingsPage——不另开一套（术语表：设置页 §19）。
  // 盖在向导上而不替换：向导保持挂载，步序与草稿不丢；返回时 keys 步轮询自刷新。
  const body = wizard ? (
    <Wizard onDone={onOpen} />
  ) : (
    <div
      data-tauri-drag-region
      style={{
        position: 'fixed', inset: 0, display: 'flex', flexDirection: 'column',
        alignItems: 'center', justifyContent: 'center', background: 'var(--bg)',
      }}
    >
      <div style={{ fontSize: 22, fontWeight: 600, marginBottom: 4 }}>Hexagon-Bot</div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 28 }}>{t('launch.title')}</div>

      <div className="panel" style={{ width: 520, padding: '14px 16px' }}>
        <div style={{ fontWeight: 510, fontSize: 13, marginBottom: 8 }}>{t('launch.recents')}</div>
        {recents.length === 0 && (
          <div className="dim3" style={{ fontSize: 12, padding: '12px 0' }}>{t('launch.empty')}</div>
        )}
        {/* 列表限高滚动：超过 ~7 行内部滚动，面板高度不随条数无限涨 */}
        <div style={{ maxHeight: 320, overflowY: 'auto', marginRight: -4, paddingRight: 4 }}>
          {recents.map((r) => {
          const removeBtn = (
            <button
              className="icon-btn"
              style={{ display: 'inline-flex', alignItems: 'center', flexShrink: 0 }}
              title={t('launch.remove')}
              aria-label={t('launch.remove')}
              onClick={(e) => {
                e.stopPropagation()
                void api.removeRecent(r.dir)
                  .then(() => api.recentProjects())
                  .then(setRecents)
                  .catch((x) => setErr(errText(x)))
              }}
            >
              <Icon name="close" size={11} />
            </button>
          )
          const info = (
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 510, color: r.exists ? undefined : 'var(--err)' }}>{r.name}</div>
              <div
                className="dim3 mono"
                style={{
                  fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                  color: r.exists ? undefined : 'var(--err)', opacity: r.exists ? undefined : 0.8,
                }}
              >
                {r.dir}
              </div>
            </div>
          )
          // 目录已删（exists=false）：不是可激活控件（open_recent 必报错），
          // 渲染成纯展示行——红字 + missing chip，只剩 × 移除钮可操作
          if (!r.exists) {
            return (
              <div
                key={r.dir}
                className="recent-row"
                data-missing="1"
                style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 10px', borderRadius: 8 }}
              >
                {info}
                <span className="chip err">{t('launch.missing')}</span>
                {removeBtn}
              </div>
            )
          }
          return (
            <Row
              key={r.dir}
              role="listitem"
              onClick={() => openDir(r.dir)}
              className="recent-row"
              style={{
                display: 'flex', alignItems: 'center', gap: 10, padding: '8px 10px',
                borderRadius: 8, cursor: 'pointer',
              }}
            >
              {info}
              <span className="chip mono">{t(`launch.mode_${r.mode}`)}</span>
              {removeBtn}
            </Row>
          )
        })}
        </div>
        {err && <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 8 }}>{err}</div>}
        <div style={{ display: 'flex', gap: 8, marginTop: 14 }}>
          {isTauri && (
            <button className="btn" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }} onClick={() => openDir()}>
              <Icon name="folder" size={12} /> {t('launch.openDir')}
            </button>
          )}
          <button className="btn primary" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }} onClick={() => setWizard(true)}>
            <Icon name="plus" size={12} /> {t('launch.newProject')}
          </button>
        </div>
      </div>
    </div>
  )

  return (
    <>
      {body}
      {/* 设置钮对启动页与向导全程恒显（owner 2026-09-30：向导期间也要能进
          设置）。zIndex 70：压过向导层 60；设置覆盖层 80 打开时自然盖住它。 */}
      <button
        className="btn"
        title={`${t('launch.settings')} ${formatBinding(bindingFor('settings'))}`}
        style={{ position: 'fixed', top: 16, right: 16, zIndex: 70, display: 'inline-flex', alignItems: 'center', gap: 5 }}
        onClick={() => setSettings(true)}
      >
        <Icon name="settings" size={13} /> {t('launch.settings')}
      </button>
      {settings && (
        <div style={{ position: 'fixed', inset: 0, zIndex: 80, background: 'var(--bg)' }}>
          <SettingsPage onBack={() => setSettings(false)} projectless />
        </div>
      )}
    </>
  )
}
