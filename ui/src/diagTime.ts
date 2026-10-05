/** QA17: diagnostic timestamps are stored in UTC; owners read local time. */
export function localLogTime(timestamp: string, locale: string, timeZone?: string): string {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return '—'
  return date.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false, timeZone })
}
