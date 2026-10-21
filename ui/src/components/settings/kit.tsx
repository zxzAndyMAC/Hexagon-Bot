// 设置页构件（ui-redesign 票 11；components E1 SettingsShell、E2 ListDetail、A19 空态、A20 骨架、A21 表格）。
// 原型 settings.html：平铺表单 set-wrap 720 居中（标题 20/600 + 说明 13 ink-2）→ 分组标题 group-t →
// 分组卡 group（圆角 12，--app-card + --shadow-card）→ 行 srow（padding 13 16，最小 56 高，行间 --line-soft）。
// 带清单的分区用「列表 | 详情」：列表 280（220–360，B8 分割线可拖，宽度存 hexagon.layout.settingsList）。
import * as React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { bindingFor, formatBinding, matches, type ActionId } from '../../keymap'
import { Icon, type IconName } from '../Icon'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Dot } from '@/components/ui/dot'
import { Empty } from '@/components/ui/empty'
import { InlineError as SharedInlineError } from '@/components/ui/inline-error'
import { Skeleton } from '@/components/ui/skeleton'
import { ResizeHandle } from '@/components/ui/resize-handle'
import { readSettingsListWidth, SETTINGS_LIST, writeSettingsListWidth } from '../../settingsLayout'
import { SettingsVirtualList } from '../SettingsVirtualList'
import { ldItemCls } from './styles'

/** 平铺表单外壳：720 居中（权限表格页 860，settings#perms）。 */
export function SetWrap({ wide = false, className, children }: { wide?: boolean; className?: string; children: React.ReactNode }) {
  return (
    <div data-set-wrap className={cn('mx-auto w-full px-8 py-9', wide ? 'max-w-[860px]' : 'max-w-[720px]', className)}>
      {children}
    </div>
  )
}

/** 分区标题 20/600 -.01em + 说明 13 ink-2（下距 24）；右侧可放动作（权限「暂停电脑操作」、键盘「搜索动作」）。 */
export function SetHead({ title, desc, actions }: { title: string; desc?: React.ReactNode; actions?: React.ReactNode }) {
  return (
    <div className="mb-6 flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <h2 className="mb-1 text-[20px] font-semibold tracking-[-.01em] text-ink">{title}</h2>
        {desc && <div className="text-[13px] text-ink-2">{desc}</div>}
      </div>
      {actions && <div className="flex flex-none items-center gap-2">{actions}</div>}
    </div>
  )
}

export function GroupTitle({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn('mb-2 ml-1 text-[12.5px] font-medium text-ink-2', className)}>{children}</div>
}

/** 分组卡：圆角 12（foundations §4 设置分组常量圆角）、--app-card、--shadow-card。 */
export function Group({ className, children, ...props }: React.ComponentProps<'div'>) {
  return (
    <div data-group className={cn('mb-5 rounded-[12px] bg-[var(--app-card)] shadow-[var(--shadow-card)]', className)} {...props}>
      {children}
    </div>
  )
}

/** 行 srow：左（名 13.5/500 + 说明 12.5 ink-3）+ 右控件；可带前置图标；body 放在行下方（承载内嵌面板）。 */
export function SRow({ icon, label, hint, children, body, className }: {
  icon?: IconName
  label: React.ReactNode
  hint?: React.ReactNode
  children?: React.ReactNode
  body?: React.ReactNode
  className?: string
}) {
  return (
    <div data-srow className={cn('border-t border-line-soft px-4 py-[13px] first:border-t-0', className)}>
      <div className="flex min-h-[30px] items-center gap-3.5">
        {icon && <Icon name={icon} size={16} className="flex-none text-ink-2" />}
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-medium text-ink">{label}</div>
          {hint && <div className="text-[12.5px] text-ink-3">{hint}</div>}
        </div>
        {children}
      </div>
      {body && <div className="mt-3">{body}</div>}
    </div>
  )
}

/** A19 空态：共享件在 ui/empty（票 13），设置页沿用此出口。 */
export { Empty }

/** E1：无项目时项目态分区显示 A19「需要先打开项目」，动作回启动页打开项目（A19 禁用行）。 */
export function NeedsProject({ onOpen }: { onOpen?: () => void }) {
  const { t } = useTranslation()
  return (
    <Empty
      icon="lock"
      title={t('settingsUi.needsProjectTitle')}
      desc={t('settings.needsProject')}
      action={onOpen && <Button size="sm" onClick={onOpen}>{t('settingsUi.openProject')}</Button>}
    />
  )
}

/** A20 骨架条（减少动效时静止为 --canvas，见 index.css 的 hx 规则）。 */
/** A20 骨架（共享件 ui/skeleton，票 13）：列表 / 表体加载中的 30 高条块。 */
export function SkeletonRows({ rows = 5, label, className }: { rows?: number; label?: string; className?: string }) {
  return (
    <div role="status" aria-busy="true" className={cn('flex flex-col gap-2 p-3', className)}>
      {label && <span className="sr-only">{label}</span>}
      {Array.from({ length: rows }, (_, i) => <Skeleton key={i} className="h-[30px]" />)}
    </div>
  )
}

export type RetrySpec = { text: string; onRetry: () => void; retryLabel?: string; retryAction?: ActionId }

/** C5 行内错误（列表 / 表体加载失败）+ 「重新加载」；给了 keymap 动作时按钮 title 带当前绑定、聚焦时按键即重试。 */
export function InlineError({ text, onRetry, retryLabel, retryAction }: Omit<RetrySpec, 'onRetry'> & { onRetry?: () => void }) {
  const { t } = useTranslation()
  const label = retryLabel ?? t('settingsUi.reload')
  // C5 块形（共享件 ui/inline-error，票 13）；重试钮保留 keymap 绑定提示与聚焦按键即重试。
  return (
    <SharedInlineError className="m-3" title={text} actions={onRetry && (
      <Button size="xs" onClick={onRetry}
        title={retryAction ? [label, formatBinding(bindingFor(retryAction))].filter(Boolean).join(' · ') : undefined}
        onKeyDown={retryAction ? (e) => {
          if (matches(e.nativeEvent, bindingFor(retryAction))) { e.preventDefault(); e.stopPropagation(); onRetry() }
        } : undefined}>
        {label}
      </Button>
    )} />
  )
}

type ListDetailProps<T> = {
  /** listbox 可读名（「供应商」「技能」…）。 */
  label: string
  items: T[]
  itemKey: (t: T) => string
  filterText: (t: T) => string
  renderItem: (t: T, selected: boolean) => React.ReactNode
  selected: string | null
  onSelect: (k: string | null) => void
  /** 分组（「已配置 / 内置目录」「编排 / 角色」）：按首次出现的顺序排。 */
  groupOf?: (t: T) => string
  groupLabel?: (g: string) => string
  itemDim?: (t: T) => boolean
  searchPlaceholder?: string
  /** 搜索框上方的头部（团队「本项目 Agent | 模板库」分段）。 */
  head?: React.ReactNode
  /** 底部固定操作行 ld-foot。 */
  foot?: React.ReactNode
  emptyText?: string
  virtualized?: boolean
  loading?: boolean
  /** 加载骨架的读屏文案（「正在加载技能…」）。 */
  loadingLabel?: string
  error?: RetrySpec
  /** 详情区底下额外的整宽块（扫描结果）。 */
  below?: React.ReactNode
  children: React.ReactNode
}

/**
 * E2 列表 | 详情。列表是 listbox：↑↓ / Home / End 在项间移动并选中；>100 条走虚拟列表。
 * 再点已选中项取消选中（回到详情空态，沿用旧行为）。分割线 B8：拖动 220–360、停在下限不吸附、双击还原 280。
 */
export function ListDetail<T>(p: ListDetailProps<T>) {
  const { t } = useTranslation()
  const [q, setQ] = React.useState('')
  const [width, setWidth] = React.useState(() => readSettingsListWidth(localStorage))
  const commit = (w: number) => { setWidth(w); writeSettingsListWidth(localStorage, w) }
  const shown = p.items.filter((it) => !q || p.filterText(it).toLowerCase().includes(q.toLowerCase()))
  const groups: { key: string; items: T[] }[] = []
  for (const it of shown) {
    const g = p.groupOf?.(it) ?? ''
    const bucket = groups.find((x) => x.key === g)
    if (bucket) bucket.items.push(it)
    else groups.push({ key: g, items: [it] })
  }
  const flat = groups.flatMap((g) => g.items)

  const listKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement
    if (target.getAttribute('role') !== 'option') return
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      const k = target.dataset.key ?? null
      p.onSelect(k === p.selected ? null : k)
      return
    }
    const cur = flat.findIndex((it) => p.itemKey(it) === target.dataset.key)
    const next = e.key === 'ArrowDown' ? Math.min(flat.length - 1, cur + 1)
      : e.key === 'ArrowUp' ? Math.max(0, cur - 1)
      : e.key === 'Home' ? 0 : e.key === 'End' ? flat.length - 1 : null
    if (next == null || !flat[next]) return
    e.preventDefault()
    const k = p.itemKey(flat[next])
    p.onSelect(k)
    e.currentTarget.querySelector<HTMLElement>(`[role="option"][data-key="${CSS.escape(k)}"]`)?.focus()
  }

  const option = (it: T) => {
    const k = p.itemKey(it)
    const sel = p.selected === k
    return (
      <div
        key={k}
        role="option"
        data-key={k}
        aria-selected={sel}
        tabIndex={sel || (p.selected == null && flat[0] === it) ? 0 : -1}
        onClick={() => p.onSelect(sel ? null : k)}
        className={ldItemCls(sel, p.itemDim?.(it))}
      >
        {p.renderItem(it, sel)}
      </div>
    )
  }

  return (
    <div data-list-detail className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className="flex min-h-0 flex-1">
        <div data-ld-list style={{ width }} className="flex min-h-0 flex-none flex-col">
          {p.head}
          {p.searchPlaceholder && (
            <Input
              className="mx-3 mt-3 mb-2 w-auto"
              leading={<Icon name="search" size={16} />}
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder={p.searchPlaceholder}
              aria-label={p.searchPlaceholder}
            />
          )}
          <div role="listbox" aria-label={p.label} onKeyDown={listKey} className={cn('min-h-0 flex-1 overflow-y-auto pb-2', !p.searchPlaceholder && !p.head && 'pt-3')}>
            {p.error ? (
              <InlineError {...p.error} />
            ) : p.loading && p.items.length === 0 ? (
              <SkeletonRows label={p.loadingLabel} />
            ) : shown.length === 0 ? (
              <div className="px-[18px] py-3 text-[12.5px] text-ink-3">{p.emptyText ?? t('settingsUi.noMatches')}</div>
            ) : p.virtualized && shown.length > 100 ? (
              <SettingsVirtualList key={q} items={shown} itemKey={p.itemKey} selected={p.selected}
                onSelect={p.onSelect} renderItem={p.renderItem} itemClass={ldItemCls} />
            ) : (
              groups.map((g, gi) => (
                <div key={g.key || gi} role={g.key ? 'group' : undefined} aria-label={g.key ? p.groupLabel?.(g.key) ?? g.key : undefined}>
                  {g.key && (
                    <div aria-hidden className={cn('px-[18px] pb-1 text-[11.5px] text-ink-3', gi === 0 ? 'pt-1' : 'pt-3')}>
                      {p.groupLabel?.(g.key) ?? g.key}
                    </div>
                  )}
                  {g.items.map(option)}
                </div>
              ))
            )}
          </div>
          {p.foot && <div data-ld-foot className="flex flex-none flex-wrap gap-1.5 border-t border-line-soft px-3 py-2.5">{p.foot}</div>}
        </div>
        <ResizeHandle
          label={t('settingsUi.listWidth')}
          value={width}
          limits={SETTINGS_LIST}
          tipTitle={t('settingsUi.listWidth')}
          tipExtra={t('layout.resetHint')}
          onDrag={(r) => setWidth(r.width)}
          onCommit={(r) => commit(r.width)}
          onDoubleClick={() => commit(SETTINGS_LIST.default)}
        />
        <div data-ld-detail className="min-h-0 min-w-0 flex-1 overflow-y-auto px-7 py-6">{p.children}</div>
      </div>
      {p.below}
    </div>
  )
}

/** 详情头：大头像 / 图标 + 名 15/600 + 副行 12 ink-3（可等宽）+ 右侧动作。 */
export function DetailHead({ lead, title, sub, mono = false, actions }: {
  lead?: React.ReactNode
  title: React.ReactNode
  sub?: React.ReactNode
  mono?: boolean
  actions?: React.ReactNode
}) {
  return (
    <div className="flex items-center gap-3">
      {lead}
      <div className="min-w-0 flex-1">
        <div className="truncate text-[15px] font-semibold text-ink">{title}</div>
        {sub && <div className={cn('truncate text-[12px] text-ink-3', mono && 'font-mono')}>{sub}</div>}
      </div>
      {actions && <div className="flex flex-none items-center gap-2">{actions}</div>}
    </div>
  )
}

/**
 * E2 停用行状态胶囊「已停用」（owner-followups-1015 #10，2026-10-17 负责人裁决 followups 第 8 项）：取代原行内开关位。
 * 行（role=option）内曾嵌 A10 开关——option 里有可交互后代，读屏读不出（axe nested-interactive），30×18 也点不准；
 * 开关挪到详情，行只表状态，启用不出东西。也不再整行 opacity .6（压低文字对比度，foundations「对比度」）。
 * `dot`：行里没有别的状态点时（技能）胶囊内带 off 点；已有左侧状态点的（MCP）不重复。
 */
export function DisabledChip({ dot = false }: { dot?: boolean }) {
  const { t } = useTranslation()
  return (
    <Badge data-disabled-chip className="h-[18px] px-1.5 text-[11px]">
      {dot && <Dot tone="off" />}
      {t('settingsUi.disabledChip')}
    </Badge>
  )
}

/** 小节标题（「模型（3）」「槽位绑定」「经验条目（2）」）：12/500，右侧可放汇总或动作。 */
export function SubHead({ children, aside, className }: { children: React.ReactNode; aside?: React.ReactNode; className?: string }) {
  return (
    <div className={cn('mt-5 mb-2 flex items-center gap-2', className)}>
      <b className="min-w-0 flex-1 text-[12px] font-medium text-ink">{children}</b>
      {aside}
    </div>
  )
}

/** 卡片内滚动有限高的扫描结果面板（标题与操作钉住，只有行列表滚）。 */
export function ScanPanel({ title, hint, rows, actions }: {
  title: string
  hint?: string
  rows: React.ReactNode
  actions: React.ReactNode
}) {
  return (
    <div data-scan-panel className="mx-7 mb-5 flex max-h-[40vh] flex-col rounded-[var(--r-card)] bg-[var(--app-card)] p-3 shadow-[var(--shadow-card)]">
      <div className="mb-1.5 flex flex-none items-center gap-2">
        <b className="text-[12.5px] font-medium text-ink">{title}</b>
        {hint && <span className="text-[12px] text-ink-3">{hint}</span>}
      </div>
      <div data-scan-rows className="min-h-0 flex-1 overflow-y-auto">{rows}</div>
      <div className="mt-2 flex flex-none gap-2">{actions}</div>
    </div>
  )
}
