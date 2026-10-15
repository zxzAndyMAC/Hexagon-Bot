// 设置整页（票 29）：工作台整体换成设置页，不是弹层。
// ui-redesign 票 11（components E1 SettingsShell，settings#general…#logs）：左 set-nav 220（--app-side 底、右线 --line）：
// 拖拽栏 44 →「← 返回工作台」ghost sm → B1 同款导航（通用 / 键盘 / 模型服务 / 团队 / 权限 / 自治 / 提示词 / 技能 /
// MCP 服务 / 用量 / 日志 / 关于；「自治」原型缺失，R-07 照现实现保留）。右侧两种内容：平铺表单 720 居中；
// 带清单的分区（模型、团队、技能、MCP、提示词）用「列表 | 详情」铺满（E2）。
// 「日志」（diagnostic-records 票 01）：诊断开关在这里，通用页不再放。
// projectless（启动页「设置」入口，无项目上下文）：项目作用域分区渲染 A19「需要先打开项目」而
// 不发必败的 conn/wb IPC——否则每个分区都弹 internal toast（ui-audit-2 收口回归）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { bindingFor, isMac, matches } from '../keymap'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Icon, type IconName } from './Icon'
import { DesktopPauseButton } from './DesktopPauseButton'
import { ProviderManager } from './ProviderManager'
import { PromptsSection } from './PromptsSection'
import { UsageTab } from './UsageTab'
import { AutonomySection } from './settings/AutonomySection'
import { GeneralSection } from './settings/GeneralSection'
import { KeysSection } from './settings/KeysSection'
import { LogsSection } from './settings/LogsSection'
import { McpSection } from './settings/McpSection'
import { PermsSection } from './settings/PermsSection'
import { SkillsSection } from './settings/SkillsSection'
import { TeamSection } from './settings/TeamSection'
import { Group, NeedsProject, SetHead, SetWrap, SRow } from './settings/kit'

export type Section = 'general' | 'keys' | 'team' | 'models' | 'perms' | 'skills' | 'mcp' | 'autonomy' | 'usage' | 'logs' | 'prompts' | 'about'

/** 导航顺序与图标（proto.js SET 表；autonomy 补在权限后，R-07）。 */
const NAV: { id: Section; icon: IconName }[] = [
  { id: 'general', icon: 'settings' },
  { id: 'keys', icon: 'keyboard' },
  { id: 'models', icon: 'cpu' },
  { id: 'team', icon: 'users' },
  { id: 'perms', icon: 'shield' },
  { id: 'autonomy', icon: 'gauge' },
  { id: 'prompts', icon: 'messages' },
  { id: 'skills', icon: 'book' },
  { id: 'mcp', icon: 'plug' },
  { id: 'usage', icon: 'chart-column' },
  { id: 'logs', icon: 'logs' },
  { id: 'about', icon: 'info' },
]

/** 列表 | 详情分区铺满右侧（E2），其余平铺表单在右侧滚动。日志自己管滚动（只有表体滚）。 */
const FILL = new Set<Section>(['team', 'models', 'skills', 'mcp', 'prompts', 'logs'])

/** 用量分区（ui-audit-2 票 05）：复用 UsageTab，「详情」在设置上下文里回工作台开用量明细。票 12 换皮。 */
function UsageSection({ onDetail }: { onDetail?: () => void }) {
  const { t } = useTranslation()
  const refreshSlow = useUiStore((s) => s.refreshSlow)
  useEffect(() => { void refreshSlow(['usage', 'team']) }, [refreshSlow])
  return <SetWrap><SetHead title={t('settings.nav_usage')} /><UsageTab onDetail={onDetail} /></SetWrap>
}

function AboutSection() {
  const { t } = useTranslation()
  return (
    <SetWrap>
      <SetHead title={t('settings.nav_about')} desc={t('settings.aboutLine')} />
      <Group>
        <SRow icon="hex" label="Hexagon-Bot" hint={t('settings.aboutLine')}>
          <span className="font-mono text-[12px] text-ink-3">v0.1.0</span>
        </SRow>
      </Group>
    </SetWrap>
  )
}

function SettingsNav({ section, onSection, onBack, projectless }: {
  section: Section
  onSection: (s: Section) => void
  onBack: () => void
  projectless: boolean
}) {
  const { t } = useTranslation()
  return (
    <aside data-set-nav className="flex w-[220px] flex-none flex-col border-r border-line bg-sidebar">
      <div data-tauri-drag-region className={cn('h-11 flex-none', isMac && 'pl-[84px]')} />
      <Button size="sm" variant="ghost" className="mx-2.5 mb-2.5 justify-start" onClick={onBack}>
        <Icon name="arrow-left" size={14} />{t('settings.back')}
      </Button>
      <nav aria-label={t('settings.title')} className="flex flex-col gap-px overflow-y-auto px-2.5 pb-3"
        onKeyDown={(e) => {
          if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
          const items = [...e.currentTarget.querySelectorAll<HTMLElement>('button')]
          const i = items.indexOf(e.target as HTMLElement)
          if (i < 0) return
          e.preventDefault()
          items[Math.min(items.length - 1, Math.max(0, i + (e.key === 'ArrowDown' ? 1 : -1)))]?.focus()
        }}>
        {NAV.map(({ id, icon }) => (
          <button
            key={id}
            type="button"
            data-nav={id}
            aria-current={section === id ? 'page' : undefined}
            onClick={() => onSection(id)}
            className={cn(
              'hx-press hx-focus flex h-8 w-full cursor-pointer items-center gap-[9px] rounded-[var(--r-sm)] px-2 text-left text-[13px] text-ink-2 live:hover:bg-hover live:hover:text-ink',
              section === id && 'bg-hover-2 font-medium text-ink',
            )}
          >
            <Icon name={icon} size={16} />
            <span className="min-w-0 flex-1 truncate">{t(`settings.nav_${id}`)}</span>
          </button>
        ))}
      </nav>
      {/* 「暂停电脑操作」在设置的任何分区都能点（票 14 验收：暂停按钮在任何页面都能点）——放在导航底部，
          权限分区不再重复放（原型 settings#perms 放在权限标题行，对账表票 11 偏差）。 */}
      {!projectless && (
        <div data-set-nav-foot className="mt-auto flex flex-none border-t border-line p-2.5">
          <DesktopPauseButton />
        </div>
      )}
    </aside>
  )
}

export function SettingsPage({ onBack, onOpenUsageDetail, projectless = false, initialSection = 'general' }: {
  onBack: () => void
  initialSection?: Section
  onOpenUsageDetail?: () => void
  /// 无项目上下文（启动页进入）：项目作用域分区只显示提示，不触发 IPC
  projectless?: boolean
}) {
  const [section, setSection] = useState<Section>(initialSection)
  useEffect(() => { setSection(initialSection) }, [initialSection])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (matches(event, bindingFor('desktopPanel')) || matches(event, bindingFor('browserPanel'))) setSection('perms')
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])

  /// 项目作用域分区在无项目上下文下的占位（见头部注释）：A19 动作回启动页打开项目
  const gated = (el: React.ReactNode): React.ReactNode =>
    projectless ? <SetWrap><NeedsProject onOpen={onBack} /></SetWrap> : el

  const body = (() => {
    switch (section) {
      case 'general': return <GeneralSection onGo={setSection} />
      case 'keys': return <KeysSection />
      case 'team': return <TeamSection projectless={projectless} />
      case 'models': return <ProviderManager /> // 全局 providers.json，无项目也可用
      case 'perms': return <PermsSection projectless={projectless} onOpenProject={onBack} />
      case 'skills': return <SkillsSection /> // 全局层无项目也可用（ADR 0057）
      case 'mcp': return <McpSection projectless={projectless} /> // 全局清单无项目可配（ADR 0057）
      case 'autonomy': return gated(<AutonomySection />)
      case 'usage': return gated(<UsageSection onDetail={onOpenUsageDetail} />)
      // 日志页不被 gated 吃掉——无项目时宿主记录仍要看（diagnostic-records 票 01）。
      case 'logs': return <LogsSection projectless={projectless} />
      // 提示词页是宿主级（翻译槽在全局 providers.json），无项目也可用（ADR 0071）。
      case 'prompts': return <PromptsSection onGoModels={() => setSection('models')} />
      case 'about': return <AboutSection />
    }
  })()

  return (
    <TooltipProvider>
      <div data-settings className="settings-scope flex h-full min-h-0 bg-[var(--app-main)] text-ink">
        <SettingsNav section={section} onSection={setSection} onBack={onBack} projectless={projectless} />
        <main data-set-body={section} className={cn('flex min-h-0 min-w-0 flex-1 flex-col', FILL.has(section) ? 'overflow-hidden' : 'overflow-y-auto')}>
          {body}
        </main>
      </div>
    </TooltipProvider>
  )
}
