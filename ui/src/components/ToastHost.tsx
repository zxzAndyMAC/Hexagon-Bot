// toast 宿主（ui-audit 票 01/04）：变更失败的用户可见出口。
// 挂在 main.tsx 与 App 平级——设置页/启动页分支也能收到。
// 只承载用户发起的变更反馈；轮询失败由调用方节流后再推。
import { useUiStore } from '../store'

export function ToastHost() {
  const toasts = useUiStore((s) => s.toasts)
  const dismissToast = useUiStore((s) => s.dismissToast)
  if (!toasts.length) return null
  return (
    <div className="toast-host" role="status" aria-live="polite">
      {toasts.map((t) => (
        <button
          key={t.id}
          className={`toast ${t.tone}`}
          onClick={() => dismissToast(t.id)}
        >
          {t.text}
        </button>
      ))}
    </div>
  )
}
