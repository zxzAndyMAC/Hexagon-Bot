// ui-redesign 票 01：头像确定色从连续 HSL 收敛到规范 foundations §1.5 的 c1–c8 八色。
import { describe, expect, it } from 'vitest'
import { agentSwatch } from './colors'

describe('agentSwatch', () => {
  it('is stable per agent id and always one of c1–c8', () => {
    for (const id of ['a0', 'a1', 'pm', 'dev-2', '', 'ünïcode']) {
      const n = agentSwatch(id)
      expect(n).toBe(agentSwatch(id))
      expect(n).toBeGreaterThanOrEqual(1)
      expect(n).toBeLessThanOrEqual(8)
    }
  })

  it('spreads adjacent ids across different swatches', () => {
    const picks = new Set(['a0', 'a1', 'a2', 'a3', 'a4', 'a5'].map(agentSwatch))
    expect(picks.size).toBeGreaterThanOrEqual(4)
  })
})
