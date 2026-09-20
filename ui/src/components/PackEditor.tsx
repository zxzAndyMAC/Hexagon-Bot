import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, isTauri } from '../api'
import { useUiStore } from '../store'

/** 流程包编辑器（票 31）：draft(pack.json) 的 JSON 编辑 → 校验保存/存模板/导出 YAML。
 *  编辑只碰 pack.json——pack.active.json（钉住副本）不动，走行中实例隔离。 */
export function PackEditor({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const { invalidate } = useUiStore()
  const [text, setText] = useState('')
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.packDraft()
      .then((v) => setText(JSON.stringify(v, null, 2)))
      .catch((e) => setErr(errText(e)))
  }, [])

  const validJson = (): string | null => {
    try { JSON.parse(text); return null } catch (e) { return String(e) }
  }

  const run = async (f: () => Promise<unknown>, ok: string) => {
    const je = validJson()
    if (je) { setErr(je); return }
    setBusy(true); setErr(null); setMsg(null)
    try {
      const r = await f()
      setMsg(typeof r === 'string' ? `${ok}: ${r}` : ok)
      await invalidate()
    } catch (e) { setErr(errText(e)) } finally { setBusy(false) }
  }

  const exportYaml = async () => {
    setBusy(true); setErr(null); setMsg(null)
    try {
      if (isTauri) {
        const { save } = await import('@tauri-apps/plugin-dialog')
        const path = await save({
          defaultPath: 'pack.yaml',
          filters: [{ name: 'YAML', extensions: ['yaml', 'yml'] }],
        })
        if (!path) { setBusy(false); return }
        await api.exportPackYaml(path)
        setMsg(`${t('pack.exported')}: ${path}`)
      } else {
        // 浏览器 dev：生成同形 YAML 下载
        const pack = JSON.parse(text)
        const list = (v?: string[]) => (v && v.length ? `[${v.join(', ')}]` : '[]')
        let y = `name: ${pack.name}\nversion: ${pack.version}\nstages:\n`
        for (const s of pack.stages ?? []) {
          y += `  - name: ${s.name}\n    roles: ${list(s.roles)}\n    due: ${list(s.due)}\n`
          if (s.checks?.length) y += `    checks: ${list(s.checks)}\n`
          if (s.reviews?.length) {
            y += '    reviews:\n'
            for (const r of s.reviews) y += `      - artifact_kind: ${r.artifact_kind}\n        reviewer: ${r.reviewer}\n`
          }
          if (s.stamp_point) y += '    stamp_point: true\n'
          if (s.backfill_edges?.length) {
            y += '    backfill_edges:\n'
            for (const e of s.backfill_edges) y += `      - [${e[0]}, ${e[1]}]\n`
          }
          if (s.consult_wake?.length) y += `    consult_wake: ${list(s.consult_wake)}\n`
        }
        const a = document.createElement('a')
        a.href = URL.createObjectURL(new Blob([y], { type: 'text/yaml' }))
        a.download = 'pack.yaml'
        a.click()
        URL.revokeObjectURL(a.href)
        setMsg(t('pack.exported'))
      }
    } catch (e) { setErr(errText(e)) } finally { setBusy(false) }
  }

  return (
    <div style={{
      position: 'fixed', inset: 0, background: 'rgba(0,0,0,.45)', zIndex: 60,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
    }} onClick={onClose}>
      <div
        style={{
          width: 'min(640px, 92vw)', maxHeight: '84vh', display: 'flex', flexDirection: 'column',
          background: 'var(--bg-0)', border: '1px solid var(--bd)', borderRadius: 10, padding: 14,
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontWeight: 560, fontSize: 13 }}>{t('pack.editor')}</div>
        <div className="dim3" style={{ fontSize: 10, margin: '4px 0 8px' }}>{t('pack.hint')}</div>
        {err && <div className="chip err" style={{ display: 'block', fontSize: 11, marginBottom: 8 }}>{err}</div>}
        {msg && <div className="chip ok" style={{ display: 'inline-block', fontSize: 11, marginBottom: 8 }}>{msg}</div>}
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          style={{
            flex: 1, minHeight: 320, padding: 10, fontSize: 11, fontFamily: 'monospace',
            background: 'var(--bg-1)', border: '1px solid var(--bd)', borderRadius: 8,
            color: 'var(--text)', resize: 'vertical',
          }}
        />
        <div style={{ display: 'flex', gap: 8, marginTop: 10, flexWrap: 'wrap' }}>
          <button className="btn primary" disabled={busy} onClick={() => run(() => api.savePackDraft(text), t('pack.draftSaved'))}>
            {t('pack.saveDraft')}
          </button>
          <button className="btn" disabled={busy} onClick={() => run(() => api.savePackTemplate(text), t('pack.templateSaved'))}>
            {t('pack.saveTemplate')}
          </button>
          <button className="btn" disabled={busy} onClick={exportYaml}>{t('pack.exportYaml')}</button>
          <div style={{ flex: 1 }} />
          <button className="btn" onClick={onClose}>{t('agent.cancel')}</button>
        </div>
      </div>
    </div>
  )
}
