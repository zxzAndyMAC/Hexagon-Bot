// 「稍后继续…」时间弹层内容（components C12a）。#11 起入口在头部 ⋯ 菜单、⌘K 与 sessionScheduleOnce 键位，
// 弹层挂在会话胶囊的状态槽上（胶囊健康时不出现，弹层锚点仍在）。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { Icon } from './Icon'

// 规范 C12a：时间选择四档；返回毫秒时间戳。
const LATER_CHOICES: { key: string; at: (now: Date) => number }[] = [
  { key: 'in5', at: (now) => now.getTime() + 5 * 60_000 },
  { key: 'in30', at: (now) => now.getTime() + 30 * 60_000 },
  { key: 'in120', at: (now) => now.getTime() + 120 * 60_000 },
  { key: 'tomorrow9', at: (now) => new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 9).getTime() },
]

export function SessionLaterPicker({ onSchedule }: { onSchedule: (fireAtMs: number) => void }) {
  const { t } = useTranslation()
  // Gap D1（2026-10-16）：预设时间只选中，「预约」primary 才落预约；曾经点预设即预约，误点无从撤回。
  const [pick, setPick] = useState<string | null>(null)
  const box = useRef<HTMLDivElement>(null)
  // 从 ⋯ 菜单 / ⌘K 打开时，菜单与面板关闭后会把焦点还给各自的触发处（radix 在卸载后的下一拍还焦点）；
  // 这里晚一拍把焦点放进弹层首项，否则焦点落回 ⋯ / 输入条，键盘用户看不到弹层在哪。
  useEffect(() => {
    const id = setTimeout(() => {
      if (box.current && !box.current.contains(document.activeElement)) box.current.querySelector('button')?.focus()
    }, 0)
    return () => clearTimeout(id)
  }, [])
  const schedule = () => {
    const choice = LATER_CHOICES.find((c) => c.key === pick)
    // 按确认时刻起算，不按点选时刻——弹层停留期间「5 分钟后」不应缩水。
    if (choice) onSchedule(choice.at(new Date()))
  }
  return (
    <div ref={box}>
      <div className="px-2 py-1.5 text-[12px] text-ink-3">{t('session.scheduleTitle')}</div>
      <div role="group" aria-label={t('session.bookPick')}>
        {LATER_CHOICES.map((choice) => {
          const picked = pick === choice.key
          return (
            <Button key={choice.key} size="sm" variant={picked ? 'quiet' : 'ghost'} aria-pressed={picked} className="w-full justify-start" data-later-choice={choice.key}
              onClick={() => setPick(choice.key)}>
              {t(`session.${choice.key}`)}
              {picked && <Icon name="check" size={14} className="ml-auto text-primary" />}
            </Button>
          )
        })}
      </div>
      <div className="flex justify-end px-1 pt-1.5 pb-0.5">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button size="sm" variant="primary" aria-disabled={pick ? undefined : true} data-later-confirm
              onClick={schedule}>{t('session.schedule')}</Button>
          </TooltipTrigger>
          {!pick && <TooltipContent>{t('session.bookPick')}</TooltipContent>}
        </Tooltip>
      </div>
    </div>
  )
}
