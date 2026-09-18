import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import i18n, { SUPPORTED, setLang, type Locale } from '../i18n'
import { useUiStore, type ThemePref } from '../store'
import { api } from '../api'

const LANG_NAMES: Record<string, string> = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  en: 'English',
  ja: '日本語',
  es: 'Español',
  pt: 'Português',
  fr: 'Français',
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 0', borderBottom: '1px solid var(--border)' }}>
      <div style={{ flex: 1 }}>
        <div>{label}</div>
        {hint && <div className="dim3" style={{ fontSize: 11, marginTop: 2 }}>{hint}</div>}
      </div>
      {children}
    </div>
  )
}

export function SettingsDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation()
  const { themePref, setThemePref } = useUiStore()
  const [logOn, setLogOn] = useState(true)

  useEffect(() => {
    if (open) api.logEnabled().then(setLogOn)
  }, [open])

  if (!open) return null
  return (
    <div style={{ position: 'fixed', inset: 0, zIndex: 50 }} onClick={onClose}>
      <div
        className="panel"
        style={{
          position: 'absolute', top: 48, right: 12, width: 320, padding: '14px 16px',
          boxShadow: '0 8px 30px rgba(0,0,0,.3)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontWeight: 510, marginBottom: 4 }}>{t('settings.title')}</div>

        <Row label={t('settings.language')}>
          <select
            className="btn"
            value={i18n.language}
            onChange={(e) => setLang(e.target.value as Locale)}
          >
            {SUPPORTED.map((l) => (
              <option key={l} value={l}>{LANG_NAMES[l]}</option>
            ))}
          </select>
        </Row>

        <Row label={t('settings.theme')}>
          <div style={{ display: 'flex', gap: 4 }}>
            {(['light', 'dark', 'system'] as ThemePref[]).map((p) => (
              <button
                key={p}
                className={`btn ${themePref === p ? 'primary' : ''}`}
                onClick={() => setThemePref(p)}
              >
                {t(`settings.theme_${p}`)}
              </button>
            ))}
          </div>
        </Row>

        <Row label={t('settings.logging')} hint={t('settings.loggingHint')}>
          <input
            type="checkbox"
            checked={logOn}
            onChange={async (e) => {
              setLogOn(e.target.checked)
              await api.setLogEnabled(e.target.checked)
            }}
          />
        </Row>
      </div>
    </div>
  )
}
