// 应用内确认层（ui-audit 票 01/03；ui-redesign 票 13 迁到共享 A17 AlertDialog，feedback#confirm）：替代原生 confirm()。
// 三级确认政策的承载件——L2 流程破坏性操作（rewind/skip）与
// palette 危险项统一走这里；Esc 取消，Enter 确认（安全类主按钮首焦 / 输入框内回车）。
// 出处：原生 confirm 与危险度倒挂（可逆的 sleepAll 弹窗、rewind 裸跑），
// 且脱离设计语言——ui-audit report P1-8。
// 首焦（A17 聚焦行）：破坏性（danger）= 取消，安全类（全员休眠，ADR 0056 附录 / 负责人 2026-10-14）= 主按钮。
// 曾经的写法：一律聚焦确认钮——danger 确认上回车即执行，与「破坏性默认聚焦取消」相反。
// 键盘不交给 Radix 的 Esc：窗口捕获监听按 keymap 绑定处理（同 LeaveGuard），
// 原生 <dialog open>（截图放大）在前景时不响应——I1 / 2026-10-06：Esc 关预览不能顺手取消后台确认。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore, type ConfirmReq } from '../store'
import { toastError } from '../toast'
import { bindingFor, matches, type ActionId } from '../keymap'
import { LeaveGuard } from './canvas/LeaveGuard'
import {
  AlertDialog, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Kbd, ShortcutKbd } from '@/components/ui/kbd'

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
  const [busy, setBusy] = useState<'confirm' | 'alternate' | null>(null)
  const [inputVal, setInputVal] = useState('')
  const confirmRef = useRef<HTMLButtonElement>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const alternateRef = useRef<HTMLButtonElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const kbd = (action?: ActionId) => (action && bindingFor(action) ? <ShortcutKbd binding={bindingFor(action)} /> : null)

  useEffect(() => {
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
    setBusy('confirm')
    try {
      await onConfirm(inputVal.trim())
    } finally {
      setBusy(null)
    }
  }
  const runAlternate = async () => {
    if (busy || !alternate) return
    setBusy('alternate')
    try { await alternate.run() } finally { setBusy(null) }
  }
  // 安全类且无输入时主按钮首焦，↵ 即确认——键位提示只在这种时候成立。
  const enterConfirms = !danger && !input

  return (
    <AlertDialog open onOpenChange={(open) => { if (!open && !busy) onCancel() }}>
      <AlertDialogContent
        busy={busy != null}
        data-confirm-dialog
        className="w-[min(430px,calc(100vw-48px))] gap-0 px-5 pt-5 pb-3.5"
        onOpenAutoFocus={(e) => {
          e.preventDefault()
          // 有输入框时焦点进框——确认分级靠按钮色，不靠焦点位置
          const first = input ? inputRef : danger ? cancelRef : confirmRef
          first.current?.focus()
        }}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <AlertDialogTitle>{title}</AlertDialogTitle>
        {body
          ? <AlertDialogDescription className="mt-1 whitespace-pre-wrap">{body}</AlertDialogDescription>
          : <AlertDialogDescription className="sr-only">{title}</AlertDialogDescription>}
        {input && (
          <Input
            ref={inputRef}
            value={inputVal}
            onChange={(e) => setInputVal(e.target.value)}
            placeholder={input.placeholder}
            className="mt-3 font-mono"
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void confirm() } }}
          />
        )}
        <AlertDialogFooter className="mt-5">
          <Button ref={cancelRef} size="sm" data-confirm="cancel" disabled={busy != null} onClick={onCancel}>
            {t('agent.cancel')}{kbd(cancelAction) ?? <Kbd>Esc</Kbd>}
          </Button>
          {alternate && (
            <Button ref={alternateRef} size="sm" data-confirm="alternate" loading={busy === 'alternate'}
              disabled={busy === 'confirm'} onClick={() => void runAlternate()}>
              {alternate.label}{kbd(alternate.action)}
            </Button>
          )}
          <Button
            ref={confirmRef}
            size="sm"
            data-confirm="confirm"
            variant={danger ? 'danger' : 'primary'}
            loading={busy === 'confirm'}
            disabled={busy === 'alternate' || Boolean(input?.required && !inputVal.trim())}
            onClick={() => void confirm()}
          >
            {confirmLabel ?? t('cards.confirm')}{kbd(confirmAction) ?? (enterConfirms ? <Kbd>↵</Kbd> : null)}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/** 确认层宿主：读 store.confirmReq，挂 main.tsx 全局可达。
 *  确认后 run + invalidate；失败走 toast 出口并收层（ui-audit 票 03/04）。 */
export function ConfirmHost() {
  const { t } = useTranslation()
  const req = useUiStore((s) => s.confirmReq)
  const clear = useUiStore((s) => s.clearConfirm)
  const invalidate = useUiStore((s) => s.invalidate)
  if (!req) return null
  const run = async (action: () => unknown | Promise<unknown>) => {
    let succeeded = false
    try {
      await action()
      if (!req.skipRefresh) await invalidate()
      succeeded = true
    } catch (e) {
      toastError(e)
    } finally {
      // An old asynchronous confirmation cannot close a newer dialog.
      if (useUiStore.getState().confirmReq === req) clear(succeeded)
    }
  }
  // 票 09（D17）：离开未保存走 LeaveGuard 形制（放弃在左、取消、保存 ⌘S），提交 / 失败口径与通用确认层相同。
  if (req.variant === 'leave') {
    return (
      <LeaveGuard
        title={req.title}
        body={req.body}
        saveLabel={req.confirmLabel ?? t('file.save')}
        discardLabel={req.alternate?.label ?? t('file.discard')}
        saveAction={req.confirmAction}
        discardAction={req.alternate?.action}
        cancelAction={req.cancelAction}
        onSave={() => run(() => req.run())}
        onDiscard={() => run(req.alternate?.run ?? (() => {}))}
        onCancel={() => clear()}
      />
    )
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
