import { describe, expect, it } from 'vitest'
import { ACTIONS } from '../../keymap'
import { groupActions, keyGroupOf } from './keyGroups'

describe('键盘分区动作分组（票 11，settings#keys）', () => {
  it('外壳与画布动作进「界面与导航」，含 02 / 03 新增与改名的动作', () => {
    for (const id of ['toggleSidebar', 'toggleCanvas', 'canvasFullWidth', 'focusMode', 'commandPalette', 'timelineLatest']) {
      expect(keyGroupOf(id)).toBe('layout')
    }
  })

  it('裁决键进「裁决」，托管与电脑操作各成组', () => {
    expect(keyGroupOf('approve')).toBe('decide')
    expect(keyGroupOf('dismissPending')).toBe('decide')
    expect(keyGroupOf('managedPause')).toBe('managed')
    expect(keyGroupOf('desktopPause')).toBe('computer')
    expect(keyGroupOf('browserPanel')).toBe('computer')
    expect(keyGroupOf('sessionPause')).toBe('session')
    expect(keyGroupOf('saveFile')).toBe('files')
  })

  it('认不出的新动作进「其他」，分组后一个不少、不重复', () => {
    expect(keyGroupOf('somethingNew')).toBe('other')
    const groups = groupActions(ACTIONS)
    const ids = groups.flatMap((g) => g.actions.map((a) => a.id))
    expect(ids.sort()).toEqual(ACTIONS.map((a) => a.id).sort())
    expect(groups[0].group).toBe('decide')
  })
})
