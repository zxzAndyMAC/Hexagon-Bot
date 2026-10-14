// 时间范围 RangePicker（components A23，usage#range）：触发钮 calendar 图标 + 「近 30 天 ▾」→ 弹层
// 左预设 2×4（即点即生效）、右月历区间（起止都选了才可「应用」）；日期都按 UTC 日界（usage.ts，票 14 裁决）。
// 日历格是本地 Date：取它的年月日当作 UTC 日期串，与后端 date(created_at) 分桶对齐。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DateRange } from 'react-day-picker'
import { enUS, es, fr, ja, pt, zhCN, zhTW } from 'react-day-picker/locale'
import { Icon } from '../Icon'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'
import { bucketTick, customRange, daysAgo, presetRange, RANGE_PRESETS, type UsageRange } from '../../usage'

const LOCALES = { 'zh-CN': zhCN, 'zh-TW': zhTW, ja, es, fr, pt, en: enUS } as const

/** UTC 日期串 → 同年月日的本地 Date（只给日历显示用）。 */
const toLocal = (day: string) => {
  const [y, m, d] = day.split('-').map(Number)
  return new Date(y, m - 1, d)
}
/** 日历选中的本地 Date → 同年月日的日期串（当 UTC 日用）。 */
const toDay = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`

export function rangeLabel(range: UsageRange, t: (k: string) => string): string {
  if (range.preset !== 'custom') return t(`usage.p_${range.preset}`)
  return `${bucketTick(range.from ?? '', 'day')} – ${bucketTick(range.to ?? '', 'day')}`
}

export function RangePicker({ range, onChange, ...rest }: {
  range: UsageRange
  onChange: (r: UsageRange) => void
} & Record<`data-${string}`, string | undefined>) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState<DateRange | undefined>()
  const today = toLocal(daysAgo(0))
  const locale = LOCALES[i18n.language as keyof typeof LOCALES] ?? enUS

  const onOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) setDraft(range.from && range.to ? { from: toLocal(range.from), to: toLocal(range.to) } : undefined)
  }
  const pick = (r: UsageRange) => { onChange(r); setOpen(false) }
  const ready = draft?.from && draft?.to

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button size="sm" aria-label={`${t('usage.range')}: ${rangeLabel(range, t)}`} {...rest}>
          <Icon name="calendar" size={12} />
          {rangeLabel(range, t)}
          <Icon name="chevron-down" size={12} className="text-ink-3" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="flex w-auto overflow-hidden p-0" data-range-pop>
        <div role="group" aria-label={t('usageDetail.presets')} className="grid grid-cols-2 content-start gap-1 border-r border-line-soft p-3">
          {RANGE_PRESETS.map((p) => (
            <Button
              key={p}
              size="xs"
              variant="ghost"
              data-preset={p}
              aria-pressed={range.preset === p}
              className={cn(range.preset === p && 'bg-hover-2 text-ink')}
              onClick={() => pick(presetRange(p))}
            >
              {t(`usage.p_${p}`)}
            </Button>
          ))}
        </div>
        <div className="px-4 pt-3.5 pb-3.5">
          <Calendar
            mode="range"
            locale={locale}
            selected={draft}
            onSelect={setDraft}
            defaultMonth={draft?.to ?? today}
            disabled={{ after: today }}
            excludeDisabled
          />
          <div className="mt-3 flex items-center gap-2">
            <span className="flex-1 text-[12px] text-ink-3 tabular-nums" data-range-draft>
              {draft?.from ? bucketTick(toDay(draft.from), 'day') : '…'} – {draft?.to ? bucketTick(toDay(draft.to), 'day') : '…'}
            </span>
            <Button
              size="xs"
              variant="primary"
              disabled={!ready}
              onClick={() => { if (draft?.from && draft.to) pick(customRange(toDay(draft.from), toDay(draft.to))) }}
            >
              {t('usage.apply')}
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
