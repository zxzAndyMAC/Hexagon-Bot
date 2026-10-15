// 离开未保存 LeaveGuard（components D17，canvas#leave；interaction §6「离开未保存」）。ui-redesign 票 09。
// A17 AlertDialog：标题「README.md 有未保存的修改」+ 说明「离开前要保存吗？放弃后修改无法找回。」
// + 按钮：放弃修改（ghost 红字，左）· 取消 · 保存 ⌘S（primary）。首焦「保存」，Esc = 取消。
// 切项目、关页签、进设置三处共用（store.confirmFileEdits 发起，ConfirmHost 按 variant 渲染本组件）。
//
// 键盘不交给 Radix 的 Esc：沿用 ConfirmDialog 的窗口捕获监听（cancelFileEdits / saveFile / discardFileEdits 绑定），
// 原生 <dialog open>（截图放大、设计稿预览）在前景时不响应——I1 / 2026-10-06：Esc 关预览不能顺手放弃后台脏稿。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor, matches, type ActionId } from '../../keymap'
import {
  AlertDialog, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { ShortcutKbd } from '@/components/ui/kbd'

export function LeaveGuard({ title, body, saveLabel, discardLabel, onSave, onDiscard, onCancel, saveAction = 'saveFile', discardAction, cancelAction = 'cancelFileEdits' }: {
  title: string
  body?: string
  saveLabel: string
  discardLabel: string
  onSave: () => Promise<void>
  onDiscard: () => Promise<void>
  onCancel: () => void
  saveAction?: ActionId
  discardAction?: ActionId
  cancelAction?: ActionId
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState<'save' | 'discard' | null>(null)
  const saveRef = useRef<HTMLButtonElement>(null)
  const discardRef = useRef<HTMLButtonElement>(null)
  const run = async (which: 'save' | 'discard') => {
    if (busy) return
    setBusy(which)
    try { await (which === 'save' ? onSave() : onDiscard()) } finally { setBusy(null) }
  }
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (document.querySelector('dialog[open]')) return
      if (busy) return
      if (matches(e, bindingFor(cancelAction))) { e.preventDefault(); e.stopImmediatePropagation(); onCancel() }
      else if (matches(e, bindingFor(saveAction))) { e.preventDefault(); e.stopImmediatePropagation(); saveRef.current?.click() }
      else if (discardAction && bindingFor(discardAction) && matches(e, bindingFor(discardAction))) { e.preventDefault(); e.stopImmediatePropagation(); discardRef.current?.click() }
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [busy, onCancel, saveAction, discardAction, cancelAction])
  return (
    <AlertDialog open>
      <AlertDialogContent busy={busy != null} data-leave-guard className="w-[420px]"
        onOpenAutoFocus={(e) => { e.preventDefault(); saveRef.current?.focus() }}
        onEscapeKeyDown={(e) => e.preventDefault()}>
        <AlertDialogTitle>{title}</AlertDialogTitle>
        {body ? <AlertDialogDescription className="mt-1">{body}</AlertDialogDescription> : <AlertDialogDescription className="sr-only">{title}</AlertDialogDescription>}
        <AlertDialogFooter className="mt-5 flex items-center gap-2">
          <Button ref={discardRef} data-leave="discard" variant="ghost" size="sm" className="text-red live:hover:text-red" disabled={busy != null}
            loading={busy === 'discard'} onClick={() => void run('discard')}>{discardLabel}</Button>
          <span className="flex-1" />
          <Button data-leave="cancel" variant="default" size="sm" disabled={busy != null} onClick={onCancel}>
            {t('agent.cancel')}{bindingFor(cancelAction) && <ShortcutKbd binding={bindingFor(cancelAction)} />}
          </Button>
          <Button ref={saveRef} data-leave="save" variant="primary" size="sm" loading={busy === 'save'} disabled={busy === 'discard'} onClick={() => void run('save')}>
            {saveLabel}<ShortcutKbd binding={bindingFor(saveAction)} />
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
