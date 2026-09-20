// 启动页（票 29）：最近项目列表 + 打开目录/新建项目入口。
// 未开项目时的全屏界面；「新建项目」→ 项目向导 Wizard。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, isTauri } from '../api'
import { Wizard } from './Wizard'
import { Icon } from './Icon'
import { SettingsPage } from './SettingsPage'

interface Recent {
  dir: string
  name: string
  mode: string
  opened_at: number
}

export function Launcher({ onOpen }: { onOpen: () => void }) {
  const { t } = useTranslation()
  const [recents, setRecents] = useState<Recent[]>([])
  const [wizard, setWizard] = useState(false)
  const [settings, setSettings] = useState(false)
  const [err, setErr] = useState('')

  useEffect(() => {
    api.recentProjects().then(setRecents).catch(() => {})
    // 向导 keys 步等处的「去设置」走 hexagon:open-settings 广播
    const h = () => setSettings(true)
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
      <button
        className="btn"
        title={t('launch.settings')}
        style={{ position: 'absolute', top: 16, right: 16, display: 'inline-flex', alignItems: 'center', gap: 5 }}
        onClick={() => setSettings(true)}
      >
        <Icon name="settings" size={13} /> {t('launch.settings')}
      </button>
      <div style={{ fontSize: 22, fontWeight: 600, marginBottom: 4 }}>Hexagon-Bot</div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 28 }}>{t('launch.title')}</div>

      <div className="panel" style={{ width: 520, padding: '14px 16px' }}>
        <div style={{ fontWeight: 510, fontSize: 13, marginBottom: 8 }}>{t('launch.recents')}</div>
        {recents.length === 0 && (
          <div className="dim3" style={{ fontSize: 12, padding: '12px 0' }}>{t('launch.empty')}</div>
        )}
        {recents.map((r) => (
          <div
            key={r.dir}
            onClick={() => openDir(r.dir)}
            className="recent-row"
            style={{
              display: 'flex', alignItems: 'center', gap: 10, padding: '8px 10px',
              borderRadius: 8, cursor: 'pointer',
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 510 }}>{r.name}</div>
              <div
                className="dim3 mono"
                style={{ fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
              >
                {r.dir}
              </div>
            </div>
            <span className="chip mono">{t(`launch.mode_${r.mode}`)}</span>
          </div>
        ))}
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
      {settings && (
        <div style={{ position: 'fixed', inset: 0, zIndex: 80, background: 'var(--bg)' }}>
          <SettingsPage onBack={() => setSettings(false)} />
        </div>
      )}
    </>
  )
}
