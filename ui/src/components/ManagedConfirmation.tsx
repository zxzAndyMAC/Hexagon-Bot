import { useEffect, useLayoutEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { blockBackgroundDecision } from '../modalDecisionGuard'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import { useDelegation } from '../useDelegation'
import { fmtYuan } from '../usage'
import { Icon } from './Icon'
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogError, AlertDialogFooter, AlertDialogIcon, AlertDialogTitle,
} from './ui/alert-dialog'
import './ProjectDelegation.css'

/** components D10 托管开启说明（managed#enable）。2026-10-11 UI alignment:
 * turning delegation on always passes this one explanation, centred, Cancel
 * focused, no default confirm key, background decisions blocked.
 * ui-redesign 票 08：从原生 <dialog> 换到共享 AlertDialog（Radix 负责焦点困住与
 * 首焦「取消」，背景裁决键由 AlertDialogContent 拦）；`confirmManaged` 键位默认空，
 * 只有负责人自己绑了才生效，这里吃掉它免得漏到背景。文案按 ADR 0087（R-01）：
 * 本机删除只进废纸篓、外部删除宿主拦不住、Linux 限制。 */
export function ManagedConfirmation({ pending, error, onCancel, onConfirm }: {
  pending: boolean; error: boolean; onCancel: () => void; onConfirm: () => void
}) {
  const { t } = useTranslation()
  const project = useUiStore(s => s.projectName)
  // 票 16B（managed#enable「仍受项目预算约束：本月剩余 ¥61.4」）：设了项目用量上限就写剩余额度
  // （上限 − 项目累计花费，与胶囊弹层预算行同源 managed_summary）；未设上限或尚未读到时保持原句。
  const { summary } = useDelegation()
  const budgetLeftMc = summary?.limit_cents != null ? Math.max(0, summary.limit_cents * 1000 - summary.spent_mc) : null
  const current = useRef({ pending, onCancel, onConfirm })
  useLayoutEffect(() => { current.current = { pending, onCancel, onConfirm } }, [pending, onCancel, onConfirm])
  useEffect(() => {
    const block = (event: KeyboardEvent) => {
      if (matches(event, bindingFor('confirmManaged'))) {
        event.preventDefault(); event.stopImmediatePropagation()
        if (!current.current.pending) current.current.onConfirm()
        return
      }
      blockBackgroundDecision(event)
    }
    window.addEventListener('keydown', block, true)
    return () => window.removeEventListener('keydown', block, true)
  }, [])
  const can = t('delegation.can', { returnObjects: true }) as string[]
  const cannot = t('delegation.cannot', { returnObjects: true }) as string[]
  const confirmKey = formatBinding(bindingFor('confirmManaged'))
  const info: { icon: Parameters<typeof Icon>[0]['name']; text: string; attr?: string }[] = [
    { icon: 'refresh', text: t('delegation.infoRounds') },
    { icon: 'usage', text: budgetLeftMc != null ? t('delegation.infoBudgetLeft', { left: fmtYuan(budgetLeftMc) }) : t('delegation.infoBudget'), attr: 'data-managed-budget-info' },
    { icon: 'flag', text: t('delegation.infoHonest') },
    // ADR0087: the host cannot block deletions inside external apps; say so before turning on.
    { icon: 'warn', text: t('delegation.externalDeletion'), attr: 'data-external-deletion' },
    { icon: 'terminal', text: t('delegation.linuxLimit'), attr: 'data-linux-limit' },
  ]
  return <AlertDialog open onOpenChange={(open) => { if (!open && !current.current.pending) current.current.onCancel() }}>
    <AlertDialogContent busy={pending} className="managed-confirm w-[min(560px,calc(100vw-48px))] gap-0 px-[22px] pt-[22px] pb-4"
      onCloseAutoFocus={(event) => event.preventDefault()}>
      <div className="flex items-start gap-3">
        <AlertDialogIcon tone="info" className="size-9 rounded-[var(--r-card)]"><Icon name="bot" size={14} /></AlertDialogIcon>
        <div className="min-w-0">
          <AlertDialogTitle>{project ? t('delegation.confirmTitleProject', { name: project }) : t('delegation.confirmTitle')}</AlertDialogTitle>
          <AlertDialogDescription className="mt-1">{t('delegation.confirmBody')}</AlertDialogDescription>
        </div>
      </div>
      <div className="mt-4 grid grid-cols-2 gap-2.5">
        <section className="rounded-[var(--r-card)] bg-inset p-3">
          <h3 className="mb-2 flex items-center gap-1.5 text-[12px] font-medium text-ink"><Icon name="check" size={12} className="text-green" />{t('delegation.canTitle')}</h3>
          <ul className="m-0 list-none p-0 text-[12px] leading-[1.7] text-ink-2">{can.map(line => <li key={line}>{line}</li>)}</ul>
        </section>
        <section className="rounded-[var(--r-card)] bg-inset p-3">
          <h3 className="mb-2 flex items-center gap-1.5 text-[12px] font-medium text-ink"><Icon name="close" size={12} className="text-red" />{t('delegation.cannotTitle')}</h3>
          <ul className="m-0 list-none p-0 text-[12px] leading-[1.7] text-ink-2">{cannot.map(line => <li key={line}>{line}</li>)}</ul>
        </section>
      </div>
      <ul className="mt-3 flex list-none flex-col gap-1.5 p-0 text-[12px] text-ink-2">
        {info.map(row => <li key={row.icon} className="flex items-start gap-2" {...(row.attr ? { [row.attr]: '' } : {})}>
          <Icon name={row.icon} size={12} className="mt-[3px] flex-none text-ink-3" /><span>{row.text}</span>
        </li>)}
      </ul>
      {error && <AlertDialogError className="mt-3">{t('delegation.failed')}</AlertDialogError>}
      <AlertDialogFooter className="mt-5">
        <AlertDialogCancel size="sm" onClick={(event) => { event.preventDefault(); onCancel() }}
          title={[t('agent.cancel'), 'Esc'].join(' · ')}>{t('agent.cancel')}</AlertDialogCancel>
        <AlertDialogAction size="sm" loading={pending} onClick={onConfirm}
          title={[t('delegation.confirmAction'), confirmKey].filter(Boolean).join(' · ')}>
          {!pending && <Icon name="bot" size={12} />}{t('delegation.confirmAction')}
        </AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
}
