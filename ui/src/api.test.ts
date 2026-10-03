import { beforeAll, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { asCmdError, batchTurnDeltas, errText, type TurnDelta } from './api'

it('coalesces a token burst without losing reset, plan, call or done boundaries', () => {
  vi.useFakeTimers()
  try {
    const got: TurnDelta[] = []
    const batch = batchTurnDeltas((d) => got.push(d))
    const frame: TurnDelta = { agent_id: 'a', stage_run_id: null, call: 0, text: '', thinking: '', reset: false, done: false, waiting: false }
    for (let i = 0; i < 100; i++) batch.push({ ...frame, plan: 'p', thinking: 't' })
    expect(got).toHaveLength(0)
    vi.advanceTimersByTime(50)
    expect(got).toHaveLength(1)
    expect(got[0].plan).toBe('p'.repeat(100))
    expect(got[0].thinking).toBe('t'.repeat(100))
    batch.push({ ...frame, call: 1, text: 'old' })
    batch.push({ ...frame, call: 1, reset: true })
    batch.push({ ...frame, call: 1, text: 'new' })
    batch.push({ ...frame, call: 2, text: 'next' })
    batch.push({ ...frame, call: 2, done: true })
    expect(got.slice(1).map((d) => d.reset ? 'reset' : d.done ? 'done' : d.text)).toEqual(['old', 'reset', 'new', 'next', 'done'])
    batch.push({ ...frame, text: 'discard on unsubscribe' })
    batch.cancel()
    vi.runAllTimers()
    expect(got).toHaveLength(6)
  } finally { vi.useRealTimers() }
})

// IPC 错误信封边界（ADR 0054，arch-review 票 06）：已知 code 走
// errors.<code> i18n，未知 code 透传服务端 message，非信封值兜底。

beforeAll(() => i18n.changeLanguage('en'))

describe('IPC error envelope', () => {
  it('maps known codes to i18n text', () => {
    expect(errText({ code: 'paused', message: 'project paused' })).toBe(
      'Project is paused — resume it first',
    )
  })

  it('passes through message for unknown codes', () => {
    expect(errText({ code: 'some_unlisted_code', message: 'raw detail 42' })).toBe('raw detail 42')
  })

  it('normalizes legacy string errors', () => {
    expect(asCmdError('plain failure')).toEqual({ code: 'unknown', message: 'plain failure' })
    expect(errText('plain failure')).toBe('plain failure')
  })

  it('maps backend-shaped envelopes across the vocabulary', () => {
    expect(errText({ code: 'git_cli', message: 'git push: rejected' })).toBe(
      'Git command failed — check remote state and credentials',
    )
    expect(errText({ code: 'not_queued', message: 'question q1 not queued' })).toBe(
      'This card was already handled',
    )
  })
})

it('explains experience eligibility failures in Chinese without exposing backend details', async () => {
  await i18n.changeLanguage('zh-CN')
  try {
    expect(errText({ code: 'unreviewed_experience', message: 'internal detail' })).toContain('当前作者')
    expect(errText({ code: 'frozen_experience', message: 'internal detail' })).toContain('经验已冻结')
    expect(errText({ code: 'stale_experience', message: 'internal detail' })).toContain('重新复审')
  } finally { await i18n.changeLanguage('en') }
})
