import { describe, expect, it } from 'vitest'
import { readSettingsListWidth, SETTINGS_LIST, SETTINGS_LIST_KEY, writeSettingsListWidth } from './settingsLayout'

function mem(init: Record<string, string> = {}) {
  const m = new Map(Object.entries(init))
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => { m.set(k, v) },
    removeItem: (k: string) => { m.delete(k) },
    get: (k: string) => m.get(k),
  }
}

describe('设置列表宽度持久化（票 11，interaction §1）', () => {
  it('键名与尺寸表一致：默认 280，220–360', () => {
    expect(SETTINGS_LIST_KEY).toBe('hexagon.layout.settingsList')
    expect(SETTINGS_LIST).toMatchObject({ default: 280, min: 220, max: 360 })
  })

  it('无记忆或内容损坏时回默认', () => {
    expect(readSettingsListWidth(mem())).toBe(280)
    expect(readSettingsListWidth(mem({ [SETTINGS_LIST_KEY]: 'garbage' }))).toBe(280)
    expect(readSettingsListWidth(mem({ [SETTINGS_LIST_KEY]: '{"width":"x"}' }))).toBe(280)
  })

  it('读回时夹到上下限', () => {
    expect(readSettingsListWidth(mem({ [SETTINGS_LIST_KEY]: '{"width":100}' }))).toBe(220)
    expect(readSettingsListWidth(mem({ [SETTINGS_LIST_KEY]: '{"width":900}' }))).toBe(360)
  })

  it('写入取整，读回同值', () => {
    const s = mem()
    writeSettingsListWidth(s, 301.6)
    expect(JSON.parse(s.get(SETTINGS_LIST_KEY)!)).toEqual({ width: 302 })
    expect(readSettingsListWidth(s)).toBe(302)
  })
})
