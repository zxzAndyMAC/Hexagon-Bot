import { beforeAll, describe, expect, it } from 'vitest'
import i18n from './i18n'
import { asCmdError, errText } from './api'

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
