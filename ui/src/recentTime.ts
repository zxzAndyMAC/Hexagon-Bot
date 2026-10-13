// 启动页最近项目的时间显示（components E11 补设计，票 10）：7 天内相对时间（Intl.RelativeTimeFormat），
// 更早用月 / 日；记录缺时间（opened_at=0，旧版 recents.json）返回空串不显示。opened_at 单位是秒（lib.rs）。
const MIN = 60
const HOUR = 60 * MIN
const DAY = 24 * HOUR

export function recentTime(openedAtSecs: number, nowMs: number, locale: string): string {
  if (!openedAtSecs) return ''
  const diff = Math.max(0, Math.floor(nowMs / 1000) - openedAtSecs)
  if (diff >= 7 * DAY) {
    return new Date(openedAtSecs * 1000).toLocaleDateString(locale, { month: 'numeric', day: 'numeric' })
  }
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  if (diff < MIN) return rtf.format(0, 'second')
  if (diff < HOUR) return rtf.format(-Math.floor(diff / MIN), 'minute')
  if (diff < DAY) return rtf.format(-Math.floor(diff / HOUR), 'hour')
  return rtf.format(-Math.floor(diff / DAY), 'day')
}
