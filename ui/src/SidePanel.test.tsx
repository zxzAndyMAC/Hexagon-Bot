import { describe, expect, it, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { SidePanel } from './components/SidePanel'
import { useUiStore } from './store'
import type { ArtifactRow } from './gen/ArtifactRow'

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
