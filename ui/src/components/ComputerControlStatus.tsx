// 头部控制状态（components D19「头部控制状态」+ B5 可插入位，control#screen；票 ui-redesign 14）。
// chip（脉冲点）写清谁在控制什么：「QA 正在控制浏览器」；点开是电脑操作与浏览器面板弹层（control#panels）。
// 旁边「暂停电脑操作」danger xs，kbd 渲染 desktopPause 的当前绑定（R-05：照 keymap，原型 ⌃⌥⌘. 只是示意）。
// 显隐由 computerControl.controlSummary 判定：电脑操作停用且没有浏览器连接时整块不渲染。
// 暂停走 desktopPause.pauseDesktop（与全局键同一路径，绑定宿主当前项目；切项目中 fail-closed）。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor } from '../keymap'
import { useUiStore } from '../store'
import { pauseDesktop } from '../desktopPause'
import { canPauseComputer, controlSummary } from '../computerControl'
import { useDesktopStatus } from '../useDesktopStatus'
import { BrowserControlPanel } from './BrowserControlPanel'
import { DesktopControlPanel } from './DesktopControlPanel'
import { Icon } from './Icon'
import { badgeVariants } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'

export function ComputerControlStatus() {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  // 功能缺口 C4：与面板、设置暂停按钮共用一份宿主读（useDesktopStatus），不再自开 2 秒定时器。
  const { status, browser } = useDesktopStatus()
  const [open, setOpen] = useState(false)
  const [pausing, setPausing] = useState(false)
  const summary = controlSummary(status, Boolean(browser?.connected), (id) => team.find((m) => m.id === id)?.role ?? id)
  if (summary.kind === 'hidden' && !open) return null
  const label = summary.kind === 'active' ? t(summary.target === 'browser' ? 'control.activeBrowser' : 'control.activeComputer', { agent: summary.agent })
    : summary.kind === 'elsewhere' ? t('control.elsewhere')
    : summary.kind === 'paused' ? t('control.paused')
    : summary.kind === 'checkpoint' ? t('control.checkpoint')
    : summary.kind === 'idle' ? t(summary.target === 'browser' ? 'control.idleBrowser' : 'control.idleComputer')
    : t('control.panel')
  const tone = summary.kind === 'active' || summary.kind === 'elsewhere' || summary.kind === 'checkpoint' ? 'warn' : summary.kind === 'idle' ? 'ok' : 'neutral'
  const pause = async () => {
    if (pausing) return
    setPausing(true)
    try { await pauseDesktop() } finally { setPausing(false) }
  }
  return (
    <div className="flex items-center gap-1.5" data-control-status={summary.kind}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button type="button" aria-haspopup="dialog" data-control-chip
            title={t('control.panel')}
            className={cn(badgeVariants({ variant: tone }), 'hx-press hx-focus max-w-[260px] cursor-pointer')}>
            {summary.kind === 'checkpoint' ? <Icon name="pause" size={12} /> : tone === 'warn' ? <Dot tone="warn" pulse /> : tone === 'ok' ? <Dot tone="on" /> : <Icon name="pause" size={12} />}
            <span className="truncate">{label}</span>
          </button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-[340px] max-h-[calc(100vh-80px)] overflow-y-auto p-0" aria-label={t('control.panel')}>
          <DesktopControlPanel />
          <div className="mx-3 border-t border-line-soft" />
          <BrowserControlPanel />
        </PopoverContent>
      </Popover>
      {canPauseComputer(status) && (
        <Button variant="danger" size="xs" data-header-pause loading={pausing} onClick={() => void pause()}>
          <Icon name="pause" size={12} />{t('control.pause')}<ShortcutKbd binding={bindingFor('desktopPause')} />
        </Button>
      )}
    </div>
  )
}
