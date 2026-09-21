import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api, errText } from '../api'
import { Icon } from './Icon'

// ui-audit-2 票 09：`#` 路径补全走核 API（repo_paths IPC，仓根有界
// 遍历）——MOCK_PATHS 占位数据已退役。防抖 120ms + seq 乱序守卫。

// 发送历史（ui-audit 票 10 / P2-10）：session 内存不落盘——
// 空输入按 ↑ 回填上一条，继续 ↑ 向前翻、↓ 向后翻。
const HISTORY: string[] = []
const HISTORY_CAP = 50

export function Composer() {
  const { t } = useTranslation()
  const { team, invalidate, mode, fastRole, pushToast } = useUiStore()
  const [text, setText] = useState('')
  const [popup, setPopup] = useState<{ kind: '@' | '#'; items: { label: string; hint: string }[] } | null>(null)
  const [sel, setSel] = useState(0)
  const [histIdx, setHistIdx] = useState(-1)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const pathSeq = useRef(0)
  const pathTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const fetchPaths = (q: string) => {
    if (pathTimer.current) clearTimeout(pathTimer.current)
    const seq = ++pathSeq.current
    pathTimer.current = setTimeout(() => {
      api.repoPaths(q)
        .then((ps) => {
          if (pathSeq.current !== seq) return // 晚到的旧查询不盖新结果
          setPopup({
            kind: '#',
            items: ps.map((p) => ({
              label: `#${p}`,
              hint: p.endsWith('/') ? t('composer.directory') : t('composer.file'),
            })),
          })
          setSel(0)
        })
        .catch(() => { if (pathSeq.current === seq) setPopup(null) })
    }, 120)
  }

  const onChange = (v: string) => {
    setText(v)
    setHistIdx(-1) // 手动编辑即退出历史浏览
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
      setPopup({ kind: '#', items: [] }) // 先开空壳，结果异步到
      fetchPaths(last.slice(1))
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
    // ui-audit 票 04（P1-6）：发送失败 toast + 草稿保留——
    // 原先 await 裸抛，文案随输入框状态悬在用户面前却无任何反馈。
    try {
      await api.sendMessage(body)
      // 快速通道：消息即任务——发完直接派给通道角色跑一回合（票 26）
      if (mode === 'fastpath' && fastRole) {
        await api.dispatch(fastRole, body).catch(() => {})
      }
      setText('')
      setHistIdx(-1)
      HISTORY.unshift(body)
      if (HISTORY.length > HISTORY_CAP) HISTORY.pop()
      await invalidate()
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }

  return (
    <div style={{ position: 'relative', padding: '10px 14px', borderTop: '1px solid var(--border)', background: 'var(--bg-1)' }}>
      {popup && popup.items.length > 0 && (
        <div className="panel panel-float" style={{ position: 'absolute', bottom: '100%', left: 14, right: 14, marginBottom: 4, overflow: 'hidden', zIndex: 10, boxShadow: '0 8px 24px rgba(0,0,0,.28)' }}>
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
              <span className="mono" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                {popup.kind === '#' && <Icon name={it.label.endsWith('/') ? 'folder' : 'artifact'} size={11} />}
                {it.label}
              </span>
              <span className="dim3" style={{ fontSize: 11 }}>{it.hint}</span>
            </div>
          ))}
        </div>
      )}
      <div style={{ display: 'flex', gap: 8, alignItems: 'flex-end' }}>
        {/* ui-audit 票 10（P2-10）：单行 input → 自动增高 textarea（约 6 行上限内滚）。
            出处：Enter 行为变了——单行时代 Enter=发送是唯一语义；多行后
            Enter=发送、Shift+Enter=换行，用户习惯断层（粘 diff/多段指令进不来），
            placeholder 写明新键位。空输入 ↑ 回填发送历史（session 内存）。 */}
        <textarea
          id="composer-input"
          ref={inputRef}
          rows={1}
          value={text}
          onChange={(e) => {
            onChange(e.target.value)
            e.target.style.height = 'auto'
            e.target.style.height = `${Math.min(e.target.scrollHeight, 140)}px`
          }}
          onKeyDown={(e) => {
            if (popup && popup.items.length) {
              if (e.key === 'ArrowDown') { setSel((sel + 1) % popup.items.length); e.preventDefault(); return }
              if (e.key === 'ArrowUp') { setSel((sel - 1 + popup.items.length) % popup.items.length); e.preventDefault(); return }
              if (e.key === 'Enter' || e.key === 'Tab') { pick(popup.items[sel].label); e.preventDefault(); return }
              if (e.key === 'Escape') { setPopup(null); return }
            }
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); return }
            if (e.key === 'ArrowUp' && (!text || histIdx >= 0)) {
              e.preventDefault()
              const next = Math.min(histIdx + 1, HISTORY.length - 1)
              if (HISTORY[next] != null) { setText(HISTORY[next]); setHistIdx(next) }
              return
            }
            if (e.key === 'ArrowDown' && histIdx >= 0) {
              e.preventDefault()
              const next = histIdx - 1
              setHistIdx(next)
              setText(next >= 0 ? HISTORY[next] : '')
            }
          }}
          placeholder={`${t('composer.placeholder')} ${t('composer.multilineHint')}`}
          style={{
            flex: 1, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 8,
            padding: '8px 12px', outline: 'none', resize: 'none', lineHeight: 1.5,
            fontFamily: 'inherit', fontSize: 'inherit', overflowY: 'auto',
          }}
        />
        <button className="btn primary" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }} onClick={send}>
          <Icon name="send" size={12} /> {t('composer.send')}
        </button>
      </div>
    </div>
  )
}
