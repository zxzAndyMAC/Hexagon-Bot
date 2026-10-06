// 应用内确认层（ui-audit 票 01/03）：替代原生 confirm()。
// 三级确认政策的承载件——L2 流程破坏性操作（rewind/skip）与
// palette 危险项统一走这里；Esc/点遮罩取消，Enter 确认。
// 出处：原生 confirm 与危险度倒挂（可逆的 sleepAll 弹窗、rewind 裸跑），
// 且脱离设计语言——ui-audit report P1-8。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore, type ConfirmReq } from '../store'
import { errText } from '../api'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'

export function ConfirmDialog({
  title,
  body,
  danger,
  confirmLabel,
  input,
  onConfirm,
  onCancel,
  alternate,
  confirmAction,
  cancelAction,
}: {
  title: string
  body?: string
  danger?: boolean
  confirmLabel?: string
  input?: { placeholder?: string; required?: boolean }
  onConfirm: (inputValue: string) => unknown | Promise<unknown>
  onCancel: () => void
  alternate?: ConfirmReq['alternate']
  confirmAction?: ActionId
  cancelAction?: ActionId
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [inputVal, setInputVal] = useState('')
  const confirmRef = useRef<HTMLButtonElement>(null)
  const alternateRef = useRef<HTMLButtonElement>(null)
  const hint = (label: string, action?: ActionId) => {
    const binding = action ? formatBinding(bindingFor(action)) : ''
    return binding ? `${label} · ${binding}` : label
  }

  useEffect(() => {
    confirmRef.current?.focus()
    const h = (e: KeyboardEvent) => {
      // I1 / 2026-10-06: native previews occupy the browser's top layer.
      // Their dismissal must not also cancel an underlying draft decision.
      if (document.querySelector('dialog[open]')) return
      if (busy) return
      if (cancelAction ? matches(e, bindingFor(cancelAction)) : e.key === 'Escape') {
        e.preventDefault(); e.stopImmediatePropagation(); onCancel()
      } else if (confirmAction && matches(e, bindingFor(confirmAction))) {
        e.preventDefault(); e.stopImmediatePropagation(); confirmRef.current?.click()
      } else if (alternate?.action && matches(e, bindingFor(alternate.action))) {
        e.preventDefault(); e.stopImmediatePropagation(); alternateRef.current?.click()
      }
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [onCancel, busy, cancelAction, confirmAction, alternate])

  const confirm = async () => {
    if (busy) return // 防双击双发
    if (input?.required && !inputVal.trim()) return // 必填输入不落空理由
    setBusy(true)
    try {
      await onConfirm(inputVal.trim())
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 80,
        background: 'rgba(0,0,0,.45)', display: 'flex',
        alignItems: 'center', justifyContent: 'center',
      }}
      onClick={() => { if (!busy) onCancel() }}
      role="presentation"
    >
      <div
        className="panel panel-float"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        style={{
          width: 'min(420px, 92vw)', padding: 16,
          boxShadow: '0 12px 40px rgba(0,0,0,.4)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontWeight: 560, fontSize: 13 }}>{title}</div>
        {body && <div className="dim3" style={{ fontSize: 12, marginTop: 6, whiteSpace: 'pre-wrap' }}>{body}</div>}
        {input && (
          <input
            value={inputVal}
            onChange={(e) => setInputVal(e.target.value)}
            placeholder={input.placeholder}
            className="mono"
            autoFocus /* 有输入框时焦点进框——确认分级靠按钮色，不靠焦点位置 */
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void confirm() } }}
            style={{
              width: '100%', marginTop: 10, fontSize: 12, boxSizing: 'border-box',
              background: 'var(--bg-2)', border: '1px solid var(--border)',
              borderRadius: 6, padding: '5px 9px', outline: 'none',
            }}
          />
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 14, justifyContent: 'flex-end' }}>
          <button className="btn" title={hint(t('agent.cancel'), cancelAction)} disabled={busy} onClick={onCancel}>
            {t('agent.cancel')}
          </button>
          {alternate && <button ref={alternateRef} className="btn" title={hint(alternate.label, alternate.action)} disabled={busy} onClick={async () => {
            if (busy) return
            setBusy(true)
            try { await alternate.run() } finally { setBusy(false) }
          }}>{alternate.label}</button>}
          <button
            ref={confirmRef}
            title={hint(confirmLabel ?? t('cards.confirm'), confirmAction)}
            className={`btn primary${danger ? ' danger' : ''}`}
            disabled={busy || Boolean(input?.required && !inputVal.trim())}
            onClick={() => void confirm()}
          >
            {confirmLabel ?? t('cards.confirm')}
          </button>
        </div>
      </div>
    </div>
  )
}

/** 确认层宿主：读 store.confirmReq，挂 main.tsx 全局可达。
 *  确认后 run + invalidate；失败走 toast 出口并收层（ui-audit 票 03/04）。 */
export function ConfirmHost() {
  const req = useUiStore((s) => s.confirmReq)
  const clear = useUiStore((s) => s.clearConfirm)
  const invalidate = useUiStore((s) => s.invalidate)
  const pushToast = useUiStore((s) => s.pushToast)
  if (!req) return null
  const run = async (action: () => unknown | Promise<unknown>) => {
    let succeeded = false
    try {
      await action()
      if (!req.skipRefresh) await invalidate()
      succeeded = true
    } catch (e) {
      pushToast(errText(e), 'err')
    } finally {
      // An old asynchronous confirmation cannot close a newer dialog.
      if (useUiStore.getState().confirmReq === req) clear(succeeded)
    }
  }
  return (
    <ConfirmDialog
      title={req.title}
      body={req.body}
      danger={req.danger}
      confirmLabel={req.confirmLabel}
      confirmAction={req.confirmAction}
      cancelAction={req.cancelAction}
      input={req.input}
      onCancel={() => clear()}
      alternate={req.alternate && { ...req.alternate, run: () => run(req.alternate!.run) }}
      onConfirm={(v) => run(() => req.run(v))}
    />
  )
}
