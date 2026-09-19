// 启动页（票 29）：最近项目列表 + 打开目录/新建项目入口。
// 未开项目时的全屏界面；「新建项目」→ 项目向导 Wizard。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import i18n, { SUPPORTED, setLang, type Locale } from '../i18n'
import { api, isTauri } from '../api'
import { useUiStore, type ThemePref } from '../store'
import { Wizard } from './Wizard'
import { Icon } from './Icon'
import { ProviderManager } from './ProviderManager'

interface Recent {
  dir: string
  name: string
  mode: string
  opened_at: number
}

const LANG_NAMES: Record<string, string> = {
  'zh-CN': '简体中文', 'zh-TW': '繁體中文', en: 'English', ja: '日本語',
  es: 'Español', pt: 'Português', fr: 'Français',
}

/// 启动页设置层（不进向导就能配供应商/语言/主题）：
/// 向导 keys 步的供应商缺口在此补齐——这是修「卡在模型步」的正路。
function LauncherSettings({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const { themePref, setThemePref } = useUiStore()
  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 70, display: 'flex',
        alignItems: 'center', justifyContent: 'center', background: 'rgba(0,0,0,.35)',
      }}
      onClick={onClose}
    >
      <div
        className="panel"
        style={{ width: 560, maxHeight: '84vh', overflowY: 'auto', padding: '18px 20px' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ display: 'flex', alignItems: 'center', marginBottom: 14 }}>
          <strong style={{ fontWeight: 560, fontSize: 15, flex: 1 }}>{t('launch.settings')}</strong>
          <button className="btn" onClick={onClose}><Icon name="close" size={12} /></button>
        </div>
        <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 6 }}>
          <span className="dim3" style={{ fontSize: 12, width: 90 }}>{t('settings.language')}</span>
          <select className="btn" value={i18n.language} onChange={(e) => setLang(e.target.value as Locale)}>
            {SUPPORTED.map((l) => (
              <option key={l} value={l}>{LANG_NAMES[l]}</option>
            ))}
          </select>
          <span className="dim3" style={{ fontSize: 12, marginLeft: 12 }}>{t('settings.theme')}</span>
          {(['light', 'dark', 'system'] as ThemePref[]).map((p) => (
            <button key={p} className={`btn ${themePref === p ? 'primary' : ''}`} style={{ fontSize: 11 }} onClick={() => setThemePref(p)}>
              {t(`settings.theme_${p}`)}
            </button>
          ))}
        </div>
        <div style={{ borderTop: '1px solid var(--border)', margin: '10px 0 12px' }} />
        <ProviderManager />
      </div>
    </div>
  )
}

export function Launcher({ onOpen }: { onOpen: () => void }) {
  const { t } = useTranslation()
  const [recents, setRecents] = useState<Recent[]>([])
  const [wizard, setWizard] = useState(false)
  const [settings, setSettings] = useState(false)
  const [err, setErr] = useState('')

  useEffect(() => {
    api.recentProjects().then(setRecents).catch(() => {})
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
      setErr(String(e))
    }
  }

  if (wizard) return <Wizard onDone={onOpen} />

  return (
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
      {settings && <LauncherSettings onClose={() => setSettings(false)} />}

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
}
