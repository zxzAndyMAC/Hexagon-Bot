import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { Composer } from './components/Composer'
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
