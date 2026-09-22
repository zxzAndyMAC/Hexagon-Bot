import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { CenterTab } from './App'
import { SidePanel } from './components/SidePanel'
import { bindingFor, formatBinding } from './keymap'
import { useUiStore } from './store'
import type { ArtifactRow } from './gen/ArtifactRow'
import type { RepoEntry } from './gen/RepoEntry'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

function Harness() {
  const tabs = useUiStore((s) => s.tabs)
  const activeTab = useUiStore((s) => s.activeTab)
  const tab = tabs.find((t) => t.id === activeTab) ?? tabs[0]
  return (
    <div>
      <main data-testid="center">
        {tab.kind === 'file' && <CenterTab tab={tab} />}
      </main>
      <SidePanel />
    </div>
  )
}

const TREE: Record<string, RepoEntry[]> = {
  '': [
    { name: 'src', path: 'src', kind: 'dir' },
    { name: 'README.md', path: 'README.md', kind: 'file' },
    { name: 'package.json', path: 'package.json', kind: 'file' },
  ],
  src: [{ name: 'main.ts', path: 'src/main.ts', kind: 'file' }],
}

function treeitem(el: HTMLElement, name: string) {
  return [...el.querySelectorAll('[role=treeitem]')].find((n) => n.textContent?.includes(name)) as HTMLElement
}

describe('右栏项目页文件树（票 11）', () => {
  const snap = useUiStore.getState()
  let root: Root | undefined
  let el: HTMLElement | undefined

  afterEach(() => {
    act(() => { root?.unmount() })
    el?.remove()
    useUiStore.setState(snap, true)
    vi.restoreAllMocks()
  })

  function mockTree() {
    vi.spyOn(api, 'listRepoDir').mockImplementation(async (rel = '') => TREE[rel] ?? [])
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('# hi\n')
    vi.spyOn(api, 'createRepoFile').mockResolvedValue(undefined)
    vi.spyOn(api, 'createRepoDir').mockResolvedValue(undefined)
    vi.spyOn(api, 'writeRepoFile').mockResolvedValue(undefined)
  }

  it('项目页按层级列目录，类型图标不同，产物页仍是交接清单', async () => {
    mockTree()
    const row: ArtifactRow = {
      id: 'a1', path: '.hexagon/specs/plan.md', kind: 'doc', tier: 'freeform',
      stage_run_id: null, author: null, version: 1, status: 'valid', upstream_id: null,
    }
    useUiStore.setState({ artifacts: [row], railOpen: true, sideTab: 'artifacts' })
    ;({ el, root } = await render(<Harness />))

    expect(el!.textContent).toContain('.hexagon/specs/plan.md')
    expect(el!.querySelector('[data-testid=file-tree]')).toBeNull()

    const project = [...el!.querySelectorAll('button')].find((b) => b.textContent === '项目')
    await act(async () => { project!.click() })
    expect(el!.querySelector('[data-testid=file-tree]')).toBeTruthy()
    expect(el!.textContent).not.toContain('.hexagon/specs/plan.md')
    expect(el!.textContent).not.toMatch(/调试器|debugger|插件/)

    const src = treeitem(el!, 'src')
    expect(src.querySelector('[data-icon]')!.getAttribute('data-icon')).toBe('folder')
    expect(treeitem(el!, 'README.md').querySelector('[data-icon]')!.getAttribute('data-icon')).toBe('file-text')
    expect(treeitem(el!, 'package.json').querySelector('[data-icon]')!.getAttribute('data-icon')).toBe('file-json')

    await act(async () => { src.click() })
    const main = treeitem(el!, 'main.ts')
    expect(main.querySelector('[data-icon]')!.getAttribute('data-icon')).toBe('file-code')
    expect(api.listRepoDir).toHaveBeenCalledWith('src')
  })

  it('新建文件、新建目录、刷新打到仓库命令，按钮带键位', async () => {
    mockTree()
    useUiStore.setState({ railOpen: true, sideTab: 'files' })
    ;({ el, root } = await render(<Harness />))

    const newFile = el!.querySelector('button[title*="新建文件"]') as HTMLButtonElement
    const newDir = el!.querySelector('button[title*="新建目录"]') as HTMLButtonElement
    const refresh = el!.querySelector('button[title*="刷新"]') as HTMLButtonElement
    expect(newFile.title).toContain(formatBinding(bindingFor('treeNewFile')))
    expect(newDir.title).toContain(formatBinding(bindingFor('treeNewFolder')))
    expect(refresh.title).toContain(formatBinding(bindingFor('treeRefresh')))

    const before = vi.mocked(api.listRepoDir).mock.calls.length
    await act(async () => { refresh.click() })
    expect(vi.mocked(api.listRepoDir).mock.calls.length).toBeGreaterThan(before)

    await act(async () => { newFile.click() })
    const input = el!.querySelector('input') as HTMLInputElement
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      set.call(input, 'notes.md')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => { (el!.querySelector('button[type=submit]') as HTMLButtonElement).click() })
    expect(api.createRepoFile).toHaveBeenCalledWith('notes.md')

    await act(async () => { newDir.click() })
    const input2 = el!.querySelector('input') as HTMLInputElement
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      set.call(input2, 'notes')
      input2.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => { (el!.querySelector('button[type=submit]') as HTMLButtonElement).click() })
    expect(api.createRepoDir).toHaveBeenCalledWith('notes')
  })

  it('点文件在中栏打开编辑器，右栏仍只是树', async () => {
    mockTree()
    useUiStore.setState({ railOpen: true, sideTab: 'files' })
    ;({ el, root } = await render(<Harness />))

    await act(async () => { treeitem(el!, 'README.md').click() })
    await act(async () => { await Promise.resolve() })

    const center = el!.querySelector('[data-testid=center]')!
    const rail = el!.querySelector('aside')!
    expect(useUiStore.getState().activeTab).toBe('file:README.md')
    expect(useUiStore.getState().tabs.some((t) => t.kind === 'timeline')).toBe(true)
    expect(useUiStore.getState().tabs.find((t) => t.id === 'file:README.md')?.kind).toBe('file')
    expect(center.querySelector('[data-testid=file-editor]')).toBeTruthy()
    expect(center.querySelector('[data-testid=file-editor]')!.getAttribute('data-path')).toBe('README.md')
    expect(rail.querySelector('[data-testid=file-editor]')).toBeNull()
    expect(rail.querySelector('[data-testid=file-tree]')).toBeTruthy()
    expect(api.readRepoFile).toHaveBeenCalledWith('README.md')

    const save = center.querySelector('button') as HTMLButtonElement
    expect(save.title).toContain(formatBinding(bindingFor('saveFile')))
  })
})
