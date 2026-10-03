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
    // 重排（2026-09-25）：brief=3 在 roles=4 前。dev mock 目录带 AGENTS.md，
    // brief 步是检出分支（无 checkbox）；卸角色挪到 roles 步。
    await clickButton(el, '下一步')
    const box = el.querySelector('input[type=checkbox]') as HTMLInputElement
    await act(async () => { box.click() })
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
  it('只提交负责人在可见质量命令表单填写并确认的命令', async () => {
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const { el, root } = await render(<Wizard onDone={vi.fn()} />)
    await reachConfirm(el)
    await clickButton(el, '上一步')
    await clickButton(el, '上一步')
    // 2026-10-01 live acceptance: PackEditor was intentionally removed by
    // ADR 0069; an unreachable editor cannot serve as runner approval.
    const field = el.querySelector<HTMLInputElement>('[data-quality-command="tests"]')
    expect(field).not.toBeNull()
    await act(async () => { setValue(field!, 'npm test') })
    await clickButton(el, '下一步')
    await clickButton(el, '下一步')
    expect(el.textContent).toContain('npm test')
    await clickButton(el, '创建项目')
    const stages = create.mock.calls[0][0].pack!.stages
    expect(stages.at(-1)?.quality_checks).toEqual({ tests: 'npm test' })
    expect(stages.every(stage => stage.checks.length === 0)).toBe(true)
    expect(stages.slice(0, -1).every(stage => Object.keys(stage.quality_checks).length === 0)).toBe(true)
    await act(async () => root.unmount())
  })

  it('恢复的模型流程草稿不能把隐藏命令当作负责人授权提交', async () => {
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const draftFlow = vi.spyOn(api, 'draftFlow').mockImplementation(async (_brief, roles) => ({
      name: 'Hidden execution draft', version: 1, knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{ name: 'Build', roles: roles.filter((r) => r !== '项目经理'), due: [],
        checks: ['printf hidden > marker'], quality_checks: { tests: 'printf hidden > marker' },
        reviews: [], stamp_point: true, backfill_edges: [], consult_wake: [] }],
    }))
    const initial = await render(<Wizard onDone={vi.fn()} />)
    await reachConfirm(initial.el)
    // Ticket 04 review: persisted pre-fix drafts can contain commands never shown
    // by the stage editor. Recovery must not silently approve those commands.
    expect(JSON.parse(localStorage.getItem('hexagon.wizard')!).flowPack.stages[0].checks).toHaveLength(1)
    await act(async () => { initial.root.unmount() })
    const callsBefore = draftFlow.mock.calls.length
    const restored = await render(<Wizard onDone={vi.fn()} />)
    await act(async () => { await new Promise((r) => setTimeout(r, 250)) })
    for (let i = 0; i < 6; i++) {
      await clickButton(restored.el, '下一步')
      await act(async () => { await new Promise((r) => setTimeout(r, 100)) })
    }
    expect(draftFlow.mock.calls.length).toBe(callsBefore)
    await clickButton(restored.el, '创建项目')
    expect(create.mock.calls[0][0].pack?.stages[0].checks).toEqual([])
    expect(create.mock.calls[0][0].pack?.stages[0].quality_checks).toEqual({})
    expect(create.mock.calls[0][0].pack?.stages[0].name).toBe('Build')
    await act(async () => { restored.root.unmount() })
  })

  it('负责人手填命令不从旧向导存档恢复授权', async () => {
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const initial = await render(<Wizard onDone={vi.fn()} />)
    await reachConfirm(initial.el)
    await clickButton(initial.el, '上一步')
    await clickButton(initial.el, '上一步')
    await act(async () => setValue(initial.el.querySelector<HTMLInputElement>('[data-quality-command="tests"]')!, 'node owner-approved-tests.mjs'))
    expect(localStorage.getItem('hexagon.wizard')).not.toContain('owner-approved-tests')
    await act(async () => initial.root.unmount())
    const restored = await render(<Wizard onDone={vi.fn()} />)
    await act(async () => { await new Promise(r => setTimeout(r, 250)) })
    for (let i = 0; i < 6; i++) {
      await clickButton(restored.el, '下一步')
      await act(async () => { await new Promise(r => setTimeout(r, 100)) })
    }
    expect(restored.el.textContent).not.toContain('owner-approved-tests')
    await clickButton(restored.el, '创建项目')
    expect(create.mock.calls[0][0].pack?.stages.every(stage => Object.keys(stage.quality_checks).length === 0)).toBe(true)
    await act(async () => restored.root.unmount())
  })

})
