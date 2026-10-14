// 头部 Header（components B5；workbench#running、index#ia；ADR 0088）。48 高，整条是窗口拖拽区。
// 左：crumb（当前阶段名 13/600 + warn 胶囊「阶段 n / N」）+ 托管胶囊（D7，票 02 原样迁入，08 换皮）；
// 右：「屏幕」ghost xs（有 Agent Screen 目标才渲染）· 画布开关 ibtn（⌥⌘B，开时 on；票 02 开关画布占位，03 接真画布）
// · 更多 ibtn（⋯：全员休眠、导出轨迹、查看流程）。
// ⋯ 菜单另收两件旧顶栏面（补设计，B5）：值守切换、沙箱状态（只读脚注）——旧顶栏撤掉后它们没有别的落点。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { api, errText } from '../../api'
import type { SandboxStatus } from '../../gen/SandboxStatus'
import { isMac } from '../../keymap'
import { runStageOp } from '../../stageops'
import { exportTrace } from '../../traceExport'
import { Icon } from '../Icon'
import { ManagedPill } from '../ManagedPill'
// 票 14（D19）：头部控制状态 + 暂停电脑操作；Agent Screen 浮窗随头部挂载（输入条旁的重复入口撤掉）
import { ComputerControlStatus } from '../ComputerControlStatus'
import { AgentScreen } from '../AgentScreen'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { ShortcutKbd } from '@/components/ui/kbd'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuFooter, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { bindingFor } from '../../keymap'
import { cn } from '@/lib/utils'

const CURRENT = new Set(['active', 'waiting_stamp', 'interrupted', 'rejected'])

/** crumb：当前阶段与「阶段 n / N」；阶段未知时显示项目名、无胶囊（B5 加载行）。 */
function useCrumb() {
  const stages = useUiStore((s) => s.stages)
  const projectName = useUiStore((s) => s.projectName)
  const sorted = [...stages].sort((a, b) => a.seq - b.seq)
  const i = sorted.findIndex((s) => CURRENT.has(s.state))
  if (i < 0) return { title: projectName, step: null }
  return { title: sorted[i].stage, step: { n: i + 1, total: sorted.length } }
}

function MoreMenu() {
  const { t } = useTranslation()
  const away = useUiStore((s) => s.away)
  const projectName = useUiStore((s) => s.projectName)
  const [sandbox, setSandbox] = useState<SandboxStatus | null>(null)
  // 票 06：沙箱实况——纯平台探测，同项目内不变
  useEffect(() => { api.sandboxStatus().then(setSandbox).catch(() => setSandbox(null)) }, [projectName])
  const st = useUiStore.getState
  const doExport = async () => {
    try {
      const count = await exportTrace()
      if (count != null) st().pushToast(t('timeline.exported', { count }), 'ok')
    } catch (e) {
      st().pushToast(errText(e), 'err')
    }
  }
  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <button type="button" aria-label={t('header.more')} data-header-more
              className="hx-press hx-focus inline-grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm)] text-ink-2 live:hover:bg-hover live:hover:text-ink">
              <Icon name="more" size={16} />
            </button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>{t('header.more')}</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" className="w-[260px]">
        {/* 全员休眠仍要二次确认：runStageOp 走现有确认层（票 13 换成 AlertDialog） */}
        <DropdownMenuItem icon={<Icon name="sleep" />} onSelect={() => runStageOp('sleepAll')}>{t('header.sleepAll')}</DropdownMenuItem>
        <DropdownMenuItem icon={<Icon name="export" />} onSelect={() => void doExport()}>{t('header.exportTrace')}</DropdownMenuItem>
        <DropdownMenuItem icon={<Icon name="expand" />} onSelect={() => st().setFlowDialogOpen(true)}>{t('sidebar.viewFlow')}</DropdownMenuItem>
        <DropdownMenuSeparator />
        {/* 票 16（方向卡 1）：值守切换。失焦 60 秒自动值守，回焦自动归来；这里是手动入口 */}
        <DropdownMenuItem icon={<Icon name="sleep" />} description={t('topbar.awayHint')} tall
          data-away={away || undefined}
          onSelect={() => void (away ? st().markBack() : st().markAway())}>
          {t(away ? 'topbar.awayActive' : 'topbar.away')}
        </DropdownMenuItem>
        {sandbox && (
          <DropdownMenuFooter data-sandbox={sandbox.available ? 'on' : 'off'}
            title={`${t(sandbox.available ? 'topbar.sandboxOnTip' : 'topbar.sandboxOffTip')} · ${sandbox.note}`}>
            <span className="inline-flex items-center gap-1.5">
              <Icon name="shield" size={12} />
              {t(sandbox.available ? 'topbar.sandboxOn' : 'topbar.sandboxOff')} · {t(sandbox.available ? 'topbar.sandboxOnTip' : 'topbar.sandboxOffTip')}
            </span>
          </DropdownMenuFooter>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * 侧栏开关（components B5 左端；2026-10-14 负责人裁决，修原型 bug）：原在侧栏顶栏，收起为图标列或 ⌘. 专注后
 * 随侧栏消失，鼠标无法恢复。现常驻头部左端、与右端画布开关镜像。专注模式（hidden）下点击走 store.toggleSidebar，
 * 它按 interaction §1 等同 ⌘.：退出专注并恢复原布局，不在看不见的侧栏上翻收起状态。
 */
function SidebarToggle({ side }: { side: 'expanded' | 'collapsed' | 'hidden' }) {
  const { t } = useTranslation()
  const open = side === 'expanded'
  const label = side === 'hidden' ? t('header.exitFocus') : t(open ? 'sidebar.collapse' : 'sidebar.expand')
  return (
    <IconButton label={label} action={side === 'hidden' ? 'focusMode' : 'toggleSidebar'}
      aria-expanded={open} data-state={open ? 'open' : 'closed'} data-header-sidebar
      onClick={() => useUiStore.getState().toggleSidebar()}>
      <Icon name={open ? 'panel-left-open' : 'panel-left'} size={16} />
    </IconButton>
  )
}

/** side（票 03）：侧栏实际形态（含被窗口逼成图标列、专注模式隐藏），决定 macOS 红绿灯让位。 */
export function Header({ side }: { side?: 'expanded' | 'collapsed' | 'hidden' } = {}) {
  const { t } = useTranslation()
  const crumb = useCrumb()
  // 票 03：头部开关反映画布是否可见；与 ⌥⌘B 同一动作（store.toggleCanvas）
  const canvasOpen = useUiStore((s) => s.canvasOpen && !s.usageView)
  const screen = useUiStore((s) => s.agentScreenAvailable)
  const storedCollapsed = useUiStore((s) => s.sideLayout.collapsed)
  const sideCollapsed = side ? side !== 'expanded' : storedCollapsed
  const st = useUiStore.getState
  return (
    <header
      data-tauri-drag-region
      data-header
      className={cn(
        'flex h-12 flex-none items-center gap-2 border-b border-line-soft pr-3 pl-4',
        // macOS overlay 红绿灯约占左上 0–70px：侧栏收成 56 宽图标列时头部左侧让出余量
        isMac && sideCollapsed && (side === 'hidden' ? 'pl-20' : 'pl-7'),
      )}
    >
      <SidebarToggle side={side ?? (storedCollapsed ? 'collapsed' : 'expanded')} />
      <div data-tauri-drag-region className="flex min-w-0 items-center gap-1.5 text-[13px] text-ink-2">
        <b className="truncate font-semibold text-ink">{crumb.title || t('app.untitledProject')}</b>
        {crumb.step && (
          <Badge variant="warn" data-stage-chip>
            <Icon name="clock" size={12} />
            {t('header.stageChip', crumb.step)}
          </Badge>
        )}
      </div>
      <ComputerControlStatus />
      <ManagedPill />
      <span data-tauri-drag-region className="flex-1 self-stretch" />
      {screen && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="xs" data-header-screen onClick={() => st().requestAgentScreen()}>
              <Icon name="computer" size={14} />{t('header.screen')}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('agentScreen.title')}<ShortcutKbd binding={bindingFor('agentScreen')} /></TooltipContent>
        </Tooltip>
      )}
      {/* 2026-10-14 负责人裁决（B5）：与左端侧栏开关镜像；状态由图标（右窗格实心 / 只剩分隔线）+ aria-expanded 表达 */}
      <IconButton label={t(canvasOpen ? 'header.closeCanvas' : 'header.openCanvas')} action="toggleCanvas"
        aria-expanded={canvasOpen} data-state={canvasOpen ? 'open' : 'closed'}
        data-header-canvas onClick={() => st().toggleCanvas()}>
        <Icon name={canvasOpen ? 'panel-right-open' : 'panel-right'} size={16} />
      </IconButton>
      <MoreMenu />
      <AgentScreen />
    </header>
  )
}
