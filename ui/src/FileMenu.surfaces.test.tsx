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
import { MessageContextMenu } from './components/MessageMenu'
import { Timeline, ToolGroupRow } from './components/Timeline'
import type { TimelineItem } from './api'
import { ToolExecCard } from './components/ExecCard'
import { AgentTab } from './components/AgentTab'
import { StageArtifacts } from './components/PendingCardParts'
import { ArtifactTab } from './components/ArtifactTab'
import { FileEditor } from './components/FileEditor'
import { Composer } from './components/Composer'
import { Launcher } from './components/Launcher'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, scrollerRef }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode; scrollerRef: (node: HTMLElement | null) => void }) =>
    <div data-scroller ref={scrollerRef}>{data.map((row, index) => <div key={index}>{itemContent(index, row)}</div>)}</div>,
}))
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
    // 产物登记路径是 .hexagon/ 内相对，盘上路径 = .hexagon/<path>
    expect(api.openWithCandidates).toHaveBeenCalledWith('.hexagon/docs/plan.pdf')
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

describe('时间线消息里的 # 路径块与附件 chip（#32 表面 5、6）', () => {
  const msg = (
    <MessageContextMenu eventId={7} body="see #src/a.rs">
      <span data-file-path=".hexagon/inbox/shot.png"><span data-chip>shot.png</span></span>
      <button type="button" data-atom-path="src/a.rs">#src/a.rs</button>
      <button type="button" data-atom-path="docs/">#docs/</button>
      <p data-plain>plain text</p>
    </MessageContextMenu>
  )
  it('路径块上右键 = 文件菜单（含「打开」，不出消息项）；目录块无「打开」', async () => {
    const host = await render(msg)
    await rightClick(host.querySelector('[data-atom-path="src/a.rs"]')!)
    expect(items()).toEqual(FULL)
    await act(async () => { menu()!.querySelector<HTMLElement>('[data-ctx-item="open"]')!.click() })
    expect(useUiStore.getState().activeTab).toBe('file:src/a.rs')
    await rightClick(host.querySelector('[data-atom-path="docs/"]')!)
    expect(items()).toEqual(NO_OPEN)
  })
  it('附件 chip 上右键 = 无「打开」的文件菜单；正文空白处仍是消息菜单', async () => {
    const host = await render(msg)
    await rightClick(host.querySelector('[data-chip]')!)
    expect(items()).toEqual(NO_OPEN)
    expect(api.openWithCandidates).toHaveBeenCalledWith('.hexagon/inbox/shot.png')
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    await flush()
    await rightClick(host.querySelector('[data-plain]')!)
    expect(items()).toEqual(['copy', 'quote'])
  })
})

const ev = (id: number, kind: TimelineItem['event']['kind'], payload: Record<string, unknown>): TimelineItem => ({
  event: { id, project_id: 'p', kind, agent_id: 'a1', stage_run_id: null, payload, created_at: '2026-10-14T14:02:00Z' },
  message: null,
})

describe('交付卡 C11（#32 表面 3）', () => {
  it('右键 = 全套；「打开」= 点卡', async () => {
    useUiStore.setState({ pending: [], streams: {}, thinkings: {}, plans: {}, streamDone: {}, stages: [], projectRoot: null, projectEpoch: 0,
      timelineFacts: null, team: [], artifacts: [],
      timeline: [ev(1, 'artifact_delivered', { path: 'deck/q4.pptx', version: 1, kind: 'doc', status: 'pending' })] } as never)
    const host = await render(<Timeline />)
    // Timeline 挂载时 dev mock 可能并入别的交付卡，按路径挑
    await rightClick(host.querySelector('.tl-deliver[title="deck/q4.pptx"]')!)
    expect(items()).toEqual(FULL)
    expect(api.openWithCandidates).toHaveBeenCalledWith('.hexagon/deck/q4.pptx')
    await act(async () => { menu()!.querySelector<HTMLElement>('[data-ctx-item="open"]')!.click() })
    expect(useUiStore.getState().activeTab).toBe('art:deck/q4.pptx')
  })
})

describe('工具调用文件 chip（#32 表面 4）', () => {
  it('右键 = 全套；根内绝对路径按相对读候选，根外绝对路径只剩「打开」', async () => {
    const calls = [
      ev(1, 'tool_called', { tool: 'fs_read', input: { path: '/proj/docs/brief.md' } }),
      ev(2, 'tool_called', { tool: 'fs_read', input: { path: '/tmp/scratch.txt' } }),
    ]
    const host = await render(<ToolGroupRow items={calls} expanded idx={0} onToggle={() => {}} />)
    const chips = host.querySelectorAll('.tchip-files .chip-btn')
    await rightClick(chips[0])
    expect(items()).toEqual(FULL)
    expect(api.openWithCandidates).toHaveBeenCalledWith('docs/brief.md')
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    await flush()
    await rightClick(chips[1])
    expect(items()).toEqual(['open'])
  })
})

describe('执行卡卡头路径（#32 表面 7）', () => {
  const call = (tool: string, input: Record<string, unknown>) => ({
    called: ev(1, 'tool_called', { tool, input, seq: 'r1:i0' }),
    result: ev(2, 'tool_result', { tool, ok: true, result: { output: {} } }),
  })
  it('fs_patch：卡头右键 = 全套，「打开」开文件页签；bash 卡不挂文件菜单', async () => {
    const host = await render(<><ToolExecCard call={call('fs_patch', { path: '/proj/src/a.ts', old: 'a', new: 'b' })} />
      <div data-bash><ToolExecCard call={call('bash', { cmd: 'ls' })} /></div></>)
    await rightClick(host.querySelector('.exec-head')!)
    expect(items()).toEqual(FULL)
    await act(async () => { menu()!.querySelector<HTMLElement>('[data-ctx-item="open"]')!.click() })
    expect(useUiStore.getState().activeTab).toBe('file:src/a.ts')
    await flush()
    await rightClick(host.querySelector('[data-bash] .exec-head')!)
    expect(menu()).toBeNull()
  })
})

describe('Agent 详情步骤路径链接（#32 表面 8）', () => {
  it('右键 = 全套；「打开」开产物页签', async () => {
    // 步骤来自分页窗口（dev mock 的 Agent 活动），挑第一条带路径的步骤
    useUiStore.setState({
      team: [{ id: 'a1', role: 'dev', model_slot: null, status: 'active', avatar_hash: null, avatar_override: false, model: null, duty: null, superior: null }] as never,
    })
    const host = await render(<AgentTab agentId="a1" />)
    await flush()
    for (const g of host.querySelectorAll<HTMLElement>('[aria-expanded="false"]')) await act(async () => { g.click() })
    const link = host.querySelector<HTMLElement>('[data-step-path]')!
    expect(link).not.toBeNull()
    await rightClick(link)
    expect(items()).toEqual(FULL)
    await act(async () => { menu()!.querySelector<HTMLElement>('[data-ctx-item="open"]')!.click() })
    expect(useUiStore.getState().activeTab).toBe(`art:${link.title}`)
  })
})

describe('盖章卡阶段产物证据行（#32 表面 9）', () => {
  it('只有文件项（无「打开」、无任何裁决项）；待恢复行不挂菜单', async () => {
    useUiStore.setState({ artifacts: [artifact('out/plan.xlsx', { stage_run_id: 'r1' }),
      Object.assign(artifact('out/b.md', { stage_run_id: 'r1' }), { materialization: 'pending_recovery' })] })
    const host = await render(<StageArtifacts runId="r1" />)
    const rows = host.querySelectorAll('[data-stage-artifact]')
    await rightClick(rows[0])
    expect(items()).toEqual(NO_OPEN)
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    await flush()
    await rightClick(rows[1])
    expect(menu()).toBeNull()
  })
})

describe('产物页子头路径 / 文件页面包屑（#32 表面 10）', () => {
  it('产物页子头：无「打开」的文件菜单，按 .hexagon/ 盘上路径', async () => {
    useUiStore.setState({ artifacts: [artifact('notes/brief.pdf')] })
    vi.spyOn(api, 'proposals').mockResolvedValue([])
    vi.spyOn(api, 'artifactContent').mockResolvedValue('x')
    vi.spyOn(api, 'artifactContentAt').mockResolvedValue('x')
    const host = await render(<ArtifactTab path="notes/brief.pdf" />)
    await rightClick(host.querySelector('[data-art-header-path]')!)
    expect(items()).toEqual(NO_OPEN)
    expect(api.openWithCandidates).toHaveBeenCalledWith('.hexagon/notes/brief.pdf')
  })
  it('文件页面包屑：无「打开」的文件菜单', async () => {
    useUiStore.setState({ fileEdits: {} })
    vi.spyOn(api, 'readRepoFile').mockResolvedValue('const a = 1\n')
    vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
    vi.spyOn(api, 'readRepoFileSnapshot').mockImplementation(async (path, project_root) => ({ path, project_root, generation: 1, content: 'const a = 1\n' }))
    const host = await render(<FileEditor path="src/a.ts" />)
    await flush()
    await rightClick(host.querySelector('nav[aria-label="src/a.ts"]')!)
    expect(items()).toEqual(NO_OPEN)
    expect(api.openWithCandidates).toHaveBeenCalledWith('src/a.ts')
  })
})

describe('输入条附件 chip（#32 表面 11）', () => {
  it('右键 = 复制路径 · 在访达中显示 · 打开方式（无「打开」「插入」）', async () => {
    useUiStore.setState({ mode: 'fastpath', fastRole: 'ops', team: [], pending: [], timeline: [] } as never)
    vi.spyOn(api, 'stageAttachment').mockResolvedValue({ media_type: 'image/png', path: '.hexagon/inbox/att-1-x.png', bytes: 11, name: 'shot.png' })
    const host = await render(<Composer />)
    const ta = host.querySelector('textarea')!
    const e = new Event('paste', { bubbles: true }) as ClipboardEvent
    const png = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1])], 'shot.png', { type: 'image/png' })
    Object.defineProperty(e, 'clipboardData', { value: { files: [png] } })
    await act(async () => { ta.dispatchEvent(e) })
    await flush()
    await rightClick(host.querySelector('[data-attach-path]')!)
    expect(items()).toEqual(['copyPath', 'reveal', 'openWith'])
    expect(api.openWithCandidates).toHaveBeenCalledWith('.hexagon/inbox/att-1-x.png')
  })
})

describe('启动页最近项目（#32 表面 12）', () => {
  it('右键 = 打开 · 打开方式（项目级，走 recent_* 命令）；目录已删的行不挂菜单；选中应用即启动', async () => {
    vi.spyOn(api, 'recentProjects').mockResolvedValue([
      { dir: '/tmp/ok-proj', name: 'ok-proj', mode: 'pack', opened_at: 1, exists: true },
      { dir: '/gone/dead', name: 'dead', mode: 'fastpath', opened_at: 0, exists: false },
    ])
    const cands = vi.spyOn(api, 'recentOpenWithCandidates').mockResolvedValue([{ id: 'dev.zed.Zed', name: 'Zed', icon: null, is_default: false }])
    const launch = vi.spyOn(api, 'recentOpenWith').mockResolvedValue(undefined)
    const host = await render(<Launcher onOpen={() => {}} />)
    await flush()
    const row = (name: string) => [...host.querySelectorAll<HTMLElement>('.recent-row')].find((n) => n.textContent?.includes(name))!
    await rightClick(row('ok-proj'))
    expect(items()).toEqual(['open', 'openWith'])
    expect(cands).toHaveBeenCalledWith('/tmp/ok-proj')
    expect(api.openWithCandidates).not.toHaveBeenCalled()
    const trig = menu()!.querySelector<HTMLElement>('[data-ctx-item="openWith"]')!
    act(() => trig.focus())
    await act(async () => { trig.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })) })
    await flush()
    await act(async () => { document.querySelector<HTMLElement>('[data-open-with-app="dev.zed.Zed"]')!.click() })
    expect(launch).toHaveBeenCalledWith('/tmp/ok-proj', 'dev.zed.Zed')
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    await flush()
    await rightClick(row('dead'))
    expect(menu()).toBeNull()
  })
})

describe('打开方式子菜单 · 系统默认标记（#32 文件类型口径）', () => {
  it('is_default 的行名后显示「(default)」，其余行不显示', async () => {
    vi.mocked(api.openWithCandidates).mockResolvedValue([
      { id: 'com.apple.Preview', name: 'Preview', icon: null, is_default: true },
      { id: 'com.google.Chrome', name: 'Google Chrome', icon: null, is_default: false },
    ])
    useUiStore.setState({ artifacts: [artifact('docs/plan.pdf')] })
    const host = await render(<ArtifactList />)
    await rightClick(host.querySelector('[role=listitem]')!)
    const trig = menu()!.querySelector<HTMLElement>('[data-ctx-item="openWith"]')!
    act(() => trig.focus())
    await act(async () => { trig.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })) })
    await flush()
    const marks = [...document.querySelectorAll<HTMLElement>('[data-open-with-default]')]
    expect(marks.map((m) => m.closest('[data-open-with-app]')!.getAttribute('data-open-with-app'))).toEqual(['com.apple.Preview'])
    expect(marks[0].textContent).toBe('(default)')
  })
})
