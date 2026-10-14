// 决策卡壳（ui-redesign 票 04，components D6 / D8，decisions#dock、decisions#cards）。
// 一张卡 = 头（28 图标块 + 标题 14/600 + 副行 12 ink-3）+ 正文（13 ink-2）+ 底（左说明 / 次动作 + 右按钮组）。
// 图标块颜色 = 类别（橙 warn 需授权 · 紫 flag 升级失速 · 红 err 不可逆 · 蓝 info 质量 · 灰 mute 未知 / 判定失败）；
// 主按钮颜色 = 后果（primary 常规，danger 只给发布与清理工作树）。
// 坞内（D6）由 PendingDock 提供外壳和头部的分页器 / 收起钮（DockChrome）；坞外是独立形 `dc`（圆角 14、raised）。
import { createContext, useContext, useState, type ReactNode } from 'react'
import { useUiStore } from '../store'
import { settleDecision } from '../decisions'
import { bindingFor, formatBinding, type ActionId } from '../keymap'
import { buttonVariants } from './ui/button'
import { Kbd } from './ui/kbd'
import { Spinner } from './ui/spinner'
import { Icon, type IconName } from './Icon'
import { cn } from '@/lib/utils'
import './decision.css'

export type DecisionCat = 'warn' | 'flag' | 'err' | 'info' | 'mute' | 'ok'

/** 坞内时由 PendingDock 提供：头部右侧的分页器与收起钮。 */
export const DockChrome = createContext<ReactNode | null>(null)

export function DecisionShell({ qid, cat, icon, title, sub, note, lead, actions, danger, children }: {
  /** 待决卡 id：裁决失败时在底栏上方显示 pendingError 红字（D6 错误态）。 */
  qid?: string
  cat: DecisionCat
  icon: IconName
  title: ReactNode
  sub?: ReactNode
  /** 底栏左侧说明（12 ink-3，例「批准后只放行这一次调用」）。 */
  note?: ReactNode
  /** 底栏左侧次动作（ghost：查看 diff、重新判定…）。 */
  lead?: ReactNode
  /** 底栏右侧按钮组（拒绝 / 主动作）。 */
  actions?: ReactNode
  /** 不可逆卡：独立形外加红 30% 环线（decisions#cards 发布确认）。 */
  danger?: boolean
  children?: ReactNode
}) {
  const chrome = useContext(DockChrome)
  const error = useUiStore((s) => (qid && s.pendingError?.id === qid ? s.pendingError.text : null))
  const body = (
    <>
      <div className="dock-h">
        <span className={`dock-ic ${cat}`}><Icon name={icon} size={16} /></span>
        <div className="dock-hl">
          <h4>{title}</h4>
          {sub != null && sub !== '' && <div className="dock-sub">{sub}</div>}
        </div>
        {chrome}
      </div>
      {children != null && <div className="dock-b">{children}</div>}
      {error && <div role="alert" className="dock-err" data-decision-error="">{error}</div>}
      {(lead || note || actions) && (
        <div className="dock-f">
          {lead}
          <span className="dock-note">{note}</span>
          {actions}
        </div>
      )}
    </>
  )
  if (chrome != null) return <div className="dock-card" data-danger={danger || undefined}>{body}</div>
  return <div className="dc" data-danger={danger || undefined}>{body}</div>
}

type Variant = 'default' | 'primary' | 'danger' | 'quiet' | 'ghost'

/** 决策按钮：A1 形（sm）。键位只在顶卡显示（interaction §2），按钮内 kbd 按平台渲染当前绑定。
 *  禁用用原生 disabled：裁决钮被禁用时任何路径都不能触发（键盘处理器读 button.disabled），
 *  原因按 D8「旁写原因」写在卡内，不靠聚焦读 tooltip——与 A1 的 aria-disabled 约定不同，见对账 §5 票 04。 */
export function DecisionButton({ variant = 'default', action, showKey, title, disabled, busy, onClick, children, ref, ...rest }: {
  ref?: React.Ref<HTMLButtonElement>
  variant?: Variant
  action?: ActionId
  showKey?: boolean
  title?: string
  disabled?: boolean
  busy?: boolean
  onClick: () => void
  children: ReactNode
} & Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'onClick' | 'children' | 'title'>) {
  const key = action ? formatBinding(bindingFor(action)) : ''
  const tip = title ?? key
  return (
    <button ref={ref} type="button" data-slot="button" data-variant={variant} title={tip || undefined}
      className={cn(buttonVariants({ variant, size: 'sm' }), 'disabled:cursor-default disabled:opacity-45')}
      disabled={disabled || busy} aria-busy={busy || undefined} onClick={onClick} {...rest}>
      {busy && <Spinner tone={variant === 'primary' || variant === 'danger' ? 'solid' : 'default'} />}
      {children}
      {showKey && key && <Kbd>{key}</Kbd>}
    </button>
  )
}

/** 结束这张卡的裁决（保存即消卡）：提交中按钮环形加载，卡先离坞；失败卡回坞顶 + 红字（settleDecision）。 */
export function SettleButton({ qid, run, ...props }: Omit<Parameters<typeof DecisionButton>[0], 'onClick' | 'busy'> & {
  qid: string
  run: () => Promise<unknown>
}) {
  const [busy, setBusy] = useState(false)
  return <DecisionButton {...props} busy={busy} onClick={() => {
    if (busy) return
    setBusy(true)
    void settleDecision(qid, run).finally(() => setBusy(false))
  }} />
}

/** kv 表（decisions#cards 发布 / 未知动作 / 工作树）：72 列标签 ink-3 + 值。 */
export function KeyValues({ rows, wide }: { rows: [ReactNode, ReactNode, string?][]; wide?: boolean }) {
  return (
    <dl className="dock-kv" data-wide={wide || undefined}>
      {rows.map(([k, v, cls], i) => (
        <div key={i} className="dock-kv-row"><dt>{k}</dt><dd className={cls}>{v}</dd></div>
      ))}
    </dl>
  )
}
