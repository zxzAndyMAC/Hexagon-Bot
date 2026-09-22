import { describe, expect, it } from 'vitest'
import { applyCreateFailure, CREATE_STEPS } from './createProgress'

describe('wizard create progress (ticket 14)', () => {
  it('lists check dir, git, roles, key recheck, then open', () => {
    expect(CREATE_STEPS).toEqual([
      'check_dir',
      'git',
      'persist_roles',
      'recheck_keys',
      'open_project',
    ])
  })

  it('stops on the failed step and does not keep later steps done', () => {
    const keys = applyCreateFailure(
      ['check_dir', 'git', 'persist_roles'],
      'missing_keys',
    )
    expect(keys.failed).toBe('recheck_keys')
    expect(keys.done).toEqual(['check_dir', 'git', 'persist_roles'])
    expect(keys.done).not.toContain('open_project')

    const dirty = applyCreateFailure(['check_dir'], 'dirty_tree')
    expect(dirty.failed).toBe('git')
    expect(dirty.done).toEqual(['check_dir'])

    const early = applyCreateFailure([], 'agents_md_exists')
    expect(early.failed).toBe('check_dir')
    expect(early.done).toEqual([])
  })

  it('a late shell error unmarks open even if that step was already reported', () => {
    const late = applyCreateFailure([...CREATE_STEPS], 'internal')
    expect(late.failed).toBe('open_project')
    expect(late.done).not.toContain('open_project')
  })
})
