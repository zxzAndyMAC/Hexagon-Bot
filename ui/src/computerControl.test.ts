import { describe, expect, it } from 'vitest'
import { browserLaunchBlock, canPauseComputer, controlSummary } from './computerControl'
import type { DesktopStatus } from './gen/DesktopStatus'

const base: DesktopStatus = { project_root: '/a', enabled: true, active_project: null, active_agent: null, busy: false, paused: false, outcome_unknown: false, screenshot_count: 0 }
const names = (id: string) => ({ a1: 'QA' } as Record<string, string>)[id] ?? id

describe('controlSummary (components D19 头部控制状态)', () => {
  it('is hidden without status, or when computer use is off and nothing is connected', () => {
    expect(controlSummary(null, false, names)).toEqual({ kind: 'hidden' })
    expect(controlSummary({ ...base, enabled: false }, false, names)).toEqual({ kind: 'hidden' })
  })
  it('names who controls what: browser when a browser session is connected, otherwise the computer', () => {
    expect(controlSummary({ ...base, active_agent: 'a1', active_project: '/a' }, true, names)).toEqual({ kind: 'active', agent: 'QA', target: 'browser' })
    expect(controlSummary({ ...base, active_agent: 'a1', active_project: '/a' }, false, names)).toEqual({ kind: 'active', agent: 'QA', target: 'computer' })
  })
  it('reports control held by another project without naming a local role', () => {
    expect(controlSummary({ ...base, active_agent: 'a1', active_project: '/b' }, false, names)).toEqual({ kind: 'elsewhere' })
  })
  it('paused wins over activity; idle when enabled or a browser is connected', () => {
    expect(controlSummary({ ...base, paused: true, active_agent: 'a1', active_project: '/a' }, true, names)).toEqual({ kind: 'paused' })
    expect(controlSummary(base, false, names)).toEqual({ kind: 'idle', target: 'computer' })
    expect(controlSummary({ ...base, enabled: false }, true, names)).toEqual({ kind: 'idle', target: 'browser' })
  })
})

describe('canPauseComputer', () => {
  it('offers pause only while something could still act', () => {
    expect(canPauseComputer(null)).toBe(false)
    expect(canPauseComputer(base)).toBe(true)
    expect(canPauseComputer({ ...base, paused: true })).toBe(false)
    expect(canPauseComputer({ ...base, enabled: false })).toBe(false)
    expect(canPauseComputer({ ...base, enabled: false, active_agent: 'a1' })).toBe(true)
  })
})

describe('browserLaunchBlock (D19 禁用：写明原因)', () => {
  it('names why opening/connecting is disabled', () => {
    expect(browserLaunchBlock(null)).toBe('loading')
    expect(browserLaunchBlock({ ...base, enabled: false })).toBe('disabled')
    expect(browserLaunchBlock({ ...base, paused: true })).toBe('paused')
    expect(browserLaunchBlock(base)).toBeNull()
  })
})
