import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api } from '../api'

// 仓库路径补全的占位数据源——真实实现走核 API 列目录（票 21+ 接）
const MOCK_PATHS = [
  'src/', 'src/api.rs', 'src/main.rs', 'docs/', 'AGENTS.md', '.github/workflows/ci.yml',
]

export function Composer() {
  const { t } = useTranslation()
  const { team, refresh, mode, fastRole } = useUiStore()
  const [text, setText] = useState('')
  const [popup, setPopup] = useState<{ kind: '@' | '#'; items: { label: string; hint: string }[] } | null>(null)
  const [sel, setSel] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  const onChange = (v: string) => {
    setText(v)
    const last = v.slice(v.lastIndexOf(' ') + 1)
    if (last.startsWith('@')) {
      const q = last.slice(1)
      setPopup({
        kind: '@',
        items: team
          .filter((m) => m.role.includes(q))
          .map((m) => ({ label: `@${m.role}`, hint: t('composer.mentionHint') })),
      })
      setSel(0)
    } else if (last.startsWith('#')) {
      const q = last.slice(1)
      setPopup({
        kind: '#',
        items: MOCK_PATHS.filter((p) => p.includes(q)).map((p) => ({
          label: `#${p}`,
          hint: p.endsWith('/') ? t('composer.directory') : t('composer.file'),
        })),
      })
      setSel(0)
    } else {
      setPopup(null)
    }
  }

  const pick = (label: string) => {
    const head = text.slice(0, text.lastIndexOf(' ') + 1)
    setText(`${head}${label} `)
    setPopup(null)
    inputRef.current?.focus()
  }

  const send = async () => {
    if (!text.trim()) return
    const body = text
    await api.sendMessage(body)
    // 快速通道：消息即任务——发完直接派给通道角色跑一回合（票 26）
    if (mode === 'fastpath' && fastRole) {
      await api.dispatch(fastRole, body).catch(() => {})
    }
    setText('')
    await refresh()
  }

  return (
    <div style={{ position: 'relative', padding: '10px 14px', borderTop: '1px solid var(--border)', background: 'var(--bg-1)' }}>
      {popup && popup.items.length > 0 && (
        <div className="panel" style={{ position: 'absolute', bottom: '100%', left: 14, right: 14, marginBottom: 4, overflow: 'hidden', zIndex: 10 }}>
          <div className="sys-row" style={{ padding: '4px 10px' }}>
            {popup.kind === '@' ? t('composer.mentionHint') : t('composer.pathHint')}
          </div>
          {popup.items.map((it, i) => (
            <div
              key={it.label}
              onClick={() => pick(it.label)}
              style={{
                padding: '5px 10px', cursor: 'pointer', display: 'flex', justifyContent: 'space-between',
                background: i === sel ? 'var(--bg-2)' : 'transparent',
                color: popup.kind === '@' ? 'var(--accent)' : 'var(--flag)',
              }}
            >
              <span className="mono">{it.label}</span>
              <span className="dim3" style={{ fontSize: 11 }}>{it.hint}</span>
            </div>
          ))}
        </div>
      )}
      <div style={{ display: 'flex', gap: 8 }}>
        <input
          ref={inputRef}
          value={text}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (popup && popup.items.length) {
              if (e.key === 'ArrowDown') { setSel((sel + 1) % popup.items.length); return }
              if (e.key === 'ArrowUp') { setSel((sel - 1 + popup.items.length) % popup.items.length); return }
              if (e.key === 'Enter' || e.key === 'Tab') { pick(popup.items[sel].label); e.preventDefault(); return }
              if (e.key === 'Escape') { setPopup(null); return }
            }
            if (e.key === 'Enter') send()
          }}
          placeholder={t('composer.placeholder')}
          style={{ flex: 1, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 8, padding: '8px 12px', outline: 'none' }}
        />
        <button className="btn primary" onClick={send}>{t('composer.send')}</button>
      </div>
    </div>
  )
}
