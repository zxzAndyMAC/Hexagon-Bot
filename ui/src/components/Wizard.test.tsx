import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import '../i18n'
import i18n from '../i18n'
import { Wizard } from './Wizard'
import { api } from '../api'
import { CREATE_STEPS } from '../createProgress'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

function setValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
  setter.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

async function clickButton(el: HTMLElement, label: string) {
  const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)
  if (!btn) throw new Error(`missing button ${label}`)
  if ((btn as HTMLButtonElement).disabled) throw new Error(`disabled button ${label}`)
  await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
}

describe('向导创建进度（票 14）', () => {
  beforeEach(() => {
    localStorage.removeItem('hexagon.wizard')
    // 票 13 起向导第一步是模型服务商。进度测试要先过这步，目录输入才出现。
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [{
        id: 'or', name: 'OpenRouter', kind: 'openai',
        base_url: 'https://example.test/v1', models: [], enabled: true, key_set: true,
      }],
      slots: { default: { provider_id: 'or', model: 'm1' } },
    })
    return i18n.changeLanguage('zh-CN')
  })
  afterEach(() => vi.restoreAllMocks())

  async function reachConfirm(el: HTMLElement) {
    await act(async () => { await new Promise((r) => setTimeout(r, 50)) })
    await clickButton(el, '下一步')
    const fields = [...el.querySelectorAll('input')].filter((i) => i.type !== 'checkbox')
    await act(async () => {
      setValue(fields[0], '/tmp/hex-create')
      setValue(fields[1], '示例')
    })
    await act(async () => { await new Promise((r) => setTimeout(r, 250)) })
    await clickButton(el, '下一步')
    const box = el.querySelector('input[type=checkbox]') as HTMLInputElement
    await act(async () => { box.click() })
    await clickButton(el, '下一步')
    await clickButton(el, '下一步')
    await act(async () => { await new Promise((r) => setTimeout(r, 80)) })
    await clickButton(el, '下一步')
    await clickButton(el, '下一步')
  }

  it('步骤回报到达就可见；失败停在该步且不进入工作台；进行中仍可点上一步', async () => {
    let rejectCreate: (e: unknown) => void = () => {}
    vi.spyOn(api, 'createProject').mockImplementation(async (_opts, onStep) => {
      onStep?.('check_dir')
      onStep?.('git')
      onStep?.('persist_roles')
      await new Promise((_resolve, reject) => { rejectCreate = reject })
    })
    const onDone = vi.fn()
    const { el, root } = await render(<Wizard onDone={onDone} />)
    await reachConfirm(el)
    await clickButton(el, '创建项目')

    const step = (name: string) =>
      el.querySelector(`[data-create-step="${name}"]`) as HTMLElement
    expect(step('check_dir').dataset.state).toBe('done')
    expect(step('git').dataset.state).toBe('done')
    expect(step('persist_roles').dataset.state).toBe('done')
    expect(step('recheck_keys').dataset.state).toBe('running')
    expect(step('open_project').dataset.state).toBe('pending')
    const back = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === '上一步')!
    expect((back as HTMLButtonElement).disabled).toBe(false)
    expect(onDone).not.toHaveBeenCalled()

    const reason = '缺模型密钥，补齐或拿掉对应角色才能开跑: chat'
    await act(async () => { rejectCreate({ code: 'missing_keys', message: reason }) })
    expect(step('recheck_keys').dataset.state).toBe('failed')
    expect(step('recheck_keys').textContent).toContain(reason)
    expect(step('open_project').dataset.state).toBe('pending')
    expect(onDone).not.toHaveBeenCalled()
    await act(async () => { root.unmount() })
  })

  it('五步都完成后才进入工作台', async () => {
    vi.spyOn(api, 'createProject').mockImplementation(async (_opts, onStep) => {
      for (const step of CREATE_STEPS) onStep?.(step)
    })
    const onDone = vi.fn()
    const { el, root } = await render(<Wizard onDone={onDone} />)
    await reachConfirm(el)
    await clickButton(el, '创建项目')
    expect(onDone).toHaveBeenCalledTimes(1)
    await act(async () => { root.unmount() })
  })
})
