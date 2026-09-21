import { describe, expect, it, vi } from 'vitest'
import { matches } from './keymap'

const ev = (key: string, mods: Partial<KeyboardEvent> = {}) =>
  new KeyboardEvent('keydown', { key, ...mods })

describe('matches 平台精确化（票 15）', () => {
  // happy-dom UA 不含 Mac → isMac=false，走 ctrlKey 分支
  it('非 mac：Ctrl+↵ 命中 mod+Enter；metaKey 不命中', () => {
    expect(matches(ev('Enter', { ctrlKey: true }), 'mod+Enter')).toBe(true)
    expect(matches(ev('Enter', { metaKey: true }), 'mod+Enter')).toBe(false)
    // 双修饰同时按下不命中（防 Ctrl+⌘+↵ 误触）
    expect(matches(ev('Enter', { ctrlKey: true, metaKey: true }), 'mod+Enter')).toBe(false)
  })

  it('alt/shift 组合仍精确', () => {
    expect(matches(ev('s', { ctrlKey: true, altKey: true }), 'alt+mod+s')).toBe(true)
    expect(matches(ev('s', { ctrlKey: true }), 'alt+mod+s')).toBe(false)
    expect(matches(ev('s', { ctrlKey: true, altKey: true, shiftKey: true }), 'alt+mod+s')).toBe(false)
  })

  it('macOS 下 mod=metaKey：Ctrl+↵ 不命中批准', async () => {
    vi.resetModules()
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
      userAgentData: { platform: 'macOS' },
    })
    try {
      const km = await import('./keymap')
      expect(km.isMac).toBe(true)
      expect(km.matches(ev('Enter', { metaKey: true }), 'mod+Enter')).toBe(true)
      expect(km.matches(ev('Enter', { ctrlKey: true }), 'mod+Enter')).toBe(false)
    } finally {
      vi.unstubAllGlobals()
      vi.resetModules()
    }
  })
})
