import { describe, expect, it, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { SidePanel } from './components/SidePanel'
import { useUiStore } from './store'
import type { ArtifactRow } from './gen/ArtifactRow'
import type { StageRow } from './gen/StageRow'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

// 回归：产物长名曾把右侧状态 chip 挤成竖排（flex 默认可缩 +
// chip 无 nowrap）。钉住：路径行 title 给全名 + minWidth:0 截断，
// chip flexShrink:0+nowrap 保形。
describe('SidePanel 产物行长名', () => {
  const snap = useUiStore.getState()
  afterEach(() => useUiStore.setState(snap, true))

  it('路径截断、chip 不缩、悬浮 title 给全名', async () => {
    const path = '.hexagon/specs/web-product-requirements-document-v2-final.md'
    const row: ArtifactRow = {
      id: 'a1', path, kind: 'doc', tier: 'freeform', stage_run_id: null,
      author: null, version: 1, status: 'superseded', upstream_id: null,
    }
    useUiStore.setState({ artifacts: [row], railOpen: true, sideTab: 'artifacts' })
    const { el } = await render(<SidePanel />)

    const name = el.querySelector('span.mono[title]') as HTMLElement
    expect(name.getAttribute('title')).toBe(path)
    expect(name.style.minWidth).toBe('0')

    const chip = el.querySelector('.chip') as HTMLElement
    expect(chip.style.flexShrink).toBe('0')
    expect(chip.style.whiteSpace).toBe('nowrap')
  })
})

describe('右栏阶段进度', () => {
  const snap = useUiStore.getState()
  afterEach(() => useUiStore.setState(snap, true))

  it('阶段是单独页签，其他页签不列出阶段', async () => {
    const stages: StageRow[] = [
      { run_id: 'r1', stage: '规格', seq: 0, state: 'done' },
      { run_id: 'r2', stage: '实现', seq: 1, state: 'active' },
    ]
    useUiStore.setState({ railOpen: true, sideTab: 'files', stages })
    const { el } = await render(<SidePanel />)
    expect(el.querySelector('[data-stage-rail]')).toBeNull()
    const tab = el.querySelector('[data-side-tab="stages"]') as HTMLButtonElement
    expect(tab).toBeTruthy()
    await act(async () => { tab.click() })
    const rail = el.querySelector('[data-stage-rail]')
    expect(rail?.textContent).toContain('规格')
    expect(rail?.textContent).toContain('实现')
    expect(rail?.querySelector('button')).toBeNull()
  })
})

it('materialization conflicts remain visible as pending recovery', async () => {
  const saved = useUiStore.getState()
  const row: ArtifactRow = Object.assign({
    id: 'pending-art', path: 'notes.md', kind: 'doc', tier: 'freeform' as const,
    stage_run_id: null, author: null, version: 2, status: 'pending' as const, upstream_id: null,
  }, { materialization: 'pending_recovery' })
  useUiStore.setState({ artifacts: [row], railOpen: true, sideTab: 'artifacts' })
  const { el, root } = await render(<SidePanel />)
  expect(el.textContent).toMatch(/Pending recovery|待恢复/)
  await act(async () => root.unmount())
  el.remove()
  useUiStore.setState(saved, true)
})
