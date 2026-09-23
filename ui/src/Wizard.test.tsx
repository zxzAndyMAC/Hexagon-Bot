import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { Wizard } from './components/Wizard'
import { api } from './api'
import type { DirReport } from './gen/DirReport'
import type { ProviderView } from './gen/ProviderView'
import type { ProvidersView } from './gen/ProvidersView'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const prevLang = i18n.language

function prov(over: Partial<ProviderView> & Pick<ProviderView, 'id' | 'name'>): ProviderView {
  return {
    kind: 'openai',
    base_url: 'https://example.test/v1',
    models: [],
    enabled: true,
    key_set: true,
    ...over,
  }
}

function report(over: Partial<DirReport>): DirReport {
  return {
    exists: true, empty: false, is_git: true, dirty: false, has_workbench: false, instructions: null,
    ...over,
  }
}

const readyDoc: ProvidersView = {
  providers: [prov({ id: 'or', name: 'OpenRouter' })],
  slots: { default: { provider_id: 'or', model: 'm1' } },
}

const draft = {
  dir: '/repo/demo', name: 'Demo', roles: [], roleOverrides: {},
  mode: 'pack', packName: '规格驱动', fastRole: '', initGit: false,
  genAgents: false, agentsMd: '',
}

let mounted: ReturnType<typeof createRoot> | null = null

async function renderWizard() {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  mounted = root
  await act(async () => {
    root.render(<Wizard onDone={() => {}} />)
    await new Promise((r) => setTimeout(r, 250))
  })
  return el
}

function nextBtn(el: HTMLElement) {
  return [...el.querySelectorAll('button')].find((b) => b.textContent === 'Next')!
}

function password(el: HTMLElement) {
  return el.querySelector('input[type="password"]')
}

async function unmount() {
  if (!mounted) return
  const root = mounted
  mounted = null
  await act(async () => { root.unmount() })
}

describe('向导先配模型服务商（票 13）', () => {
  beforeEach(async () => {
    localStorage.clear()
    await i18n.changeLanguage('en')
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
  })
  afterEach(async () => {
    await unmount()
    vi.restoreAllMocks()
    document.body.replaceChildren()
    await i18n.changeLanguage(prevLang)
  })

  it('打开向导时第一步是模型服务商，不是选目录', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    const el = await renderWizard()
    expect(el.textContent).toContain('1 · Model providers')
    expect(el.textContent).not.toContain('Project directory')
    expect(el.textContent).not.toContain('2 · Directory')
  })

  it('启用且有钥匙但没有 default 槽时不能继续，且不再要钥匙', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'or', name: 'OpenRouter' })],
      slots: {},
    })
    const el = await renderWizard()
    expect(el.textContent).toContain('1 · Model providers')
    expect(nextBtn(el).disabled).toBe(true)
    expect(password(el)).toBeNull()
    expect(el.textContent).toContain('default slot')
  })

  it('缺钥匙时不能继续，并给出钥匙输入', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'or', name: 'OpenRouter', key_set: false })],
      slots: { default: { provider_id: 'or', model: 'm1' } },
    })
    const el = await renderWizard()
    expect(nextBtn(el).disabled).toBe(true)
    expect(password(el)).toBeTruthy()
  })

  it('停用的供应商即使有钥匙和 default 槽也不能继续，且不再要钥匙', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'or', name: 'OpenRouter', enabled: false })],
      slots: { default: { provider_id: 'or', model: 'm1' } },
    })
    const el = await renderWizard()
    expect(nextBtn(el).disabled).toBe(true)
    expect(password(el)).toBeNull()
  })

  it('三项齐全时显示就绪、不渲染钥匙框，下一步进入目录', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
    const el = await renderWizard()
    expect(el.textContent).toContain('Ready')
    expect(el.textContent).toContain('OpenRouter')
    expect(password(el)).toBeNull()
    expect(nextBtn(el).disabled).toBe(false)
    await act(async () => {
      nextBtn(el).dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(el.textContent).toContain('2 · Directory')
    expect(el.textContent).toContain('Project directory')
  })

  it('表单控件走 .input 原语——.btn 只匹配 button，套在输入框上是死样式融进底色', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    const el = await renderWizard()
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    // 新建供应商表单：名称/类型/地址/密钥/模型
    expect(el.querySelectorAll('input.input, select.input')).toHaveLength(5)
  })

  it('已有钥匙时绑定 default 槽走现有 API，且不重传钥匙', async () => {
    const doc: ProvidersView = {
      providers: [prov({
        id: 'or', name: 'OpenRouter',
        models: [{ id: 'm1', name: null, group: null, caps: [], context_window: null, max_output: null }],
      })],
      slots: {},
    }
    vi.spyOn(api, 'listProviders').mockResolvedValue(doc)
    const save = vi.spyOn(api, 'saveProvider').mockResolvedValue(undefined)
    const bind = vi.spyOn(api, 'setSlotBinding').mockResolvedValue(undefined)
    const el = await renderWizard()
    expect(password(el)).toBeNull()
    const apply = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Save and bind default')!
    expect(apply.disabled).toBe(false)
    await act(async () => {
      apply.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ id: 'or', enabled: true }), undefined)
    expect(bind).toHaveBeenCalledWith('default', 'or', 'm1')
  })
})

describe('向导目录（ADR 0060）', () => {
  beforeEach(async () => {
    localStorage.clear()
    await i18n.changeLanguage('en')
    localStorage.setItem('hexagon.wizard', JSON.stringify(draft))
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
  })
  afterEach(async () => {
    await unmount()
    vi.restoreAllMocks()
    document.body.replaceChildren()
    localStorage.clear()
    await i18n.changeLanguage(prevLang)
  })

  async function onDirectory() {
    const el = await renderWizard()
    await act(async () => {
      nextBtn(el).dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    return el
  }

  it('脏树不停步，并说明改动会保留', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ dirty: true }))
    const el = await onDirectory()
    expect(el.textContent).toContain('Uncommitted changes stay as they are')
    expect(el.textContent).not.toContain('stops here')
    expect(nextBtn(el).disabled).toBe(false)
  })

  it('目录步输入框走 .input 原语', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({}))
    const el = await onDirectory()
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    expect(el.querySelectorAll('input.input')).toHaveLength(2) // 目录 + 项目名
  })

  it('已有工作台状态不能下一步新建，打开走 openRecent', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ has_workbench: true, dirty: true }))
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await onDirectory()
    expect(nextBtn(el).disabled).toBe(true)
    expect(el.textContent).toContain('open it instead of creating again')
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Open this project')!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(open).toHaveBeenCalledWith('/repo/demo')
    expect(create).not.toHaveBeenCalled()
  })
})

const MODEL_DRAFT = [
  '# Demo',
  '',
  '## 做什么',
  '一个本地待办清单。',
  '',
  '## Commands',
  '- Build:',
  '- Test:',
  '- Check:',
  '',
  '## Layout',
  '- 未知',
  '',
  '## Conventions',
  '-',
  '',
].join('\n')

function setTextArea(el: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
  setter.call(el, value)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}

async function advance(el: HTMLElement, title: string) {
  for (let i = 0; i < 8 && !el.textContent?.includes(title); i++) {
    const btn = nextBtn(el)
    if (btn.disabled) {
      await act(async () => { await new Promise((r) => setTimeout(r, 250)) })
      continue
    }
    await act(async () => {
      btn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
  }
  expect(el.textContent).toContain(title)
}

describe('一句话优化成项目说明（票 16）', () => {
  const seeded = {
    ...draft,
    roles: ['产品策划'],
    brief: '',
    genAgents: false,
    agentsMd: '',
  }

  beforeEach(async () => {
    localStorage.clear()
    await i18n.changeLanguage('en')
    localStorage.setItem('hexagon.wizard', JSON.stringify(seeded))
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([{
      origin: 'builtin',
      def: {
        name: '产品策划', duty: 'plan', reviewer: null,
        model_slot: 'default', globs: [], skills: [],
      },
    }])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ empty: true, is_git: true }))
  })
  afterEach(async () => {
    await unmount()
    vi.restoreAllMocks()
    document.body.replaceChildren()
    localStorage.clear()
    await i18n.changeLanguage(prevLang)
  })

  it('空目录优化后可改，确认才提交草稿，后退不创建', async () => {
    const optimize = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const skeleton = vi.spyOn(api, 'agentsMdDraft').mockResolvedValue('# SKELETON')
    const el = await renderWizard()
    await advance(el, '4 · Project brief')
    expect(skeleton).not.toHaveBeenCalled()
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    expect(optimizeBtn.disabled).toBe(true)
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    expect(optimizeBtn.disabled).toBe(false)
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(optimize).toHaveBeenCalledWith('Demo', '一个本地待办清单')
    expect(create).not.toHaveBeenCalled()
    const boxes = [...el.querySelectorAll('textarea')]
    expect(boxes).toHaveLength(2)
    expect(boxes[1].value).toBe(MODEL_DRAFT)
    expect(boxes[1].value).toContain('- Build:')
    expect(boxes[1].value).toContain('未知')
    expect(boxes[1].value).not.toMatch(/npm|cargo/)
    await act(async () => { setTextArea(boxes[1], `${MODEL_DRAFT}\n人手改过`) })
    const back = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    await act(async () => { back.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.textContent).toContain('3 · Roles')
    expect(create).not.toHaveBeenCalled()
    await advance(el, '7 · Launch')
    const launch = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Create project')!
    await act(async () => { launch.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(create).toHaveBeenCalledTimes(1)
    const md = create.mock.calls[0][0].agentsMd as string
    expect(md).toContain('人手改过')
    expect(md).toContain('- Build:')
    expect(md).toContain('未知')
    expect(md).not.toMatch(/npm|cargo/)
  })

  it('流程草稿可增删并改四项，检验命令、产物清单和回填边不出现', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...seeded, brief: '一个本地待办' }))
    vi.spyOn(api, 'draftFlow').mockResolvedValue({
      name: '从说明来',
      version: 1,
      knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{
        name: '规格',
        roles: ['产品策划'],
        due: ['规格'],
        checks: ['npm test'],
        reviews: [],
        stamp_point: true,
        backfill_edges: [['QA', '前端']],
        consult_wake: ['架构师'],
      }],
    })
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    expect(el.querySelector('[data-flow-draft]')).toBeTruthy()
    expect(el.textContent).not.toContain('npm test')
    expect(el.textContent).not.toContain('backfill')
    expect([...el.querySelectorAll('input, textarea')].every((n) => (n as HTMLInputElement).value !== 'npm test')).toBe(true)
    const name = el.querySelector('input[aria-label="Stage name"]') as HTMLInputElement
    const setVal = (input: HTMLInputElement, v: string) => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      set.call(input, v)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    }
    await act(async () => { setVal(name, '改过的阶段') })
    expect(name.value).toBe('改过的阶段')
    const stamp = el.querySelector('input[type="checkbox"]') as HTMLInputElement
    expect(stamp.checked).toBe(true)
    await act(async () => { stamp.click() })
    expect(stamp.checked).toBe(false)
    const add = el.querySelector('[data-add-stage]') as HTMLButtonElement
    await act(async () => { add.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.querySelectorAll('[data-stage-row]')).toHaveLength(2)
    const remove = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Remove stage')!
    await act(async () => { remove.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.querySelectorAll('[data-stage-row]')).toHaveLength(1)
    const up = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Move up')!
    const down = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Move down')!
    expect(up.disabled).toBe(true)
    expect(down.disabled).toBe(true)
  })

  it('说明步 textarea 走 .input 原语', async () => {
    const el = await renderWizard()
    await advance(el, '4 · Project brief')
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    expect(el.querySelectorAll('textarea.input').length).toBeGreaterThanOrEqual(1)
  })

  it('优化后离开向导不会创建项目', async () => {
    vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await renderWizard()
    await advance(el, '4 · Project brief')
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    await act(async () => { optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.textContent).toContain('未知')
    await unmount()
    expect(create).not.toHaveBeenCalled()
  })

  it.each(['AGENTS.md', 'CLAUDE.md'])('%s 已在时不提供优化，创建也不带草稿', async (file) => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...seeded,
      genAgents: true,
      agentsMd: '# SHOULD NOT LAND\n- Build: npm test\n',
      brief: 'overwrite me',
    }))
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ instructions: file, empty: false }))
    const optimize = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await renderWizard()
    await advance(el, '4 · Project brief')
    expect(el.textContent).toContain('Instructions file detected')
    expect(el.textContent).toContain(file)
    expect([...el.querySelectorAll('button')].some((b) => b.textContent === 'Optimize')).toBe(false)
    expect(el.textContent).not.toContain('SHOULD NOT LAND')
    expect(optimize).not.toHaveBeenCalled()
    await advance(el, '7 · Launch')
    const launch = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Create project')!
    await act(async () => { launch.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(create).toHaveBeenCalledTimes(1)
    expect(create.mock.calls[0][0].agentsMd).toBeNull()
  })
})
