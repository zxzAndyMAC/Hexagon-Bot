// 设置整页（票 29）：工作台整体换成设置页，不是弹层。
// 左 nav 七分区：通用/键盘/模型与凭据/权限/MCP 服务/用量/关于；
// 数据面未接的分区先占位。键盘分区支持重映射 + 冲突检测（app 级 hexagon.key.*）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import i18n, { SUPPORTED, setLang, type Locale } from '../i18n'
import { useUiStore, type ThemePref } from '../store'
import { api } from '../api'
import { ACTIONS, bindingFor, conflictFor, formatBinding, normalizeEvent, resetBinding, setBinding, type ActionId } from '../keymap'
import { Icon } from './Icon'

const LANG_NAMES: Record<string, string> = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  en: 'English',
  ja: '日本語',
  es: 'Español',
  pt: 'Português',
  fr: 'Français',
}

type Section = 'general' | 'keys' | 'models' | 'perms' | 'mcp' | 'usage' | 'about'
const SECTIONS: Section[] = ['general', 'keys', 'models', 'perms', 'mcp', 'usage', 'about']

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

function KeyRow({ id, onChanged }: { id: ActionId; onChanged: () => void }) {
  const { t } = useTranslation()
  const [capturing, setCapturing] = useState(false)
  const [conflict, setConflict] = useState<ActionId | null>(null)
  const [, setTick] = useState(0)

  useEffect(() => {
    if (!capturing) return
    const h = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') { setCapturing(false); return }
      const b = normalizeEvent(e)
      if (!b) return // 纯修饰键，继续等
      const c = conflictFor(id, b)
      if (c) { setConflict(c); setCapturing(false); return }
      setBinding(id, b)
      setConflict(null)
      setCapturing(false)
      setTick((n) => n + 1)
      onChanged()
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [capturing, id, onChanged])

  const label = ACTIONS.find((a) => a.id === id)?.labelKey ?? id
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 0', borderBottom: '1px solid var(--border)' }}>
      <div style={{ flex: 1, fontSize: 13 }}>{t(label)}</div>
      {conflict && (
        <span style={{ color: 'var(--accent)', fontSize: 11 }}>
          {t('settings.keyConflict', {
            action: t(ACTIONS.find((a) => a.id === conflict)?.labelKey ?? conflict),
          })}
        </span>
      )}
      <button
        className="btn mono"
        style={{ minWidth: 90 }}
        onClick={() => { setConflict(null); setCapturing(true) }}
      >
        {capturing ? t('settings.pressKey') : formatBinding(bindingFor(id))}
      </button>
      <button
        className="btn"
        style={{ fontSize: 11 }}
        onClick={() => { resetBinding(id); setConflict(null); setTick((n) => n + 1); onChanged() }}
      >
        {t('settings.resetKey')}
      </button>
    </div>
  )
}

export function SettingsPage({ onBack }: { onBack: () => void }) {
  const { t } = useTranslation()
  const { themePref, setThemePref } = useUiStore()
  const [section, setSection] = useState<Section>('general')
  const [logOn, setLogOn] = useState(true)
  const [, setKeyTick] = useState(0)

  useEffect(() => {
    api.logEnabled().then(setLogOn).catch(() => {})
  }, [])

  const bodies: Record<Section, React.ReactNode> = {
    general: (
      <>
        <Row label={t('settings.language')}>
          <select className="btn" value={i18n.language} onChange={(e) => setLang(e.target.value as Locale)}>
            {SUPPORTED.map((l) => (
              <option key={l} value={l}>{LANG_NAMES[l]}</option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.theme')}>
          <div style={{ display: 'flex', gap: 4 }}>
            {(['light', 'dark', 'system'] as ThemePref[]).map((p) => (
              <button key={p} className={`btn ${themePref === p ? 'primary' : ''}`} onClick={() => setThemePref(p)}>
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
              await api.setLogEnabled(e.target.checked).catch(() => {})
            }}
          />
        </Row>
      </>
    ),
    keys: (
      <>
        <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('settings.keysHint')}</div>
        {ACTIONS.map((a) => (
          <KeyRow key={a.id} id={a.id} onChanged={() => setKeyTick((n) => n + 1)} />
        ))}
      </>
    ),
    models: <div className="dim3" style={{ fontSize: 12, padding: '20px 0' }}>{t('settings.comingSoon')}</div>,
    perms: <div className="dim3" style={{ fontSize: 12, padding: '20px 0' }}>{t('settings.comingSoon')}</div>,
    mcp: <div className="dim3" style={{ fontSize: 12, padding: '20px 0' }}>{t('settings.comingSoon')}</div>,
    usage: <div className="dim3" style={{ fontSize: 12, padding: '20px 0' }}>{t('settings.comingSoon')}</div>,
    about: (
      <div style={{ fontSize: 12, lineHeight: 2 }}>
        <div><b>Hexagon-Bot</b> · v0.1.0</div>
        <div className="dim3">{t('settings.aboutLine')}</div>
      </div>
    ),
  }

  return (
    <div data-tauri-drag-region style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div
        data-tauri-drag-region
        className="row-line"
        style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 14px', background: 'var(--bg-1)', paddingLeft: '84px' }}
      >
        <button className="btn" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }} onClick={onBack}>
          <Icon name="arrow-left" size={12} /> {t('settings.back')}
        </button>
        <strong style={{ fontWeight: 510 }}>{t('settings.title')}</strong>
      </div>
      <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
        <div style={{ width: 180, padding: '14px 10px', borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column', gap: 2 }}>
          {SECTIONS.map((s) => (
            <button
              key={s}
              className="btn"
              style={{
                textAlign: 'left', border: 'none',
                background: section === s ? 'var(--bg-2)' : 'transparent',
                color: section === s ? 'var(--text)' : 'var(--text-2)',
              }}
              onClick={() => setSection(s)}
            >
              {t(`settings.nav_${s}`)}
            </button>
          ))}
        </div>
        <div style={{ flex: 1, padding: '16px 24px', overflowY: 'auto', maxWidth: 720 }}>
          <div style={{ fontWeight: 560, fontSize: 14, marginBottom: 10 }}>{t(`settings.nav_${section}`)}</div>
          {bodies[section]}
        </div>
      </div>
    </div>
  )
}
