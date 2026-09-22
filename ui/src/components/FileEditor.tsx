import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'

/// 中栏文件页（票 11）。右栏不渲染本组件。
/// 测试环境不挂 Monaco（happy-dom 没有 worker）；正文用 textarea，
/// 生产路径动态加载 monacoFile。两路都写同一个 textRef，保存口径一致。
/// `key={path}` 换文件时整页重挂，避免在 effect 里同步清状态。
export function FileEditor({ path }: { path: string }) {
  return <FileBody key={path} path={path} />
}

function FileBody({ path }: { path: string }) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const saveReq = useUiStore((s) => s.saveFileReq)
  const activeTab = useUiStore((s) => s.activeTab)
  const hostRef = useRef<HTMLDivElement>(null)
  const textRef = useRef('')
  const seenSave = useRef(saveReq)
  const saveRef = useRef<() => Promise<void>>(async () => {})
  const [text, setText] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [dirty, setDirty] = useState(false)
  const [busy, setBusy] = useState(false)
  const testing = import.meta.env.MODE === 'test'

  useEffect(() => {
    let live = true
    api.readRepoFile(path).then((body) => {
      if (!live) return
      textRef.current = body
      setText(body)
    }).catch((e) => {
      if (live) setErr(errText(e))
    })
    return () => { live = false }
  }, [path])

  useEffect(() => {
    if (testing || text == null) return
    const el = hostRef.current
    if (!el) return
    let dead = false
    let handle: { dispose: () => void } | undefined
    void import('./monacoFile').then(({ mountFileEditor }) => {
      if (dead || !hostRef.current) return
      handle = mountFileEditor(hostRef.current, path, text, (next) => {
        textRef.current = next
        setDirty(true)
      })
    }).catch((e) => setErr(errText(e)))
    return () => {
      dead = true
      handle?.dispose()
    }
  }, [path, text, testing])

  async function save() {
    if (text == null || busy) return
    setBusy(true)
    try {
      await api.writeRepoFile(path, textRef.current)
      setDirty(false)
      pushToast(t('file.saved'), 'ok')
    } catch (e) {
      pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    saveRef.current = save
  })

  useEffect(() => {
    if (saveReq === seenSave.current) return
    seenSave.current = saveReq
    if (activeTab !== `file:${path}`) return
    void saveRef.current()
  }, [saveReq, activeTab, path])

  const saveTip = `${t('file.save')} ${formatBinding(bindingFor('saveFile'))}`

  return (
    <div data-testid="file-editor" data-path={path} style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      <div className="row-line" style={{ padding: '8px 14px', display: 'flex', gap: 8, alignItems: 'center' }}>
        <span className="mono" style={{ fontWeight: 560, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{path}</span>
        <div style={{ flex: 1 }} />
        <button
          className="btn primary"
          type="button"
          title={saveTip}
          disabled={text == null || !dirty || busy}
          onClick={() => void save()}
          style={{ fontSize: 11, padding: '2px 8px' }}
        >
          {busy ? t('file.saving') : t('file.save')}
        </button>
      </div>
      {err && <div className="dim3" style={{ padding: 14, fontSize: 12 }}>{err}</div>}
      {text == null && !err && <div className="dim3" style={{ padding: 14, fontSize: 12 }}>{t('file.loading')}</div>}
      {text != null && testing && (
        <textarea
          aria-label={path}
          value={text}
          onChange={(e) => {
            textRef.current = e.target.value
            setText(e.target.value)
            setDirty(true)
          }}
          style={{ flex: 1, minHeight: 0, resize: 'none', border: 'none', background: 'transparent', color: 'inherit', fontFamily: 'inherit', padding: 14 }}
        />
      )}
      {text != null && !testing && <div ref={hostRef} style={{ flex: 1, minHeight: 0 }} />}
    </div>
  )
}
