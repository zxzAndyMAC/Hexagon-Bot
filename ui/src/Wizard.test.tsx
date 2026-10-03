import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { Wizard } from './components/Wizard'
import { api } from './api'
import type { DirReport } from './gen/DirReport'
import type { PackDef } from './gen/PackDef'
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
  // brief 步提到角色前（2026-09-25）：存档草稿自带一句话，角色步直接过 canNext
  brief: 'demo brief',
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
    // 答问流（2026-09-25）：出题在前，答齐敲定才起草；跳过讨论直出。
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
      { question: '给谁用？', options: ['家人'] },
    ])
    let resolveDraft: (v: string) => void = () => {}
    const optimize = vi.spyOn(api, 'optimizeAgentsMd').mockImplementation(
      () => new Promise((r) => { resolveDraft = r }),
    )
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const skeleton = vi.spyOn(api, 'agentsMdDraft').mockResolvedValue('# SKELETON')
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    expect(skeleton).not.toHaveBeenCalled()
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    expect(optimizeBtn.disabled).toBe(true)
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    expect(optimizeBtn.disabled).toBe(false)
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    // 答问卡出现：选项敲定前不放行
    const qaPanel = el.querySelector('[data-qa-cards]')
    expect(qaPanel).toBeTruthy()
    // 右侧并列弹层（同定制弹窗对位），不再是滚动体内联卡
    expect(qaPanel!.getAttribute('role')).toBe('dialog')
    expect(qaPanel!.previousElementSibling?.textContent).toContain('Project wizard')
    const gen = () =>
      [...el.querySelectorAll('button')].find((b) => b.textContent === 'Draft with these answers')!
    const back = () =>
      [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    expect(gen().disabled).toBe(true)
    const cards = [...el.querySelectorAll('[data-qa-cards] .card-ask')]
    expect(cards).toHaveLength(2)
    // 进度计数（题数不设上限，进度是完成度信号）：答一题翻一格
    const progress = () => el.querySelector('[data-qa-progress]')!.textContent
    expect(progress()).toBe('0 of 2 answered')
    // 第一题点选项，第二题「其他」手填
    const opt = cards[0].querySelector('button')!
    await act(async () => { opt.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(progress()).toBe('1 of 2 answered')
    const other = cards[1].querySelector('input') as HTMLInputElement
    await act(async () => { setInput(other, '外婆') })
    expect(progress()).toBe('2 of 2 answered')
    expect(gen().disabled).toBe(false)
    await act(async () => {
      gen().dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    // 起草在途：敲定钮置灰（owner 2026-09-25），Back/Next 同步锁死
    expect(gen().disabled).toBe(true)
    expect(back().disabled).toBe(true)
    expect(nextBtn(el).disabled).toBe(true)
    await act(async () => {
      resolveDraft(MODEL_DRAFT)
      await new Promise((r) => setTimeout(r, 250))
    })
    expect(back().disabled).toBe(false)
    expect(nextBtn(el).disabled).toBe(false)
    expect(optimize).toHaveBeenCalledWith('Demo', '一个本地待办清单', [
      { question: '技术栈？', answer: 'React' },
      { question: '给谁用？', answer: '外婆' },
    ])
    expect(create).not.toHaveBeenCalled()
    const boxes = [...el.querySelectorAll('textarea')]
    expect(boxes).toHaveLength(2)
    expect(boxes[1].value).toBe(MODEL_DRAFT)
    expect(boxes[1].value).toContain('- Build:')
    expect(boxes[1].value).toContain('未知')
    expect(boxes[1].value).not.toMatch(/npm|cargo/)
    await act(async () => { setTextArea(boxes[1], `${MODEL_DRAFT}\n人手改过`) })
    await act(async () => { back().dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    // 重排（2026-09-25）：brief=3，后退回到目录=2，不再是 roles=3
    expect(el.textContent).toContain('2 · Directory')
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

  it('出题在途时 Back/Next 都锁死，落地才放行', async () => {
    // owner 2026-09-25：任何 AI 调用在途都不许跳步——草稿还在生成
    let resolveQ: (v: { question: string; options: string[] }[]) => void = () => {}
    vi.spyOn(api, 'briefQuestions').mockImplementation(
      () => new Promise((r) => { resolveQ = r }),
    )
    vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    const back = () => [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(back().disabled).toBe(true)
    expect(nextBtn(el).disabled).toBe(true)
    await act(async () => {
      resolveQ([{ question: '技术栈？', options: ['React'] }])
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(back().disabled).toBe(false)
    expect(nextBtn(el).disabled).toBe(false)
  })

  it('答问卡可取消：关掉侧弹不发起起草', async () => {
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
    ])
    const optimize = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    const panel = el.querySelector('[data-qa-cards]') as HTMLElement
    expect(panel).toBeTruthy()
    await clickIn(panel, 'Cancel')
    expect(el.querySelector('[data-qa-cards]')).toBeNull()
    expect(optimize).not.toHaveBeenCalled()
  })

  it('出题失败回落直出草稿，答问不挡优化', async () => {
    vi.spyOn(api, 'briefQuestions').mockRejectedValue({ code: 'bad_questions', message: 'no json' })
    const optimize = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    expect(el.querySelector('[data-qa-cards]')).toBeNull()
    expect(optimize).toHaveBeenCalledWith('Demo', '一个本地待办清单', [])
    const boxes = [...el.querySelectorAll('textarea')]
    expect(boxes[1].value).toBe(MODEL_DRAFT)
  })

  it('跳过讨论直接起草：qa 为空数组', async () => {
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
    ])
    const optimize = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    expect(el.querySelector('[data-qa-cards]')).toBeTruthy()
    const skip = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Skip — draft now')!
    await act(async () => {
      skip.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    expect(optimize).toHaveBeenCalledWith('Demo', '一个本地待办清单', [])
  })

  it('优化期间显示动画+秒数，一句话输入与下方草稿都禁用', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...seeded, brief: '一个本地待办清单', agentsMd: '# 旧草稿', genAgents: true,
    }))
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([])
    let resolveDraft: (v: string) => void = () => {}
    vi.spyOn(api, 'optimizeAgentsMd').mockImplementation(
      () => new Promise((r) => { resolveDraft = r }),
    )
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    // LoadingState（role=status + shimmer 文案 + 计时），两个文本框都锁
    expect(el.querySelector('[role="status"]')).toBeTruthy()
    expect(el.textContent).toContain('Drafting AGENTS.md')
    const boxes = [...el.querySelectorAll('textarea')] as HTMLTextAreaElement[]
    expect(boxes[0].disabled).toBe(true)
    expect(boxes[1].disabled).toBe(true)
    // 起草在途锁跳步（owner 2026-09-25）
    const back = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    expect(back.disabled).toBe(true)
    expect(nextBtn(el).disabled).toBe(true)
    await act(async () => {
      resolveDraft(MODEL_DRAFT)
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(el.querySelector('[role="status"]')).toBeNull()
    expect(boxes[0].disabled).toBe(false)
    expect(boxes[1].disabled).toBe(false)
    expect(back.disabled).toBe(false)
    expect(nextBtn(el).disabled).toBe(false)
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
        checks: ['npm test'], quality_checks: { tests: 'npm test' },
        reviews: [],
        stamp_point: true,
        backfill_edges: [['QA', '前端']],
        consult_wake: ['架构师'],
      }],
    })
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    expect(el.querySelector('[data-flow-draft]')).toBeTruthy()
    // 2026-09-30 原生验收：取消架构师后生成流程仍带架构师；请求必须携带当前名单。
    expect(api.draftFlow).toHaveBeenCalledWith('一个本地待办', seeded.roles)
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

  it('项目说明换成魂斗罗后，确认页不再沿用上一个项目的流程名', async () => {
    const calc = {
      name: '桌面端跨平台计算器软件',
      version: 1,
      knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{
        name: '规格', roles: ['产品策划'], due: ['规格'], checks: [], quality_checks: {},
        reviews: [], stamp_point: false, backfill_edges: [], consult_wake: [],
      }],
    }
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...seeded,
      dir: '/Users/andyzheng/work/bbbb',
      name: 'aacddd',
      brief: '魂斗罗横版射击游戏',
      agentsMd: '桌面端跨平台计算器软件。四则运算。',
      flowPack: calc,
    }))
    let resolveFlow: (v: PackDef) => void = () => {}
    const flow = vi.spyOn(api, 'draftFlow').mockImplementation(
      () => new Promise((r) => { resolveFlow = r }),
    )
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    expect(el.querySelector('[role="status"]')).toBeTruthy()
    expect(el.textContent).toContain('Regenerating the flow draft for this project')
    expect(nextBtn(el).disabled).toBe(true)
    await act(async () => {
      resolveFlow({ ...calc, name: '魂斗罗' })
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(el.textContent).not.toContain('Regenerating the flow draft')
    await advance(el, '7 · Launch')
    expect(flow).toHaveBeenCalled()
    expect(String(flow.mock.calls.at(-1)?.[0])).toContain('魂斗罗')
    expect(el.textContent).toContain('魂斗罗')
    expect(el.textContent).not.toContain('桌面端跨平台计算器软件')
  })

  it('已选角色变化后重新生成流程，不沿用含已卸角色的缓存', async () => {
    const current = { ...seeded, brief: '本地容量台' }
    const pack: PackDef = {
      name: '旧团队', version: 1,
      knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{ name: '规格', roles: ['架构师'], due: [], checks: [], quality_checks: {}, reviews: [], stamp_point: true, backfill_edges: [], consult_wake: [] }],
    }
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...current, flowPack: pack,
      flowDraftKey: JSON.stringify([`${current.dir}\0${current.name}\0\0${current.brief}`, ['架构师']]),
    }))
    const flow = vi.spyOn(api, 'draftFlow').mockResolvedValue({ ...pack, name: '当前团队', stages: [{ ...pack.stages[0], roles: current.roles }] })
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    expect(flow).toHaveBeenCalledWith(current.brief, current.roles)
    expect(el.textContent).toContain('当前团队')
    expect(el.textContent).not.toContain('旧团队')
  })

  it('重起草失败不能携带旧团队流程继续创建', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...seeded, brief: '本地容量台', flowDraftKey: 'old-team',
      flowPack: { name: '旧团队', version: 1, stages: [{ name: '旧阶段', roles: ['架构师'], due: [] }] },
    }))
    vi.spyOn(api, 'draftFlow').mockRejectedValue(new Error('unselected role'))
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    expect(el.textContent).toContain('unselected role')
    expect(nextBtn(el).disabled).toBe(true)
  })

  it('流程起草在途锁跳步，落地才放行', async () => {
    // draft_flow 也是 AI 调用：在途 Back/Next 同锁（owner 2026-09-25）
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...seeded, brief: '一个本地待办' }))
    let resolveFlow: (v: PackDef) => void = () => {}
    vi.spyOn(api, 'draftFlow').mockImplementation(
      () => new Promise((r) => { resolveFlow = r }),
    )
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    const back = () => [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    expect(back().disabled).toBe(true)
    expect(nextBtn(el).disabled).toBe(true)
    expect(el.querySelector('[role="status"]')).toBeTruthy()
    await act(async () => {
      resolveFlow({
        name: '从说明来',
        version: 1,
        knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
        stages: [{
          name: '规格', roles: ['产品策划'], due: ['规格'], checks: [], quality_checks: {},
          reviews: [], stamp_point: true, backfill_edges: [], consult_wake: [],
        }],
      })
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(back().disabled).toBe(false)
    expect(nextBtn(el).disabled).toBe(false)
  })

  // 阶段角色从手填输入改为 EntityChips 勾选（名单=第三步勾选角色）——断言面跟着换：
  // 不再找 input，改看 chip + picker 弹窗
  it('阶段角色走勾选弹窗：chip 展示、名单来自勾上的角色、编外名保留', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...seeded, roles: ['产品策划', 'QA'], brief: '一个本地待办',
    }))
    vi.spyOn(api, 'draftFlow').mockResolvedValue({
      name: '从说明来',
      version: 1,
      knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{
        name: '规格',
        roles: ['产品策划', '编外角色'],
        due: [], checks: [], quality_checks: {}, reviews: [],
        stamp_point: false, backfill_edges: [], consult_wake: [],
      }],
    })
    const el = await renderWizard()
    await advance(el, '5 · Flow draft')
    const group = el.querySelector('[role="group"][aria-label="Active roles"]')!
    expect(group).toBeTruthy()
    expect(el.querySelector('input[aria-label="Active roles"]')).toBeNull()
    expect(group.textContent).toContain('产品策划')
    expect(group.textContent).toContain('编外角色')
    const add = [...group.querySelectorAll('button')].find((b) => b.textContent === '+ Add')!
    await act(async () => { add.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const dialog = el.querySelector('[role="dialog"]')!
    expect(dialog).toBeTruthy()
    const rowFor = (name: string) =>
      [...dialog.querySelectorAll('label')].find((l) => l.querySelector('.mono')?.textContent === name)!
    // 名单=第三步勾选角色；编外名以 custom 行出现可取消
    expect(rowFor('QA')).toBeTruthy()
    await act(async () => { (rowFor('QA').querySelector('input') as HTMLInputElement).click() })
    await act(async () => { (rowFor('编外角色').querySelector('input') as HTMLInputElement).click() })
    const done = [...dialog.querySelectorAll('button')].find((b) => b.textContent === 'Done')!
    await act(async () => { done.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    // 落 chip 按勾选名单序（产品策划→QA），编外名勾掉后消失
    const chips = [...group.querySelectorAll('.chip')].map((c) => c.textContent?.replace('×', '').trim())
    expect(chips).toEqual(['产品策划', 'QA'])
  })

  it('说明步 textarea 走 .input 原语', async () => {
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    expect(el.querySelectorAll('textarea.input').length).toBeGreaterThanOrEqual(1)
  })

  it('优化后离开向导不会创建项目', async () => {
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([])
    vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await renderWizard()
    await advance(el, '3 · Project brief')
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '一个本地待办清单') })
    const optimizeBtn = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Optimize')!
    await act(async () => {
      optimizeBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
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
    await advance(el, '3 · Project brief')
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

const ROLE_TPLS = [
  { origin: 'builtin', def: { name: '项目经理', duty: '派活', reviewer: null, model_slot: 'chat', globs: [], skills: [] } },
  { origin: 'builtin', def: { name: '后端', duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', globs: [], skills: [] } },
  { origin: 'custom', def: { name: '插画师', duty: '出图', reviewer: null, model_slot: 'chat', globs: [], skills: [] } },
]

function setInput(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
  setter.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

async function clickIn(el: HTMLElement | Element, label: string) {
  const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)
  if (!btn) throw new Error(`missing button ${label}`)
  await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
}

describe('向导角色步：默认全选 + 定制弹窗', () => {
  beforeEach(async () => {
    localStorage.clear()
    await i18n.changeLanguage('en')
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue(ROLE_TPLS)
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({}))
  })
  afterEach(async () => {
    await unmount()
    vi.restoreAllMocks()
    document.body.replaceChildren()
    localStorage.clear()
    await i18n.changeLanguage(prevLang)
  })

  // 重排（2026-09-25）：roles=4，前面隔着 brief=3——存档草稿靠 draft.brief 过门
  const onRoles = (el: HTMLElement) => advance(el, '4 · Roles')

  it('全新草稿默认全选所有角色模板', async () => {
    // 全新草稿没有存档可带 brief——非空目录走「生成 AGENTS.md」勾选项过 brief 门
    vi.spyOn(api, 'agentsMdDraft').mockResolvedValue('# skel')
    const el = await renderWizard()
    await act(async () => { nextBtn(el).dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const fields = [...el.querySelectorAll('input')].filter((i) => i.type !== 'checkbox') as HTMLInputElement[]
    await act(async () => { setInput(fields[0], '/repo/demo'); setInput(fields[1], 'Demo') })
    await act(async () => { await new Promise((r) => setTimeout(r, 250)) })
    await advance(el, '3 · Project brief')
    const gen = el.querySelector('input[type=checkbox]') as HTMLInputElement
    await act(async () => { gen.click() })
    await act(async () => { await new Promise((r) => setTimeout(r, 250)) })
    await onRoles(el)
    const boxes = [...el.querySelectorAll<HTMLInputElement>('input[type=checkbox]')]
    expect(boxes).toHaveLength(ROLE_TPLS.length)
    expect(boxes.every((b) => b.checked)).toBe(true)
  })

  it('存档草稿的勾选不被默认全选盖掉', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const el = await renderWizard()
    await onRoles(el)
    const picked = [...el.querySelectorAll<HTMLInputElement>('input[type=checkbox]')].filter((b) => b.checked)
    expect(picked).toHaveLength(1)
    expect(picked[0].closest('label')!.textContent).toContain('后端')
  })

  it('定制开右侧弹窗：取消丢弃编辑，保存才落 override 并标已定制', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const el = await renderWizard()
    await onRoles(el)
    await clickIn(el, 'Customize')
    const dialog = el.querySelector('[data-role-customize]') as HTMLElement
    expect(dialog).toBeTruthy()
    expect(dialog.getAttribute('role')).toBe('dialog')
    // 与向导并列同层（非滚动体内联）
    expect(dialog.previousElementSibling?.textContent).toContain('Project wizard')
    await act(async () => { setTextArea(dialog.querySelector('textarea')!, '改过的职责') })
    await clickIn(dialog, 'Cancel')
    expect(el.querySelector('[data-role-customize]')).toBeNull()
    expect(el.textContent).not.toContain('Customized')
    await clickIn(el, 'Customize')
    const again = el.querySelector('[data-role-customize]') as HTMLElement
    await act(async () => { setTextArea(again.querySelector('textarea')!, '改过的职责') })
    await clickIn(again, 'Save')
    expect(el.querySelector('[data-role-customize]')).toBeNull()
    const card = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(card.textContent).toContain('Customized')
  })

  it('还原模板再保存：与模板同形不写 override，已定制标消失', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端'],
      roleOverrides: { 后端: { ...ROLE_TPLS[1].def, duty: 'custom duty' } },
    }))
    const el = await renderWizard()
    await onRoles(el)
    const card = () => [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(card().textContent).toContain('Customized')
    await clickIn(el, 'Customize')
    const dialog = el.querySelector('[data-role-customize]') as HTMLElement
    await clickIn(dialog, 'Reset to template')
    await clickIn(dialog, 'Save')
    expect(el.querySelector('[data-role-customize]')).toBeNull()
    expect(card().textContent).not.toContain('Customized')
  })

  /// 定制弹窗按卡定位——「Customize」按钮每张卡一个，要按角色名找所属 panel
  async function customize(el: HTMLElement, roleName: string) {
    const btn = [...el.querySelectorAll('button')].find(
      (b) => b.textContent === 'Customize' && b.closest('.panel')?.textContent?.includes(roleName),
    )
    if (!btn) throw new Error(`missing Customize for ${roleName}`)
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }

  it('AI 起草职责：按说明起草段落落 override；globs 不产不覆盖（ADR 0075）', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft, roles: ['后端', '项目经理'],
    }))
    // mock 仍返回 globs（旧行为），UI 侧应一律不采纳
    const seeds = vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([
      { name: '项目经理', duty: '菜谱项目的派活与验收', globs: ['docs/**'] },
      { name: '后端', duty: '菜谱 API 与服务端实现', globs: ['server/**', 'src/api/**'] },
    ])
    const el = await renderWizard()
    await onRoles(el)
    await clickIn(el, 'AI-draft duties')
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def, ROLE_TPLS[1].def])
    const card = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(card.textContent).toContain('菜谱 API 与服务端实现')
    expect(card.textContent).toContain('Customized')
    // 职责段落卡面截两行（line-clamp 样式 jsdom 读不回，靠浏览器侧）；
    // 全文经 title 悬浮与「定制」卡可达
    const dutyLine = [...card.querySelectorAll('div')].find(
      (d) => d.getAttribute('title') === '菜谱 API 与服务端实现',
    )
    expect(dutyLine?.textContent).toBe('菜谱 API 与服务端实现')
    // 定制弹窗：globs 收进高级折叠，展开后是模板生效值（空）而非 AI 起草值
    await customize(el, '后端')
    const dialog = el.querySelector('[data-role-customize]') as HTMLElement
    const areas = [...dialog.querySelectorAll('textarea')] as HTMLTextAreaElement[]
    expect(areas[0].value).toBe('菜谱 API 与服务端实现')
    expect(areas).toHaveLength(1)
    const toggle = dialog.querySelector('[data-globs-toggle]') as HTMLButtonElement
    await act(async () => { toggle.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const expanded = [...dialog.querySelectorAll('textarea')] as HTMLTextAreaElement[]
    expect(expanded[1].value).toBe('')
    await clickIn(dialog, 'Cancel')
  })

  it('AI 起草可重复：覆盖自己的草稿，不碰人手定制', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft, roles: ['后端', '项目经理'],
    }))
    const seeds = vi.spyOn(api, 'draftRoleDefs')
      .mockResolvedValueOnce([
        { name: '项目经理', duty: '初稿派活', globs: [] },
        { name: '后端', duty: '初稿 API', globs: [] },
      ])
      .mockResolvedValueOnce([
        { name: '项目经理', duty: '重起草派活', globs: [] },
      ])
    const el = await renderWizard()
    await onRoles(el)
    const draftBtn = () => el.querySelector('[data-ai-draft]') as HTMLButtonElement
    const clickDraft = () => act(async () => {
      draftBtn().dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    await clickDraft()
    // 起草过一次按钮仍可点且换标签——AI 落的 override 不算人手定制
    expect(draftBtn().disabled).toBe(false)
    expect(draftBtn().textContent).toBe('AI re-draft duties')
    // 人手定制「后端」：保存即出 AI 起草名单，之后起草不再碰它
    await customize(el, '后端')
    const dialog = el.querySelector('[data-role-customize]') as HTMLElement
    await act(async () => { setTextArea(dialog.querySelector('textarea')!, '人手改过的职责') })
    await clickIn(dialog, 'Save')
    await clickDraft()
    // 再起草只入参可再起草的角色（项目经理），后端不碰。
    // 不把上一轮职责附进说明——附进去会把旧项目原文抄回来。
    expect(seeds).toHaveBeenLastCalledWith('demo brief', [expect.objectContaining({ name: '项目经理' })])
    const pm = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('项目经理'))!
    expect(pm.textContent).toContain('重起草派活')
    const be = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(be.textContent).toContain('人手改过的职责')
    expect(be.textContent).not.toContain('重起草')
    // 「项目经理」仍是 AI 来源——按钮继续可用，可无限重复起草
    expect(draftBtn().disabled).toBe(false)
  })

  it('AI 起草回了相同稿：落库但给「相同」提示，不沉默（owner 实测 2026-09-25）', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft, roles: ['后端', '项目经理'],
      roleDrafted: ['后端', '项目经理'],
      roleOverrides: {
        后端: { ...ROLE_TPLS[1].def, duty: '现稿 API' },
        项目经理: { ...ROLE_TPLS[0].def, duty: '现稿派活' },
      },
    }))
    vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([
      { name: '后端', duty: '现稿 API', globs: [] },
      { name: '项目经理', duty: '现稿派活', globs: [] },
    ])
    const el = await renderWizard()
    await onRoles(el)
    await act(async () => {
      el.querySelector<HTMLButtonElement>('[data-ai-draft]')!
        .dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(el.textContent).toContain('Model returned the same duties')
  })

  it('老草稿无 roleDrafted：override 按 AI 起草回填，按钮可用可重起草', async () => {
    // roleDrafted 引入前的存档：override 键缺省按全部 AI 起草处理，
    // 否则起草按钮永久锁死（owner 裁决 2026-09-25）
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端', '项目经理'],
      roleOverrides: {
        后端: { ...ROLE_TPLS[1].def, duty: '旧版 AI 落的职责' },
        项目经理: { ...ROLE_TPLS[0].def, duty: '旧版 AI 落的派活' },
      },
    }))
    const seeds = vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([
      { name: '项目经理', duty: '新稿派活', globs: [] },
      { name: '后端', duty: '新稿 API', globs: [] },
    ])
    const el = await renderWizard()
    await onRoles(el)
    const draftBtn = () => el.querySelector('[data-ai-draft]') as HTMLButtonElement
    expect(draftBtn().disabled).toBe(false)
    expect(draftBtn().textContent).toBe('AI re-draft duties')
    await act(async () => {
      draftBtn().dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(seeds).toHaveBeenCalled()
    const be = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(be.textContent).toContain('新稿 API')
  })

  it('新建角色走右侧弹卡：取消不建不存', async () => {
    // owner 2026-09-25：新建卡与定制卡同款右侧并列弹层，取消整份丢弃
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const save = vi.spyOn(api, 'saveRoleTemplate').mockResolvedValue(undefined)
    const el = await renderWizard()
    await onRoles(el)
    const open = el.querySelector('[data-new-role]') as HTMLButtonElement
    await act(async () => { open.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const form = el.querySelector('[data-new-role-form]') as HTMLElement
    expect(form).toBeTruthy()
    expect(form.getAttribute('role')).toBe('dialog')
    // 与向导并列同层（非滚动体内联）
    expect(form.previousElementSibling?.textContent).toContain('Project wizard')
    // 填了名也不落库——取消整份丢弃
    await act(async () => { setInput(form.querySelector('input')!, '新角色') })
    await clickIn(form, 'Cancel')
    expect(el.querySelector('[data-new-role-form]')).toBeNull()
    expect(save).not.toHaveBeenCalled()
    expect(el.textContent).not.toContain('新角色')
  })

  it('新建角色里 AI 起草职责在途时锁跳步，落定才放行', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    let resolveDuty: (v: string) => void = () => {}
    vi.spyOn(api, 'draftRoleDuty').mockImplementation(
      () => new Promise((r) => { resolveDuty = r }),
    )
    const el = await renderWizard()
    await onRoles(el)
    const open = el.querySelector('[data-new-role]') as HTMLButtonElement
    await act(async () => { open.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const form = el.querySelector('[data-new-role-form]') as HTMLElement
    await act(async () => { setInput(form.querySelector('input')!, '插画师2') })
    const draftBtn = [...form.querySelectorAll('button')].find((b) => b.textContent === 'Draft with AI')!
    const back = () => [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    await act(async () => {
      draftBtn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(back().disabled).toBe(true)
    expect(nextBtn(el).disabled).toBe(true)
    await act(async () => {
      resolveDuty('起草的职责')
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(back().disabled).toBe(false)
    expect(nextBtn(el).disabled).toBe(false)
  })

  it('名单里只有产品策划时，其余已定制角色仍整批重写', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端', '项目经理'],
      roleDrafted: ['项目经理'],
      roleOverrides: {
        项目经理: { ...ROLE_TPLS[0].def, duty: '俄罗斯方块的派活。不写规格。' },
        后端: { ...ROLE_TPLS[1].def, duty: '实现俄罗斯方块的接口。不写界面。' },
      },
    }))
    const seeds = vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([
      { name: '项目经理', duty: '魂斗罗的关卡与验收。不写实现。', globs: [] },
      { name: '后端', duty: '魂斗罗的存档与接口。不写界面。', globs: [] },
    ])
    const el = await renderWizard()
    await onRoles(el)
    await act(async () => {
      el.querySelector<HTMLButtonElement>('[data-ai-draft]')!
        .dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def, ROLE_TPLS[1].def])
    const be = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(be.textContent).toContain('魂斗罗的存档')
    expect(be.textContent).not.toContain('俄罗斯方块')
  })

  it('换项目后模型抄回的旧职责不能留在其他角色上', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端', '项目经理'],
      brief: '魂斗罗',
      roleDrafted: ['后端', '项目经理'],
      roleOverrides: {
        项目经理: { ...ROLE_TPLS[0].def, duty: '俄罗斯方块的派活与验收。不写规格。' },
        后端: { ...ROLE_TPLS[1].def, duty: '实现俄罗斯方块的数据与接口。不写界面。' },
      },
    }))
    vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([
      { name: '项目经理', duty: '魂斗罗的关卡范围与验收。不写实现。', globs: [] },
      { name: '后端', duty: '实现俄罗斯方块的数据与接口。不写界面。', globs: [] },
    ])
    const el = await renderWizard()
    await onRoles(el)
    await act(async () => {
      el.querySelector<HTMLButtonElement>('[data-ai-draft]')!
        .dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    const pm = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('项目经理'))!
    const be = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(pm.textContent).toContain('魂斗罗')
    expect(be.textContent).not.toContain('俄罗斯方块')
    expect(be.textContent).toContain('服务端实现')
  })

  it('项目说明变更后重置 AI 职责，按钮回到起草而不是重新起草', async () => {
    // owner 2026-09-25：说明变了，上一轮按旧说明起草的职责不再对应当前项目。
    // 应清掉 AI 落的 override，按钮回到「AI-draft duties」。人手定制保留。
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ empty: true }))
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端', '项目经理'],
      brief: '旧项目：菜谱',
      roleDrafted: ['项目经理'],
      roleHandTuned: ['后端'],
      roleOverrides: {
        项目经理: { ...ROLE_TPLS[0].def, duty: '菜谱项目的派活与验收。不写规格。' },
        后端: { ...ROLE_TPLS[1].def, duty: '人手改过的职责' },
      },
    }))
    const el = await renderWizard()
    await onRoles(el)
    const draftBtn = () => el.querySelector('[data-ai-draft]') as HTMLButtonElement
    expect(draftBtn().textContent).toBe('AI re-draft duties')
    const back = () => [...el.querySelectorAll('button')].find((b) => b.textContent === 'Back')!
    await act(async () => { back().dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const brief = el.querySelector('textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '新项目：本地待办') })
    await onRoles(el)
    const pm = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('项目经理'))!
    expect(pm.textContent).toContain('派活')
    expect(pm.textContent).not.toContain('菜谱项目的派活')
    const be = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(be.textContent).toContain('人手改过的职责')
    expect(draftBtn().textContent).toBe('AI-draft duties')
  })

  it('AI 起草跳过已定制角色：人手编辑优先于 AI 草稿', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端', '项目经理'],
      roleOverrides: { 后端: { ...ROLE_TPLS[1].def, duty: '人手定制' } },
      roleHandTuned: ['后端'],
      roleDrafted: [],
    }))
    const seeds = vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([
      { name: '项目经理', duty: '起草的派活', globs: [] },
    ])
    const el = await renderWizard()
    await onRoles(el)
    await clickIn(el, 'AI-draft duties')
    // 已定制的「后端」不入参，AI 碰不到它
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def])
    const be = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('后端'))!
    expect(be.textContent).toContain('人手定制')
    expect(be.textContent).not.toContain('服务端实现')
  })
})
