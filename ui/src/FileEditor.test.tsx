import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { FileEditor } from './components/FileEditor'
import { useUiStore } from './store'
import type { ArtifactRow } from './gen/ArtifactRow'

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

function segBtn(el: HTMLElement, label: string) {
  return [...el.querySelectorAll('.seg button')].find((b) => b.textContent === label) as HTMLButtonElement | undefined
}

async function type(el: HTMLElement, value: string) {
  const ta = el.querySelector('textarea') as HTMLTextAreaElement
  await act(async () => {
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
    set.call(ta, value)
    ta.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

describe('文件页三态视图（预览/对比回归）', () => {
  const snap = useUiStore.getState()
  let root: Root | undefined
  let el: HTMLElement | undefined

  afterEach(() => {
    act(() => { root?.unmount() })
    el?.remove()
    useUiStore.setState(snap, true)
    vi.restoreAllMocks()
  })

  it('md 文件可预览，预览的是编辑中的实时内容', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('# 标题\n\n原文\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    ;({ el, root } = await render(<FileEditor path="docs/note.md" />))

    // 先改未保存，再切预览——预览必须渲染编辑缓冲区而不是盘上旧文。
    await type(el!, '# 标题\n\n新写的段落\n')
    expect(el!.querySelector('.dot.warn')).toBeTruthy()
    await act(async () => { segBtn(el!, '预览')!.click() })
    await act(async () => {})

    const body = el!.querySelector('.msg-body')!
    expect(body).toBeTruthy()
    expect(body.querySelector('h1')!.textContent).toBe('标题')
    expect(body.textContent).toContain('新写的段落')
    expect(body.textContent).not.toContain('原文')
  })

  it('非 md 文件没有预览档，只有编辑/对比', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('const a = 1\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    ;({ el, root } = await render(<FileEditor path="src/a.ts" />))
    expect(segBtn(el!, '编辑')).toBeTruthy()
    expect(segBtn(el!, '对比')).toBeTruthy()
    expect(segBtn(el!, '预览')).toBeUndefined()
  })

  it('对比默认基线=已保存，现读盘上内容 vs 当前缓冲区', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('line1\nline2\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    ;({ el, root } = await render(<FileEditor path="src/a.ts" />))

    await type(el!, 'line1\nline2\nline3\n')
    await act(async () => { segBtn(el!, '对比')!.click() })
    await act(async () => {})

    // + 号是 ins 行符号；-del 同理（DiffView 行渲染）
    expect(el!.textContent).toContain('+line3')
    // 未跟踪/非 git：HEAD 基线不出现
    expect(segBtn(el!, 'HEAD')).toBeUndefined()
  })

  it('有 HEAD 基线时出现 HEAD 选项并可对比', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('new line\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue('old line\n')
    ;({ el, root } = await render(<FileEditor path="src/a.ts" />))
    await act(async () => { segBtn(el!, '对比')!.click() })
    await act(async () => {})

    const head = segBtn(el!, 'HEAD')
    expect(head).toBeTruthy()
    await act(async () => { head!.click() })
    await act(async () => {})
    expect(el!.textContent).toContain('-old line')
    expect(el!.textContent).toContain('+new line')
  })

  it('.hexagon 产物文件给出版本链基线', async () => {
    const rows: ArtifactRow[] = [
      { id: 'a1', path: 'specs/prd.md', kind: '规格', tier: 'skeleton', stage_run_id: 'r0', author: 'a0', version: 1, status: 'superseded', upstream_id: null },
      { id: 'a2', path: 'specs/prd.md', kind: '规格', tier: 'skeleton', stage_run_id: 'r0', author: 'a0', version: 2, status: 'stamped', upstream_id: null },
    ]
    useUiStore.setState({ artifacts: rows })
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('当前盘上内容\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    vi.spyOn(api, 'artifactContentAt').mockImplementation(async (_p, v) => `版本${v}内容\n`)
    ;({ el, root } = await render(<FileEditor path=".hexagon/specs/prd.md" />))

    await act(async () => { segBtn(el!, '对比')!.click() })
    await act(async () => {})
    expect(segBtn(el!, 'v1')).toBeTruthy()
    expect(segBtn(el!, 'v2')).toBeTruthy()

    await act(async () => { segBtn(el!, 'v1')!.click() })
    await act(async () => {})
    expect(api.artifactContentAt).toHaveBeenCalledWith('specs/prd.md', 1)
    expect(el!.textContent).toContain('-版本1内容')
  })

  it('与基线一致时给结论而不是整屏等同行', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('same\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    ;({ el, root } = await render(<FileEditor path="src/a.ts" />))
    await act(async () => { segBtn(el!, '对比')!.click() })
    await act(async () => {})
    expect(el!.textContent).toContain('与基线一致')
  })

  it('保存写回缓冲区并清脏标记', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('a\n')
    const write = vi.spyOn(api, 'writeRepoFile').mockResolvedValue(undefined)
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    ;({ el, root } = await render(<FileEditor path="src/a.ts" />))

    await type(el!, 'a\nb\n')
    expect(el!.querySelector('.dot.warn')).toBeTruthy()
    const save = [...el!.querySelectorAll('button')].find((b) => b.title.includes('保存'))!
    await act(async () => { save.click() })
    expect(write).toHaveBeenCalledWith('src/a.ts', 'a\nb\n')
    expect(el!.querySelector('.dot.warn')).toBeNull()
  })
})
