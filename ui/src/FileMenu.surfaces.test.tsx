// 共用文件菜单（components A16b，owner-followups-1015 #32，2026-10-21 负责人裁决「凡是显示一个有盘上路径的文件的表面，
// 右键都给同一份文件菜单」）：每个表面一条用例，钉住项序（同文件树行）、不适用项不渲染、根外 / 不落盘没有路径类项。
import { act } from 'react'
import type { ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { TooltipProvider } from './components/ui/tooltip'
import { ArtifactList } from './components/shell/ArtifactList'
import { Canvas } from './components/shell/Canvas'
import type { ArtifactRow } from './gen/ArtifactRow'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: Root | undefined
let el: HTMLDivElement | undefined
async function render(node: ReactNode) {
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => { root!.render(<TooltipProvider>{node}</TooltipProvider>) })
  return el
}
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })
async function rightClick(target: Element) {
  await act(async () => {
    const r = target.getBoundingClientRect()
    target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: r.left + 4, clientY: r.top + 4 }))
  })
  await flush()
}
const menu = () => document.querySelector('[data-slot="context-menu-content"]') as HTMLElement | null
/** 当前右键菜单的项序（data-ctx-item）；没开菜单 = null。 */
const items = () => {
  const m = menu()
  return m ? [...m.querySelectorAll<HTMLElement>('[data-ctx-item]')].map((n) => n.dataset.ctxItem) : null
}
const FULL = ['open', 'insert', 'copyPath', 'reveal', 'openWith']
const NO_OPEN = ['insert', 'copyPath', 'reveal', 'openWith']

const snap = useUiStore.getState()
beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ projectRoot: '/proj', tabs: [], activeTab: null, toasts: [] })
  vi.spyOn(api, 'openWithCandidates').mockResolvedValue([])
  vi.spyOn(api, 'revealRepoPath').mockResolvedValue(undefined)
})
afterEach(async () => {
  await act(async () => root?.unmount())
  el?.remove()
  document.body.innerHTML = ''
  useUiStore.setState(snap, true)
  vi.restoreAllMocks()
})

const artifact = (path: string, extra: Partial<ArtifactRow> = {}): ArtifactRow => ({
  id: path, path, kind: 'doc', tier: 'freeform', stage_run_id: null, author: null, version: 1, status: 'valid', upstream_id: null, ...extra,
})

describe('产物清单行（#32 表面 1）', () => {
  it('右键 = 文件树行同序全套；「打开」打开产物页签；打开方式按项目相对路径读候选', async () => {
    useUiStore.setState({ artifacts: [artifact('docs/plan.pdf')] })
    const host = await render(<ArtifactList />)
    await rightClick(host.querySelector('[role=listitem]')!)
    expect(items()).toEqual(FULL)
    expect(api.openWithCandidates).toHaveBeenCalledWith('docs/plan.pdf')
    await act(async () => { menu()!.querySelector<HTMLElement>('[data-ctx-item="open"]')!.click() })
    expect(useUiStore.getState().activeTab).toBe('art:docs/plan.pdf')
  })

  it('待恢复（盘上还没写成）只给「打开」', async () => {
    useUiStore.setState({ artifacts: [Object.assign(artifact('docs/a.md'), { materialization: 'pending_recovery' })] })
    const host = await render(<ArtifactList />)
    await rightClick(host.querySelector('[role=listitem]')!)
    expect(items()).toEqual(['open'])
  })
})


describe('画布空态最近产物（#32 表面 2）', () => {
  it('右键 = 全套；项目根外的绝对路径产物只剩「打开」', async () => {
    useUiStore.setState({ artifacts: [artifact('/elsewhere/x.md'), artifact('report.pptx')], canvasOpen: true, tabs: [], activeTab: null })
    const host = await render(<Canvas open mode="side" width={600} />)
    const rows = [...host.querySelectorAll<HTMLElement>('[data-canvas-recent]')]
    expect(rows.map((r) => r.textContent)).toEqual(['report.pptx', '/elsewhere/x.md'])
    await rightClick(rows[0])
    expect(items()).toEqual(FULL)
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    await flush()
    await rightClick(rows[1])
    expect(items()).toEqual(['open'])
  })
})
