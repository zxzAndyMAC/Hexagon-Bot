import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './api'
import { setLang } from './i18n'
import { Wizard } from './components/Wizard'
import type { DirReport } from './gen/DirReport'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  // 目录体检是 200ms 防抖，冲掉后再断言 chips
  await act(async () => { await vi.advanceTimersByTimeAsync(250) })
  return { el, root }
}

function report(over: Partial<DirReport>): DirReport {
  return {
    exists: true, empty: false, is_git: true, dirty: false, has_workbench: false, instructions: null,
    ...over,
  }
}

const draft = {
  dir: '/repo/demo', name: 'Demo', roles: [], roleOverrides: {},
  mode: 'pack', packName: '规格驱动', fastRole: '', initGit: false,
  genAgents: false, agentsMd: '',
}

describe('向导目录（ADR 0060）', () => {
  beforeEach(async () => {
    vi.useFakeTimers()
    localStorage.clear()
    await setLang('en')
    localStorage.setItem('hexagon.wizard', JSON.stringify(draft))
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    localStorage.clear()
  })

  it('脏树不停步，并说明改动会保留', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ dirty: true }))
    const { el, root } = await render(<Wizard onDone={() => {}} />)
    expect(el.textContent).toContain('Uncommitted changes stay as they are')
    expect(el.textContent).not.toContain('stops here')
    const next = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Next')
    expect(next?.disabled).toBe(false)
    root.unmount()
  })

  it('已有工作台状态不能下一步新建，打开走 openRecent', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ has_workbench: true, dirty: true }))
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const onDone = vi.fn()
    const { el, root } = await render(<Wizard onDone={onDone} />)
    const next = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Next')
    expect(next?.disabled).toBe(true)
    expect(el.textContent).toContain('open it instead of creating again')
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Open this project')!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(open).toHaveBeenCalledWith('/repo/demo')
    expect(create).not.toHaveBeenCalled()
    expect(onDone).toHaveBeenCalled()
    root.unmount()
  })
})
