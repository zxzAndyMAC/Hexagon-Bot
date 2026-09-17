import { describe, expect, it } from 'vitest'
import { useUiStore } from './store'

describe('ui store', () => {
  it('holds core status as pure UI state', () => {
    useUiStore.getState().setCoreStatus('ok')
    expect(useUiStore.getState().coreStatus).toBe('ok')
  })
})
