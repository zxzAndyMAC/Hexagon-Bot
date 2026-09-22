import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { Composer } from './components/Composer'
import { atomicDeletion, findAtoms } from './composerAtoms'
import { useUiStore } from './store'
import { api } from './api'
import type { TeamRow } from './gen/TeamRow'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const member = (role: string, status: 'active' | 'sleeping' = 'active'): TeamRow =>
  ({ id: `id-${role}`, role, model_slot: 'chat', status, avatar_hash: null })

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

async function sendText(el: HTMLElement, body: string) {
  const ta = el.querySelector('textarea')!
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
    setter.call(ta, body)
    ta.dispatchEvent(new Event('input', { bubbles: true }))
  })
  // Enter=发送（placeholder 承诺的键位）；Shift+Enter 才是换行
  await act(async () => {
    ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  })
}

// 回归（真窗口活测 D-06）：pack 模式此前无任何触发 agent 回合的
// UI 路径——send 只落库，dispatch 只在 fastpath 调。「@点名即派活」
// 补上最后一寸；无 mention 的广播消息不触发回合（受控：不烧 token）。
describe('Composer pack 模式 @点名派活（D-06）', () => {
  beforeEach(() => {
    useUiStore.setState({
      mode: 'pack',
      fastRole: null,
      team: [member('产品策划'), member('后端'), member('UX', 'sleeping')],
      pending: [],
      timeline: [],
    })
  })
  afterEach(() => vi.restoreAllMocks())

  it('@花名册角色 → dispatch 到该角色；一条消息可点多人', async () => {
    const sendSpy = vi.spyOn(api, 'sendMessage')
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await sendText(el, '@产品策划 @后端 把首页骨架搭出来')
    expect(sendSpy).toHaveBeenCalledWith('@产品策划 @后端 把首页骨架搭出来', [])
    expect(dispSpy).toHaveBeenCalledTimes(2)
    expect(dispSpy).toHaveBeenCalledWith('产品策划', '@产品策划 @后端 把首页骨架搭出来', [])
    expect(dispSpy).toHaveBeenCalledWith('后端', '@产品策划 @后端 把首页骨架搭出来', [])
    root.unmount()
  })

  it('同一角色重复 @ 只派一次', async () => {
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await sendText(el, '@产品策划 写范围 @产品策划 别忘了验收标准')
    expect(dispSpy).toHaveBeenCalledTimes(1)
    root.unmount()
  })

  it('无 mention 的消息只落库不派活（广播/steering 不烧 token）', async () => {
    const sendSpy = vi.spyOn(api, 'sendMessage')
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await sendText(el, '先别动，等我确认范围')
    expect(sendSpy).toHaveBeenCalledTimes(1)
    expect(dispSpy).not.toHaveBeenCalled()
    root.unmount()
  })

  it('@ 非花名册名字不派活（拼错的角色名不误触回合）', async () => {
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await sendText(el, '@不存在的人 干活')
    expect(dispSpy).not.toHaveBeenCalled()
    root.unmount()
  })

  it('fastpath 语义不变：消息直接派给通道角色', async () => {
    useUiStore.setState({ mode: 'fastpath', fastRole: '运维' })
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await sendText(el, '准备部署清单')
    expect(dispSpy).toHaveBeenCalledTimes(1)
    expect(dispSpy).toHaveBeenCalledWith('运维', '准备部署清单', [])
    root.unmount()
  })
})

// 票 03：图片附件——粘贴/拖拽 → chip → send/dispatch 带 AttachRef
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3])
const imgFile = (name = 'shot.png', size?: number) => {
  const f = new File([size ? new Uint8Array(size) : PNG], name, { type: 'image/png' })
  return f
}

function pasteEvent(el: HTMLElement, files: File[]) {
  const ta = el.querySelector('textarea')!
  const e = new Event('paste', { bubbles: true }) as ClipboardEvent
  Object.defineProperty(e, 'clipboardData', { value: { files } })
  return act(async () => { ta.dispatchEvent(e) })
}

describe('Composer 图片附件（票 03）', () => {
  beforeEach(() => {
    useUiStore.setState({
      mode: 'fastpath', fastRole: '运维',
      team: [member('运维')], pending: [], timeline: [],
    })
  })
  afterEach(() => vi.restoreAllMocks())

  it('粘贴图片 → stage + chip 渲染 → send 带 AttachRef', async () => {
    const stageSpy = vi.spyOn(api, 'stageAttachment').mockResolvedValue({
      media_type: 'image/png', path: '.hexagon/inbox/att-1-x.png', bytes: 11, name: 'shot.png',
    })
    const sendSpy = vi.spyOn(api, 'sendMessage')
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await pasteEvent(el, [imgFile()])
    expect(stageSpy).toHaveBeenCalledWith('shot.png', expect.any(Uint8Array))
    expect(el.querySelector('.attach-chip')).toBeTruthy()
    await sendText(el, '看这个截图')
    expect(sendSpy).toHaveBeenCalledWith('看这个截图', [
      expect.objectContaining({ path: '.hexagon/inbox/att-1-x.png' }),
    ])
    expect(dispSpy).toHaveBeenCalledWith('运维', '看这个截图', [
      expect.objectContaining({ media_type: 'image/png' }),
    ])
    root.unmount()
  })

  it('chip 移除 → discard_attachments + 不随消息发送', async () => {
    vi.spyOn(api, 'stageAttachment').mockResolvedValue({
      media_type: 'image/png', path: '.hexagon/inbox/att-2-x.png', bytes: 11, name: 'shot.png',
    })
    const discardSpy = vi.spyOn(api, 'discardAttachments')
    const sendSpy = vi.spyOn(api, 'sendMessage')
    const { el, root } = await render(<Composer />)
    await pasteEvent(el, [imgFile()])
    expect(el.querySelector('.attach-chip')).toBeTruthy()
    await act(async () => {
      el.querySelector<HTMLButtonElement>('.attach-remove')!.click()
    })
    expect(discardSpy).toHaveBeenCalledWith([
      expect.objectContaining({ path: '.hexagon/inbox/att-2-x.png' }),
    ])
    expect(el.querySelector('.attach-chip')).toBeFalsy()
    await sendText(el, '不带图')
    expect(sendSpy).toHaveBeenCalledWith('不带图', [])
    root.unmount()
  })

  it('超限前端拦截：>5MB / 非图片 / 超 4 图 不触 IPC', async () => {
    const stageSpy = vi.spyOn(api, 'stageAttachment').mockResolvedValue({
      media_type: 'image/png', path: '.hexagon/inbox/a.png', bytes: 11, name: 'x.png',
    })
    const toastSpy = vi.fn()
    useUiStore.setState({ pushToast: toastSpy })
    const { el, root } = await render(<Composer />)
    // 非图片
    const txt = new File(['hi'], 'a.txt', { type: 'text/plain' })
    await pasteEvent(el, [txt])
    expect(stageSpy).not.toHaveBeenCalled()
    expect(toastSpy).toHaveBeenCalled()
    // >5MB
    await pasteEvent(el, [imgFile('big.png', 5 * 1024 * 1024 + 1)])
    expect(stageSpy).not.toHaveBeenCalled()
    // 满 4 图后再来一张
    for (let i = 0; i < 4; i++) await pasteEvent(el, [imgFile(`s${i}.png`)])
    expect(stageSpy).toHaveBeenCalledTimes(4)
    await pasteEvent(el, [imgFile('one-more.png')])
    expect(stageSpy).toHaveBeenCalledTimes(4)
    root.unmount()
  })
})

// 票 10：点名 / 路径是原子块。正文仍是 @角色 / #路径（派活口径不变）；
// 空白收束后退格或删除一次去掉整块，不留残字。目录只是一条路径。
const ROLES = new Set(['产品策划', '后端', 'UX'])

async function setComposer(el: HTMLElement, body: string) {
  const ta = el.querySelector('textarea')!
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
    setter.call(ta, body)
    ta.dispatchEvent(new Event('input', { bubbles: true }))
  })
  return ta
}

async function keyAt(ta: HTMLTextAreaElement, key: string, at: number) {
  await act(async () => {
    ta.setSelectionRange(at, at)
    ta.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
  })
}

function clickLabel(el: HTMLElement, label: string) {
  const node = [...el.querySelectorAll('.mono')].find((n) => n.textContent?.trim() === label)
  if (!node?.parentElement) throw new Error(`no popup row ${label}`)
  return act(async () => { node.parentElement!.click() })
}

describe('composerAtoms（票 10）', () => {
  it('空白收束的点名：退格一次不留残字，连同尾随空白', () => {
    const text = '@产品策划 '
    const atoms = findAtoms(text, ROLES)
    expect(atoms.map((a) => a.value)).toEqual(['产品策划'])
    expect(atomicDeletion(text, text.length, text.length, 'Backspace', atoms)).toEqual({ text: '', caret: 0 })
  })

  it('未收束的路径还在打，不升成块', () => {
    expect(findAtoms('#src/main.rs', ROLES)).toEqual([])
    expect(atomicDeletion('#src/main.rs', 12, 12, 'Backspace', [])).toBeNull()
  })

  it('目录是一条路径，值保留尾 /', () => {
    const atoms = findAtoms('看 #src/ ', ROLES)
    expect(atoms).toEqual([{ kind: 'path', value: 'src/', start: 2, end: 7 }])
  })

  it('光标在块中间或只选中残段，删除扩成整块', () => {
    const text = '@产品策划 你好'
    const atoms = findAtoms(text, ROLES)
    expect(atomicDeletion(text, 3, 3, 'Backspace', atoms)?.text).toBe('你好')
    expect(atomicDeletion(text, 3, 5, 'Backspace', atoms)?.text).toBe('你好')
    expect(atomicDeletion(text, 0, 0, 'Delete', atoms)?.text).toBe('你好')
  })

  it('连续两块只删光标打中的那一块', () => {
    const text = '@产品策划 @后端 '
    const atoms = findAtoms(text, ROLES)
    expect(atomicDeletion(text, text.length, text.length, 'Backspace', atoms)?.text).toBe('@产品策划 ')
  })

  it('非花名册 @ 不是点名块', () => {
    expect(findAtoms('@陌生人 ', ROLES)).toEqual([])
  })
})

describe('Composer 原子块（票 10）', () => {
  beforeEach(() => {
    useUiStore.setState({
      mode: 'pack',
      fastRole: null,
      team: [member('产品策划'), member('后端'), member('UX', 'sleeping')],
      pending: [],
      timeline: [],
    })
  })
  afterEach(() => vi.restoreAllMocks())

  it('点名一个角色后，退格一次删除整个点名，不留残字', async () => {
    const { el, root } = await render(<Composer />)
    await setComposer(el, '@产品')
    await clickLabel(el, '@产品策划')
    const ta = el.querySelector('textarea')!
    expect(ta.value).toBe('@产品策划 ')
    expect(el.querySelector('.atom-token')?.getAttribute('data-kind')).toBe('mention')
    expect(el.querySelector('.atom-token')?.getAttribute('data-value')).toBe('产品策划')
    await keyAt(ta, 'Backspace', ta.value.length)
    expect(ta.value).toBe('')
    expect(ta.value).not.toMatch(/产|策/)
    root.unmount()
  })

  it('手打路径与弹层路径是同一种 path 块；退格一次删整段', async () => {
    vi.useFakeTimers()
    vi.spyOn(api, 'repoPaths').mockResolvedValue(['src/', 'src/main.rs', 'src/lib.rs'])
    const { el, root } = await render(<Composer />)
    try {
    await setComposer(el, '看 #ui/App.tsx ')
    const typed = el.querySelector('.atom-token')
    expect(typed?.getAttribute('data-kind')).toBe('path')
    expect(typed?.getAttribute('data-value')).toBe('ui/App.tsx')
    expect(typed?.className).toBe('atom-token')

    await setComposer(el, '看 #ui/App.tsx #sr')
    await act(async () => { await vi.advanceTimersByTimeAsync(200) })
    await clickLabel(el, '#src/main.rs')
    const atoms = [...el.querySelectorAll('.atom-token')]
    expect(atoms.map((n) => n.getAttribute('data-kind'))).toEqual(['path', 'path'])
    expect(new Set(atoms.map((n) => n.className))).toEqual(new Set(['atom-token']))
    expect(atoms.map((n) => n.getAttribute('data-value'))).toEqual(['ui/App.tsx', 'src/main.rs'])

    const ta = el.querySelector('textarea')!
    await keyAt(ta, 'Backspace', ta.value.length)
    expect(ta.value).toBe('看 #ui/App.tsx ')
    expect(ta.value).not.toContain('main.rs')
    expect(el.querySelector('.atom-token')?.getAttribute('data-value')).toBe('ui/App.tsx')
    } finally {
      vi.useRealTimers()
      root.unmount()
    }
  })

  it('目录块不把子文件展进输入框，退格一次删掉这条路径', async () => {
    vi.useFakeTimers()
    vi.spyOn(api, 'repoPaths').mockResolvedValue(['src/', 'src/main.rs', 'src/lib.rs'])
    const { el, root } = await render(<Composer />)
    try {
    await setComposer(el, '#src')
    await act(async () => { await vi.advanceTimersByTimeAsync(200) })
    expect(el.textContent).toContain('#src/main.rs') // 弹层可以列出子文件
    await clickLabel(el, '#src/')
    const ta = el.querySelector('textarea')!
    expect(ta.value).toBe('#src/ ')
    expect(ta.value).not.toContain('main.rs')
    expect(ta.value).not.toContain('lib.rs')
    const tokens = [...el.querySelectorAll('.atom-token')]
    expect(tokens).toHaveLength(1)
    expect(tokens[0].getAttribute('data-kind')).toBe('path')
    expect(tokens[0].getAttribute('data-value')).toBe('src/')
    await keyAt(ta, 'Delete', 0)
    expect(ta.value).toBe('')
    } finally {
      vi.useRealTimers()
      root.unmount()
    }
  })

  it('点名块发送后派活口径不变：正文仍是 @角色', async () => {
    const sendSpy = vi.spyOn(api, 'sendMessage')
    const dispSpy = vi.spyOn(api, 'dispatch')
    const { el, root } = await render(<Composer />)
    await setComposer(el, '@产品')
    await clickLabel(el, '@产品策划')
    const ta = el.querySelector('textarea')!
    await act(async () => {
      ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    })
    expect(sendSpy).toHaveBeenCalledWith('@产品策划 ', [])
    expect(dispSpy).toHaveBeenCalledTimes(1)
    expect(dispSpy).toHaveBeenCalledWith('产品策划', '@产品策划 ', [])
    root.unmount()
  })
})

describe('Composer 图片放大（票 10）', () => {
  beforeEach(() => {
    useUiStore.setState({
      mode: 'fastpath', fastRole: '运维',
      team: [member('运维')], pending: [], timeline: [],
    })
  })
  afterEach(() => vi.restoreAllMocks())

  it('缩略图可放大，关闭预览回到输入框且不移除图片', async () => {
    vi.spyOn(api, 'stageAttachment').mockResolvedValue({
      media_type: 'image/png', path: '.hexagon/inbox/att-z.png', bytes: 11, name: 'shot.png',
    })
    const discardSpy = vi.spyOn(api, 'discardAttachments')
    const { el, root } = await render(<Composer />)
    await pasteEvent(el, [imgFile()])
    const thumb = el.querySelector('img')
    expect(thumb).toBeTruthy()
    await act(async () => { el.querySelector<HTMLButtonElement>('.attach-thumb')!.click() })
    const zoom = el.querySelector('.attach-zoom')
    expect(zoom).toBeTruthy()
    expect(zoom?.querySelector('img')?.getAttribute('alt')).toBe('shot.png')
    await act(async () => { el.querySelector<HTMLButtonElement>('.attach-zoom-close')!.click() })
    expect(el.querySelector('.attach-zoom')).toBeNull()
    expect(el.querySelector('.attach-chip')).toBeTruthy()
    expect(discardSpy).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(el.querySelector('textarea'))
    root.unmount()
  })

  it('Esc 关闭放大也回到输入框', async () => {
    vi.spyOn(api, 'stageAttachment').mockResolvedValue({
      media_type: 'image/png', path: '.hexagon/inbox/att-z2.png', bytes: 11, name: 'shot.png',
    })
    const { el, root } = await render(<Composer />)
    await pasteEvent(el, [imgFile()])
    await act(async () => { el.querySelector<HTMLButtonElement>('.attach-thumb')!.click() })
    expect(el.querySelector('.attach-zoom')).toBeTruthy()
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    })
    expect(el.querySelector('.attach-zoom')).toBeNull()
    expect(document.activeElement).toBe(el.querySelector('textarea'))
    root.unmount()
  })

  it('关闭钮只去掉这一张', async () => {
    vi.spyOn(api, 'stageAttachment').mockImplementation(async (name: string) => ({
      media_type: 'image/png', path: `.hexagon/inbox/${name}`, bytes: 11, name,
    }))
    const discardSpy = vi.spyOn(api, 'discardAttachments')
    const { el, root } = await render(<Composer />)
    await pasteEvent(el, [imgFile('a.png')])
    await pasteEvent(el, [imgFile('b.png')])
    expect(el.querySelectorAll('.attach-chip')).toHaveLength(2)
    await act(async () => { el.querySelectorAll<HTMLButtonElement>('.attach-remove')[0].click() })
    expect(el.querySelector('.attach-zoom')).toBeNull()
    expect(el.querySelectorAll('.attach-chip')).toHaveLength(1)
    expect(el.querySelector('.attach-thumb img')?.getAttribute('alt')).toBe('b.png')
    expect(discardSpy).toHaveBeenCalledTimes(1)
    expect(discardSpy).toHaveBeenCalledWith([
      expect.objectContaining({ path: '.hexagon/inbox/a.png', name: 'a.png' }),
    ])
    root.unmount()
  })
})
