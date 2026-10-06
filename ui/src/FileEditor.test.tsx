import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { FileEditor } from './components/FileEditor'
import { ConfirmHost } from './components/ConfirmDialog'
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

  beforeEach(() => {
    useUiStore.setState({ projectRoot: '/test', fileEdits: {} })
    vi.spyOn(api, 'readRepoFileSnapshot').mockImplementation(async (path, project_root) => ({
      path, project_root, generation: 1, content: await api.readRepoFile(path),
    }))
  })

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
    expect(write).toHaveBeenCalledWith({ path: 'src/a.ts', project_root: '/test', generation: 1, content: 'a\n' }, 'a\nb\n')
    expect(el!.querySelector('.dot.warn')).toBeNull()
  })

  it('未保存编辑在负责人选择前拦住关闭，取消后仍可继续编辑', async () => {
    // competitor benchmark I1, 2026-10-06: project IPC used to invalidate
    // identity immediately, leaving a dirty buffer able to target another root.
    useUiStore.setState({ projectRoot: '/project/A' })
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('project A original')
    ;({ el, root } = await render(<FileEditor path="README.md" />))
    await type(el!, 'project A unsaved edit')
    let result!: Promise<string>
    await act(async () => {
      result = api.closeProject().then(() => 'closed', () => 'cancelled')
    })
    expect(useUiStore.getState().projectRoot).toBe('/project/A')
    expect(useUiStore.getState().confirmReq).not.toBeNull()
    await act(async () => useUiStore.getState().clearConfirm())
    expect(await result).toBe('cancelled')
    expect(el!.querySelector('textarea')!.value).toBe('project A unsaved edit')
  })

  it.each(['保存', '放弃修改'])('离开项目选择%s只作用来源项目', async choice => {
    useUiStore.setState({ projectRoot: '/project/A' })
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    const saved: { root: string | null; content: string }[] = []
    vi.spyOn(api, 'writeRepoFile').mockImplementation(async (_path, content) => {
      saved.push({ root: useUiStore.getState().projectRoot, content })
    })
    ;({ el, root } = await render(<><FileEditor path="README.md" /><ConfirmHost /></>))
    await type(el!, 'unsaved')
    let leaving!: Promise<void>
    await act(async () => { leaving = api.closeProject() })
    const button = [...el!.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find(b => b.textContent === choice)!
    await act(async () => { button.click(); await leaving })
    expect(useUiStore.getState().projectRoot).toBeNull()
    expect(saved).toEqual(choice === '保存' ? [{ root: '/project/A', content: 'unsaved' }] : [])
  })

  it('保存失败不离开，脏稿继续保留', async () => {
    useUiStore.setState({ projectRoot: '/project/A' })
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    vi.spyOn(api, 'writeRepoFile').mockRejectedValue(new Error('disk full'))
    ;({ el, root } = await render(<><FileEditor path="README.md" /><ConfirmHost /></>))
    await type(el!, 'unsaved')
    let leaving!: Promise<string>
    await act(async () => { leaving = api.closeProject().then(() => 'closed', () => 'cancelled') })
    const save = [...el!.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find(b => b.textContent === '保存')!
    await act(async () => { save.click(); await leaving })
    expect(useUiStore.getState().projectRoot).toBe('/project/A')
    expect(el!.querySelector('textarea')!.value).toBe('unsaved')
    expect(el!.querySelector('.dot.warn')).toBeTruthy()
  })

  it('保存等待期间新编辑不能被误清为已保存', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    let finish!: () => void
    vi.spyOn(api, 'writeRepoFile').mockImplementation(() => new Promise<void>(resolve => { finish = resolve }))
    ;({ el, root } = await render(<FileEditor path="README.md" />))
    await type(el!, 'first edit')
    const save = [...el!.querySelectorAll<HTMLButtonElement>('button')].find(b => b.title.includes('保存'))!
    await act(async () => save.click())
    await type(el!, 'newer edit')
    await act(async () => finish())
    expect(el!.querySelector('textarea')!.value).toBe('newer edit')
    expect(el!.querySelector('.dot.warn')).toBeTruthy()
  })

  it('迟到旧项目读取不能进入同路径的新项目缓冲', async () => {
    let finishOld!: (value: Awaited<ReturnType<typeof api.readRepoFileSnapshot>>) => void
    const read = vi.spyOn(api, 'readRepoFileSnapshot').mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve }))
      .mockResolvedValue({ path: 'README.md', project_root: '/new', generation: 2, content: 'new project' })
    ;({ el, root } = await render(<FileEditor path="README.md" />))
    await act(async () => useUiStore.setState({ projectRoot: '/new', projectEpoch: useUiStore.getState().projectEpoch + 1 }))
    await act(async () => finishOld({ path: 'README.md', project_root: '/test', generation: 1, content: 'old project' }))
    expect(read).toHaveBeenLastCalledWith('README.md', '/new')
    expect(el!.querySelector('textarea')!.value).toBe('new project')
  })

  it('第二次保存使用上次成功写入的原文证据，冲突失败保留脏稿', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    const write = vi.spyOn(api, 'writeRepoFile').mockResolvedValueOnce(undefined).mockRejectedValueOnce({code:'file_changed', message:'conflict'})
    ;({ el, root } = await render(<FileEditor path="README.md" />))
    const button = [...el!.querySelectorAll<HTMLButtonElement>('button')].find(b => b.title.includes('保存'))!
    await type(el!, 'first edit')
    await act(async () => button.click())
    await type(el!, 'second edit')
    await act(async () => button.click())
    expect(write.mock.calls[1]).toEqual([{ path: 'README.md', project_root: '/test', generation: 1, content: 'first edit' }, 'second edit'])
    expect(el!.querySelector('textarea')!.value).toBe('second edit')
    expect(el!.querySelector('.dot.warn')).toBeTruthy()
  })

  it('保存第二份文件期间重新修改第一份文件，不能丢稿离开', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    let finish!: () => void
    let secondReady!: () => void
    const ready = new Promise<void>(resolve => { secondReady = resolve })
    vi.spyOn(api, 'writeRepoFile').mockResolvedValueOnce(undefined).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; secondReady() }))
    ;({ el, root } = await render(<><FileEditor path="first.md" /><FileEditor path="second.md" /><ConfirmHost /></>))
    const editors = el!.querySelectorAll<HTMLElement>('[data-testid="file-editor"]')
    await type(editors[0], 'first draft')
    await type(editors[1], 'second draft')
    let leaving!: Promise<string>
    await act(async () => { leaving = api.closeProject().then(() => 'closed', () => 'cancelled') })
    const save = [...el!.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find(b => b.textContent === '保存')!
    await act(async () => { save.click(); await ready })
    await type(editors[0], 'newer first draft')
    await act(async () => { finish(); await leaving })
    expect(await leaving).toBe('cancelled')
    expect(useUiStore.getState().projectRoot).toBe('/test')
    expect(editors[0].querySelector('textarea')!.value).toBe('newer first draft')
    expect(editors[0].querySelector('.dot.warn')).toBeTruthy()
  })


  it('关闭脏文件页签沿用保存／放弃／取消保护', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    useUiStore.getState().openTab({id:'file:README.md',kind:'file',title:'README.md',path:'README.md'})
    ;({ el, root } = await render(<><FileEditor path="README.md" /><ConfirmHost /></>))
    await type(el!, 'unsaved')
    await act(async () => useUiStore.getState().closeTab('file:README.md'))
    expect(useUiStore.getState().tabs.some(tab=>tab.id==='file:README.md')).toBe(true)
    const dialog=el!.querySelector('[role="alertdialog"]')!
    expect(dialog).toBeTruthy()
    const cancel=[...dialog.querySelectorAll<HTMLButtonElement>('button')].find(button=>button.textContent==='取消')!
    await act(async () => cancel.click())
    expect(useUiStore.getState().tabs.some(tab=>tab.id==='file:README.md')).toBe(true)
    expect(el!.querySelector('textarea')!.value).toBe('unsaved')
  })

  it('原生模态在前景时Escape不能取消后台脏稿裁决', async () => {
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('original')
    useUiStore.getState().openTab({id:'file:README.md',kind:'file',title:'README.md',path:'README.md'})
    ;({ el, root } = await render(<><FileEditor path="README.md" /><ConfirmHost /></>))
    await type(el!, 'unsaved')
    await act(async () => useUiStore.getState().closeTab('file:README.md'))
    const native = document.createElement('dialog')
    native.setAttribute('open', '')
    el!.appendChild(native)
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})))
    expect(useUiStore.getState().confirmReq).not.toBeNull()
    expect(el!.querySelector('textarea')!.value).toBe('unsaved')
    native.remove()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})))
    expect(useUiStore.getState().confirmReq).toBeNull()
    expect(useUiStore.getState().tabs.some(tab=>tab.id==='file:README.md')).toBe(true)
  })

})
