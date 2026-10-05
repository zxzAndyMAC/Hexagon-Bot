import { expect, it } from 'vitest'
import { localLogTime } from './diagTime'
it('converts UTC records across local day boundaries', () => {
  expect(localLogTime('2026-10-05T22:09:10Z', 'zh-CN', 'Asia/Shanghai')).toBe('06:09:10')
  expect(localLogTime('2026-10-05T22:09:10Z', 'en-GB', 'UTC')).toBe('22:09:10')
})
it('does not display an invalid timestamp as a plausible clock time', () => {
  expect(localLogTime('invalid timestamp', 'en-GB')).toBe('—')
})
