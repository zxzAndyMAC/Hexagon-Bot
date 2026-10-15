// toast 宿主（ui-audit 票 01/04；ui-redesign 票 13 改 Sonner 形制，components A18）。
// 挂在 main.tsx 与 App 平级——设置页/启动页分支也能收到。
// 只承载负责人自己发起的操作结果；轮询失败由调用方节流后再推。
// store.toasts 是队列（调用方与测试读它）；本宿主把新项交给 Sonner，Sonner 管寿命
// （约 3.5s、悬停暂停、不抢焦点），关闭（到时 / 动作 / 被挤出）时回调 dismissToast 出队。
// F6（focusToasts）把焦点送到首条 toast 的动作上（A18 聚焦态：动作可 Tab 到）。
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { toastError } from '../toast'
import { useUiStore, type Toast } from '../store'
import { bindingFor, matches } from '../keymap'
import { Toaster, ToastPill } from '@/components/ui/sonner'

export function ToastHost() {
  const { t } = useTranslation()
  const toasts = useUiStore((s) => s.toasts)
  const shown = useRef(new Set<number>())

  useEffect(() => {
    const live = new Set(toasts.map((x) => x.id))
    for (const x of toasts) {
      if (shown.current.has(x.id)) continue
      shown.current.add(x.id)
      show(x)
    }
    // 队列里被外部清掉的（换项目清空等）同步关掉
    for (const id of [...shown.current]) {
      if (!live.has(id)) { shown.current.delete(id); toast.dismiss(id) }
    }
  }, [toasts])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!matches(e, bindingFor('focusToasts'))) return
      const target = document.querySelector<HTMLElement>('[data-slot="toast"] [data-toast-action]')
      if (!target) return
      e.preventDefault()
      target.focus()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  // hotkey 交给 keymap（focusToasts），Sonner 自带的 alt+T 关掉，免得与可改绑的键位并存。
  return <Toaster hotkey={[]} containerAriaLabel={t('feedback.region')} />
}

function show(t: Toast) {
  const dismiss = () => useUiStore.getState().dismissToast(t.id)
  toast.custom(
    (id) => (
      <ToastPill
        tone={t.tone}
        icon={t.icon}
        text={t.text}
        code={t.code}
        action={t.action?.label}
        onAction={t.action && (() => {
          toast.dismiss(id)
          void Promise.resolve().then(t.action!.run).catch((e) => toastError(e))
        })}
      />
    ),
    { id: t.id, onDismiss: dismiss, onAutoClose: dismiss },
  )
}
