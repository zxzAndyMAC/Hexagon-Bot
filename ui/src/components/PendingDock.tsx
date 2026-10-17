// 待决坞（ui-redesign 票 04，ADR 0089，components D6，interaction §2，decisions#dock / #collapsed）。
// 输入条正上方、最宽 720，不遮挡时间线、不抢焦点；按 SEVERITY 排序分页「‹ 1 / N ›」，顶卡（第一张）吃 ⌘↵ / ⌘⌫
// （键由 App 的 usePendingKeys → handlePendingKey 处理，键位写在顶卡按钮上）。
// ⌘Esc（dismissPending）收起成「N 件待决 · 最严重：…」胶囊——收起 ≠ 解决，侧栏徽标数不变；新卡到达自动展开
// （store.syncPendingDialog）。托管事项由宿主的 owner_pending 读排除，不进坞、不计数、不吃键。
// in_review 提案排在全部待决卡之后的几页，不吃裁决键。
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { bindingFor, matches } from '../keymap'
import { kindTitleKey, visiblePending } from '../decisions'
import { Icon } from './Icon'
import { IconButton } from './ui/icon-button'
import { Button } from './ui/button'
import { Dot } from './ui/dot'
import { DockChrome } from './DecisionShell'
import { LOCATE_PENDING_EVENT } from '../managedNext'
import { PendingCard } from './PendingCards'
import { ReviewCard } from './PendingCardParts'
import './decision.css'

type Page = { key: string; node: React.ReactNode }

export function PendingDock() {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.pending)
  const hidden = useUiStore((s) => s.hiddenPending)
  const reviewRows = useUiStore((s) => s.reviewRows)
  const open = useUiStore((s) => s.pendingDialogOpen)
  const error = useUiStore((s) => s.pendingError)
  const [currentKey, setCurrentKey] = useState<string | null>(null)

  // in_review 提案进坞的数据源（原 PendingDialog）：随 pending 变化重读。
  useEffect(() => {
    let cancel = false
    api.proposals()
      .then((all) => { if (!cancel) useUiStore.getState().noteReviewRows(all.filter((r) => r.status === 'in_review')) })
      .catch((e) => {
        if (cancel) return
        useUiStore.getState().noteReviewRows([])
        useUiStore.getState().pushToast(errText(e), 'err')
      })
    return () => { cancel = true }
  }, [pending])

  useEffect(() => { useUiStore.getState().syncPendingDialog() }, [pending, reviewRows])

  // 裁决失败：卡回到坞顶（D6 错误态）——翻回这张，坞展开。
  useEffect(() => {
    if (!error) return
    setCurrentKey(`q:${error.id}`)
    useUiStore.getState().openPendingDialog()
  }, [error])

  // review R2：托管报告「定位到卡」——翻到这张卡并展开坞（卡不在坞里时调用方已改走时间线）。
  useEffect(() => {
    const locate = (event: Event) => {
      const id = (event as CustomEvent<unknown>).detail
      if (typeof id !== 'string') return
      setCurrentKey(`q:${id}`)
      useUiStore.getState().openPendingDialog()
    }
    window.addEventListener(LOCATE_PENDING_EVENT, locate)
    return () => window.removeEventListener(LOCATE_PENDING_EVENT, locate)
  }, [])

  useEffect(() => {
    if (!open) return
    const h = (e: KeyboardEvent) => {
      if (!matches(e, bindingFor('dismissPending'))) return
      // Ticket 07: a native permissions dialog is above this dock.
      // Its close key must not collapse the dock as well.
      if (document.querySelector('dialog[open]')) return
      const st = useUiStore.getState()
      if (st.modalScope !== 'workbench' || st.confirmReq) return
      e.preventDefault()
      e.stopPropagation()
      st.closePendingDialog()
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [open])

  const sorted = useMemo(() => visiblePending(pending, hidden), [pending, hidden])
  const pages: Page[] = useMemo(() => [
    ...sorted.map((q, i) => ({ key: `q:${q.id}`, node: <PendingCard key={q.id} q={q} top={i === 0} /> })),
    ...reviewRows.map((r) => ({ key: `r:${r.id}`, node: <ReviewCard key={r.id} row={r} /> })),
  ], [sorted, reviewRows])

  if (pages.length === 0) return null
  const total = pending.length + reviewRows.length
  const found = pages.findIndex((p) => p.key === currentKey)
  // 当前页的卡已解决（或从未翻过页）→ 回到顶卡
  const index = found < 0 ? 0 : found
  const go = (i: number) => setCurrentKey(pages[(i + pages.length) % pages.length].key)
  const worst = sorted[0] ? t(kindTitleKey(sorted[0])) : t('cards.proposalReview')

  if (!open) {
    return (
      <div className="dock-pill-row">
        <Button size="sm" className="dock-pill" data-dock="collapsed" aria-expanded={false}
          onClick={() => useUiStore.getState().openPendingDialog()}>
          <Dot tone="warn" pulse />
          {t('dock.collapsed', { count: total, kind: worst })}
          <Icon name="chevron-up" size={12} />
        </Button>
      </div>
    )
  }

  const chrome = (
    <>
      {pages.length > 1 && (
        <div className="dock-pager" role="group" aria-label={t('dock.pager', { n: index + 1, total: pages.length })}
          onKeyDown={(e) => {
            if (e.key === 'ArrowLeft') { e.preventDefault(); go(index - 1) }
            if (e.key === 'ArrowRight') { e.preventDefault(); go(index + 1) }
          }}>
          <IconButton size="sm" label={t('dock.prev')} onClick={() => go(index - 1)}><Icon name="chevron-left" size={12} /></IconButton>
          <span aria-live="polite">{index + 1} / {pages.length}</span>
          <IconButton size="sm" label={t('dock.next')} onClick={() => go(index + 1)}><Icon name="chevron-right" size={12} /></IconButton>
        </div>
      )}
      <IconButton size="sm" label={t('dock.collapse')} action="dismissPending" onClick={() => useUiStore.getState().closePendingDialog()}>
        <Icon name="chevron-down" size={14} />
      </IconButton>
    </>
  )
  return (
    <section className="dock" role="region" aria-label={t('dock.label', { n: index + 1, total: pages.length })}
      data-dock="open" data-pending-top={sorted[0]?.id}>
      <DockChrome.Provider value={chrome}>
        {pages[index].node}
      </DockChrome.Provider>
    </section>
  )
}
