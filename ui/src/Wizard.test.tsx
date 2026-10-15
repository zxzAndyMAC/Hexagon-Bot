import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import i18n from './i18n'
import { api } from './api'
import type { PackDef } from './gen/PackDef'
import type { ProvidersView } from './gen/ProvidersView'
import {
  MODEL_DRAFT, advance, backBtn, button, click, clickIn, currentStep, draft, isOff, nextBtn, prov,
  readyDoc, renderWizard, report, setInput, setTextArea, settle, unmount,
} from './wizardTestUtils'

// ui-redesign 票 10：断言面迁到新外壳（components E9/E10）。步骤以步骤轨 aria-current 判定，
// 按钮禁用读 aria-disabled（components §0：禁用仍可聚焦以读出原因），表单控件走 A9 Field 外壳。
// 角色步的用例在 Wizard.roles.test.tsx。

const prevLang = i18n.language
const password = (el: HTMLElement) => el.querySelector('input[type="password"]')

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
    expect(currentStep(el)).toBe('providers')
    expect(el.textContent).toContain('Connect a model provider')
    expect(el.textContent).not.toContain('Project directory')
  })

  it('步骤轨七步、未到的步骤不可点', async () => {
    // E9：竖向步骤轨，完成步可点回跳；未来步不是按钮（跳步会绕过放行条件）
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
    const el = await renderWizard()
    const steps = [...el.querySelectorAll('[data-step]')].map((n) => n.getAttribute('data-step'))
    expect(steps).toEqual(['providers', 'dir', 'brief', 'roles', 'flow', 'keys', 'confirm'])
    expect(el.querySelector('[data-step="dir"] button')).toBeNull()
    await click(nextBtn(el))
    expect(currentStep(el)).toBe('dir')
    const done = el.querySelector('[data-step="providers"] button')
    expect(done).not.toBeNull()
    await click(done)
    expect(currentStep(el)).toBe('providers')
  })

  it('启用且有钥匙但没有 default 槽时不能继续，且不再要钥匙', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'or', name: 'OpenRouter' })],
      slots: {},
    })
    const el = await renderWizard()
    expect(isOff(nextBtn(el))).toBe(true)
    expect(password(el)).toBeNull()
    expect(el.textContent).toContain('default slot')
  })

  it('缺钥匙时不能继续，并给出钥匙输入', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'or', name: 'OpenRouter', key_set: false })],
      slots: { default: { provider_id: 'or', model: 'm1' } },
    })
    const el = await renderWizard()
    expect(isOff(nextBtn(el))).toBe(true)
    expect(password(el)).toBeTruthy()
  })

  it('停用的供应商即使有钥匙和 default 槽也不能继续，且不再要钥匙', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'or', name: 'OpenRouter', enabled: false })],
      slots: { default: { provider_id: 'or', model: 'm1' } },
    })
    const el = await renderWizard()
    expect(isOff(nextBtn(el))).toBe(true)
    expect(password(el)).toBeNull()
  })

  it('三项齐全时显示就绪、不渲染钥匙框，下一步进入目录', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
    const el = await renderWizard()
    expect(el.textContent).toContain('Ready')
    expect(el.textContent).toContain('OpenRouter')
    expect(password(el)).toBeNull()
    expect(isOff(nextBtn(el))).toBe(false)
    await click(nextBtn(el))
    expect(currentStep(el)).toBe('dir')
    expect(el.textContent).toContain('Project directory')
  })

  it('表单控件走 A9 Field 外壳', async () => {
    // 票 10：旧断言是「走 .input 原语」；新体系的 A9 Field 外壳是 [data-slot=input|native-select]，
    // 守的仍是同一件事——输入框不能套按钮样式融进底色。
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    const el = await renderWizard()
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    // gap B8（2026-10-15）：原型三字段——服务商下拉（默认落第一条内置预设）/ 密钥 / 模型。
    // 票 10 时空表是五个框（名称/类型/地址/密钥/模型）；名称/类型/地址现在只在「自定义」下展开。
    // gap B12（2026-10-15）：服务商下拉改成带头像的 Select（[data-slot=select] 外壳同 A9），计数不变。
    expect(el.querySelectorAll('[data-slot=input] input, [data-slot=native-select] select, [data-slot=select] button')).toHaveLength(3)
  })

  // gap B8：内置服务商预设（core provider_presets）
  const PRESETS = [
    { id: 'anthropic', name: 'Anthropic', kind: 'anthropic' as const, base_url: 'https://api.anthropic.com', default_model: 'claude-opus-5-5' },
    { id: 'or', name: 'OpenRouter', kind: 'openai' as const, base_url: 'https://openrouter.ai/api/v1', default_model: '' },
  ]
  // gap B12（2026-10-15）：服务商下拉从原生 <select> 改成带头像的 Select（Radix，role=combobox / option）。
  // B8 时读 select.value / options；现在读触发器显示的选中项文本、弹层 option 文本。
  const providerSelect = (el: HTMLElement) => el.querySelector('[data-provider-select]') as HTMLButtonElement
  const options = () => [...document.querySelectorAll('[role="option"]')] as HTMLElement[]
  const text = (n: Element | null) => n?.querySelector('[data-select-text]')?.textContent
  async function openSelect(el: HTMLElement) {
    await act(async () => { providerSelect(el).dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }
  async function choose(el: HTMLElement, label: string) {
    await openSelect(el)
    const opt = options().find((o) => text(o) === label)!
    await act(async () => { opt.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }

  it('选预设自动填类型与地址，只问钥匙和模型', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'providerPresets').mockResolvedValue(PRESETS)
    vi.spyOn(api, 'probeProviderKey').mockResolvedValue({ ok: true, failure: null, status: null, model_listed: true, window: 200000 })
    const save = vi.spyOn(api, 'saveProvider').mockResolvedValue(undefined)
    const bind = vi.spyOn(api, 'setSlotBinding').mockResolvedValue(undefined)
    const el = await renderWizard()
    expect(text(providerSelect(el))).toBe('Anthropic')
    const model = el.querySelector('input[aria-label="Default model"]') as HTMLInputElement
    expect(model.value).toBe('claude-opus-5-5')
    await act(async () => { setInput(password(el) as HTMLInputElement, 'sk-test') })
    await click(button(el, 'Save and bind default'))
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'anthropic', name: 'Anthropic', kind: 'anthropic', base_url: 'https://api.anthropic.com', enabled: true }),
      'sk-test')
    expect(bind).toHaveBeenCalledWith('default', 'anthropic', 'claude-opus-5-5')
  })

  it('「自定义」才展开名称 / 类型 / 地址', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'providerPresets').mockResolvedValue(PRESETS)
    const el = await renderWizard()
    expect(el.querySelector('input[aria-label="Base URL"]')).toBeNull()
    await choose(el, 'Custom…')
    expect(el.querySelectorAll('[data-slot=input] input, [data-slot=native-select] select, [data-slot=select] button')).toHaveLength(6)
    expect(isOff(button(el, 'Save and bind default'))).toBe(true)
  })

  it('已配置的供应商与未配置的预设同列；同 id 预设不重复', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [prov({ id: 'or', name: 'My OpenRouter', key_set: false })], slots: {} })
    vi.spyOn(api, 'providerPresets').mockResolvedValue(PRESETS)
    const el = await renderWizard()
    expect(text(providerSelect(el))).toBe('My OpenRouter')
    expect(providerSelect(el).querySelector('[data-slot=avatar]')).not.toBeNull()
    await openSelect(el)
    expect(options().map(text)).toEqual(['My OpenRouter', 'Anthropic', 'Custom…'])
    // gap B12：每个服务商选项带字母头像（确定色，按供应商 id 哈希）；「自定义」不是服务商，不画头像
    expect(options().map((o) => !!o.querySelector('[data-slot=avatar]'))).toEqual([true, true, false])
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    // 已配置但缺钥匙：只补钥匙
    expect(password(el)).not.toBeNull()
  })

  // gap B13：预设表与设置目录同一份；向导只列能当 default 主对话槽、要钥匙的（Jev、本机端点去设置页配）
  it('向导不列 Jev 与本机端点预设', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'providerPresets').mockResolvedValue([
      ...PRESETS,
      { id: 'ollama', name: 'Ollama', kind: 'openai', base_url: 'http://localhost:11434/v1', default_model: '' },
      { id: 'typesafe', name: 'TypeSafe', kind: 'jev', base_url: 'https://api.typesafe.ai', default_model: 'jev-latest' },
    ])
    const el = await renderWizard()
    await openSelect(el)
    expect(options().map(text)).toEqual(['Anthropic', 'OpenRouter', 'Custom…'])
  })

  // gap B12：下拉换成 Select 后键盘与读屏照旧——combobox 触发器、↵ 打开、↓ 移动、↵ 选中、焦点回触发器
  it('服务商下拉可全键盘操作', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'providerPresets').mockResolvedValue(PRESETS)
    const el = await renderWizard()
    const trigger = providerSelect(el)
    expect(trigger.getAttribute('role')).toBe('combobox')
    expect(trigger.getAttribute('aria-label')).toBe('Provider')
    const key = (target: Element, k: string) =>
      act(async () => {
        target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }))
        await new Promise((r) => setTimeout(r, 0)) // Radix 在下一拍挪焦点
      })
    trigger.focus()
    await key(trigger, 'Enter')
    expect(trigger.getAttribute('aria-expanded')).toBe('true')
    // gap C5（2026-10-15，foundations §4）：向导降为全屏视图 16，低于弹层 20，弹层照常挂 body 也看得见；
    // gap B12 时向导是 z-60，才需要把弹层挂进向导层（已撤）。
    const lb = document.querySelector<HTMLElement>('[role="listbox"]')!
    expect(lb.closest('[data-wizard]')).toBeNull()
    expect(document.querySelector('[data-wizard]')!.className).toContain('z-[16]')
    expect(document.activeElement && text(document.activeElement)).toBe('Anthropic')
    await key(document.activeElement!, 'ArrowDown')
    expect(text(document.activeElement)).toBe('OpenRouter')
    await key(document.activeElement!, 'Enter')
    expect(trigger.getAttribute('aria-expanded')).toBe('false')
    expect(text(providerSelect(el))).toBe('OpenRouter')
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
    const apply = button(el, 'Save and bind default')!
    expect(isOff(apply)).toBe(false)
    await click(apply)
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ id: 'or', enabled: true }), undefined)
    expect(bind).toHaveBeenCalledWith('default', 'or', 'm1')
  })

  // gap B4（2026-10-15）：服务商步实时检测钥匙（原型「检测通过 · 200K 窗口」）；票 10 时无检测 IPC，不检测
  const probeDoc = (): ProvidersView => ({
    providers: [prov({
      id: 'or', name: 'OpenRouter',
      models: [{ id: 'm1', name: null, group: null, caps: [], context_window: null, max_output: null }],
    })],
    slots: {},
  })
  const waitProbe = () => act(async () => { await new Promise((r) => setTimeout(r, 700)) })

  it('已存钥匙 + 模型：停顿后检测一次，通过时显示窗口', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(probeDoc())
    const probe = vi.spyOn(api, 'probeProviderKey').mockResolvedValue({ ok: true, failure: null, status: null, model_listed: true, window: 200000 })
    const el = await renderWizard()
    await waitProbe()
    expect(probe).toHaveBeenCalledTimes(1)
    expect(probe).toHaveBeenCalledWith(expect.objectContaining({ id: 'or', models: [] }), undefined, 'm1')
    expect(el.querySelector('[data-probe="ok"]')?.textContent).toContain('200K')
  })

  it('检测失败写出原因，保存仍可点（检测不是闸）', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(probeDoc())
    vi.spyOn(api, 'probeProviderKey').mockResolvedValue({ ok: false, failure: 'authentication', status: 401, model_listed: null, window: null })
    const el = await renderWizard()
    await waitProbe()
    expect(el.querySelector('[data-probe="failed"]')).not.toBeNull()
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('rejected this key')
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('HTTP 401')
    expect(isOff(button(el, 'Save and bind default')!)).toBe(false)
  })

  it('目录里没有所填模型时提示核对拼写', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(probeDoc())
    vi.spyOn(api, 'probeProviderKey').mockResolvedValue({ ok: true, failure: null, status: null, model_listed: false, window: null })
    const el = await renderWizard()
    await waitProbe()
    expect(el.querySelector('[data-probe="unlisted"]')).not.toBeNull()
    expect(el.textContent).toContain('check the spelling')
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
    await click(nextBtn(el))
    await settle()
    return el
  }

  it('脏树不停步，并说明改动会保留', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ dirty: true }))
    const el = await onDirectory()
    expect(el.textContent).toContain('Uncommitted changes stay as they are')
    expect(el.textContent).not.toContain('stops here')
    expect(isOff(nextBtn(el))).toBe(false)
  })

  it('目录步输入框走 A9 Field 外壳', async () => {
    // 票 10：旧断言为 input.input 计数，改为 A9 外壳计数（目录 + 项目名）
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({}))
    const el = await onDirectory()
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    expect(el.querySelectorAll('[data-slot=input] input')).toHaveLength(2)
  })

  it('已有文件但没有说明时解释停步原因，并在核对草稿后放行', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, brief: '' }))
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({}))
    const skeleton = vi.spyOn(api, 'agentsMdDraft').mockResolvedValue('# Owner instructions\n')
    const el = await onDirectory()
    await click(nextBtn(el))
    // Owner QA 2026-10-06: a disabled Next button gave no reason or usable
    // next step for an existing folder without instructions (issue 06).
    expect(el.textContent).toContain('To continue, this folder needs project instructions.')
    expect(isOff(nextBtn(el))).toBe(true)
    expect(skeleton).not.toHaveBeenCalled()
    await click(el.querySelector('[data-gen-agents]'))
    expect(skeleton).toHaveBeenCalledWith('Demo')
    expect(el.textContent).not.toContain('To continue, this folder needs project instructions.')
    expect(isOff(nextBtn(el))).toBe(false)
  })

  it('已有工作台状态不能下一步新建，打开走 openRecent', async () => {
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ has_workbench: true, dirty: true }))
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await onDirectory()
    expect(isOff(nextBtn(el))).toBe(true)
    expect(el.textContent).toContain('open it instead of creating again')
    await clickIn(el, 'Open this project')
    expect(open).toHaveBeenCalledWith('/repo/demo')
    expect(create).not.toHaveBeenCalled()
  })
})

describe('一句话优化成项目说明（票 16 · 票 10 卡内逐题）', () => {
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

  const briefBox = (el: HTMLElement) => el.querySelector('[data-brief-compose] textarea') as HTMLTextAreaElement
  const draftBox = (el: HTMLElement) => el.querySelector('[data-agents-draft] textarea') as HTMLTextAreaElement | null
  const card = (el: HTMLElement) => el.querySelector('[data-qa-cards]') as HTMLElement | null
  const progress = (el: HTMLElement) => el.querySelector('[data-qa-progress]')!.textContent
  const continueBtn = (el: HTMLElement) => el.querySelector('[data-qa-continue]') as HTMLButtonElement

  async function optimize(el: HTMLElement, sentence: string) {
    await act(async () => { setTextArea(briefBox(el), sentence) })
    await act(async () => {
      button(el, 'Optimize')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
  }

  it('空目录优化后可改，确认才提交草稿，后退不创建', async () => {
    // 答问流（2026-09-25）：出题在前，答齐敲定才起草；跳过讨论直出。
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
      { question: '给谁用？', options: ['家人'] },
    ])
    let resolveDraft: (v: string) => void = () => {}
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockImplementation(
      () => new Promise((r) => { resolveDraft = r }),
    )
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const skeleton = vi.spyOn(api, 'agentsMdDraft').mockResolvedValue('# SKELETON')
    const el = await renderWizard()
    await advance(el, 'brief')
    expect(skeleton).not.toHaveBeenCalled()
    expect(isOff(button(el, 'Optimize'))).toBe(true)
    await act(async () => { setTextArea(briefBox(el), '一个本地待办清单') })
    expect(isOff(button(el, 'Optimize'))).toBe(false)
    await act(async () => {
      button(el, 'Optimize')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    // 票 10 / reconciliation §2 #5：答问卡改在向导卡片内原地逐题（旧断言是右侧并列 role=dialog 弹层）。
    expect(card(el)).toBeTruthy()
    expect(card(el)!.getAttribute('role')).not.toBe('dialog')
    expect(card(el)!.closest('[data-wizard]')).toBeTruthy()
    // 一句话折叠在上方，可「修改」
    expect(el.textContent).toContain('「一个本地待办清单」')
    // components E9：答问期间步骤轨与上一步 / 下一步先锁定
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    expect(el.textContent).toContain('Locked while you answer')
    // 逐题：进度是当前题号 / 总数；未答不能继续
    expect(progress(el)).toBe('1 / 2')
    expect(isOff(continueBtn(el))).toBe(true)
    await click(card(el)!.querySelector('[data-qa-option]'))
    expect(isOff(continueBtn(el))).toBe(false)
    await click(continueBtn(el))
    expect(progress(el)).toBe('2 / 2')
    // 已答的题折叠成一行
    expect(el.textContent).toContain('1 answered: 技术栈？ = React')
    // 第二题「其他」手填
    await click(card(el)!.querySelector('[data-qa-other]'))
    await act(async () => { setInput(card(el)!.querySelector('input')!, '外婆') })
    await act(async () => {
      continueBtn(el).dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    // 起草在途：卡位换像素格，Back/Next 同步锁死（owner 2026-09-25）
    expect(card(el)).toBeNull()
    expect(el.querySelector('[role="status"]')).toBeTruthy()
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    await act(async () => {
      resolveDraft(MODEL_DRAFT)
      await new Promise((r) => setTimeout(r, 250))
    })
    expect(isOff(backBtn(el))).toBe(false)
    expect(isOff(nextBtn(el))).toBe(false)
    expect(optimizeApi).toHaveBeenCalledWith('Demo', '一个本地待办清单', [
      // fullstack QA #01：原选项也是负责人作出选择时的上下文，须随问题保留。
      { question: '技术栈？\n1. React\n2. Python', answer: 'React' },
      { question: '给谁用？\n1. 家人', answer: '外婆' },
    ])
    expect(create).not.toHaveBeenCalled()
    // 答完同一位置换成草稿编辑框，上方是「已答 N 题 · 用于起草」
    expect(el.textContent).toContain('2 answers · used for the draft')
    expect(draftBox(el)!.value).toBe(MODEL_DRAFT)
    await act(async () => { setTextArea(draftBox(el)!, `${MODEL_DRAFT}\n人手改过`) })
    await click(backBtn(el))
    // 重排（2026-09-25）：brief=3，后退回到目录
    expect(currentStep(el)).toBe('dir')
    expect(create).not.toHaveBeenCalled()
    await advance(el, 'confirm')
    await clickIn(el, 'Create project')
    expect(create).toHaveBeenCalledTimes(1)
    const md = create.mock.calls[0][0].agentsMd as string
    expect(md).toContain('人手改过')
    expect(md).toContain('- Build:')
    expect(md).toContain('未知')
    expect(md).not.toMatch(/npm|cargo/)
  })

  it('数字键选择、↵ 继续，「其他」输入框里数字是正文', async () => {
    // entry#wz-brief：数字键 1–9 选择、↵ 继续，是卡内局部键（不进全局 keymap）
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '平台？', options: ['Web', '桌面', '手机'] },
      { question: '账号？', options: ['不要', '要'] },
    ])
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '记账')
    const key = async (target: Element, k: string) => {
      await act(async () => { target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true })) })
    }
    await key(card(el)!, '2')
    expect(card(el)!.querySelector('[aria-checked="true"]')!.textContent).toContain('桌面')
    await key(card(el)!, 'Enter')
    expect(progress(el)).toBe('2 / 2')
    await click(card(el)!.querySelector('[data-qa-other]'))
    const other = card(el)!.querySelector('input')!
    await key(other, '1')
    expect(card(el)!.querySelector('[aria-checked="true"]')).toBeNull()
    await act(async () => { setInput(other, '需要 2 个账号') })
    await key(other, 'Enter')
    await settle(50)
    expect(optimizeApi).toHaveBeenCalledWith('Demo', '记账', [
      { question: '平台？\n1. Web\n2. 桌面\n3. 手机', answer: '桌面' },
      { question: '账号？\n1. 不要\n2. 要', answer: '需要 2 个账号' },
    ])
  })

  it('已答的题可展开回到该题修改', async () => {
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
      { question: '给谁用？', options: ['家人', '同事'] },
    ])
    vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '待办')
    await click(card(el)!.querySelector('[data-qa-option]'))
    await click(continueBtn(el))
    expect(progress(el)).toBe('2 / 2')
    await clickIn(el, 'Expand')
    const back = [...el.querySelectorAll('li button')].find((b) => b.textContent?.includes('技术栈？'))!
    await click(back)
    expect(progress(el)).toBe('1 / 2')
    await click([...card(el)!.querySelectorAll('[data-qa-option]')].find((b) => b.textContent?.includes('Python')))
    expect(card(el)!.querySelector('[aria-checked="true"]')!.textContent).toContain('Python')
  })

  it.each(['以上三类都要', '前两类先做'])('起草保留完整选项，让指代答案“%s”有上下文', async (answer) => {
    // 2026-10-03 fullstack QA #01：只传“以上三类都要”曾让模型声明统计需求未明确。
    const options = ['工单状态与积压', '成员工作量与完成率', '库存占用与预警', '以上三类都要']
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '统计页展示哪些内容？', options },
    ])
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '团队交付台')
    if (options.includes(answer)) {
      await click([...card(el)!.querySelectorAll('[data-qa-option]')].find((b) => b.textContent?.includes(answer)))
    } else {
      await click(card(el)!.querySelector('[data-qa-other]'))
      await act(async () => { setInput(card(el)!.querySelector('input')!, answer) })
    }
    await click(continueBtn(el))
    const qa = optimizeApi.mock.calls[0][2]!
    expect(qa).toHaveLength(1)
    expect(qa[0].answer).toBe(answer)
    for (const option of options) expect(qa[0].question).toContain(option)
  })

  it('出题在途时步骤轨与 Back/Next 都锁死；答问期间仍锁，修改一句话才放行', async () => {
    // owner 2026-09-25：任何 AI 调用在途都不许跳步——草稿还在生成
    let resolveQ: (v: { question: string; options: string[] }[]) => void = () => {}
    vi.spyOn(api, 'briefQuestions').mockImplementation(
      () => new Promise((r) => { resolveQ = r }),
    )
    vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    expect(el.querySelector('[data-step="dir"] button')).not.toBeNull()
    await act(async () => { setTextArea(briefBox(el), '一个本地待办清单') })
    await act(async () => {
      button(el, 'Optimize')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    // E9：AI 在途时步骤轨回跳也锁（完成步不再是按钮）
    expect(el.querySelector('[data-step="dir"] button')).toBeNull()
    expect(el.textContent).toContain('AI is generating')
    await act(async () => {
      resolveQ([{ question: '技术栈？', options: ['React'] }])
      await new Promise((r) => setTimeout(r, 50))
    })
    // 票 10 改写：旧断言是出题落地即放行；entry#wz-brief / components E9「答问期间先锁定」，
    // 出口是答问卡的继续 / 跳过或一句话的「修改」。
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    await clickIn(el, 'Edit')
    expect(isOff(backBtn(el))).toBe(false)
    expect(isOff(nextBtn(el))).toBe(false)
    expect(el.querySelector('[data-step="dir"] button')).not.toBeNull()
  })

  it('一句话「修改」关掉答问，不发起起草', async () => {
    // 票 10 改写：旧版是并列弹层的「取消」；卡内逐题后撤出口是一句话折叠行的「修改」
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
    ])
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '一个本地待办清单')
    expect(card(el)).toBeTruthy()
    await clickIn(el, 'Edit')
    expect(card(el)).toBeNull()
    expect(briefBox(el).value).toBe('一个本地待办清单')
    expect(optimizeApi).not.toHaveBeenCalled()
  })

  it('出题失败回落直出草稿，答问不挡优化', async () => {
    vi.spyOn(api, 'briefQuestions').mockRejectedValue({ code: 'bad_questions', message: 'no json' })
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '一个本地待办清单')
    expect(card(el)).toBeNull()
    expect(optimizeApi).toHaveBeenCalledWith('Demo', '一个本地待办清单', [])
    expect(draftBox(el)!.value).toBe(MODEL_DRAFT)
    expect(el.textContent).toContain('Questions could not be generated')
  })

  it('跳过讨论直接起草：qa 为空数组', async () => {
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([
      { question: '技术栈？', options: ['React', 'Python'] },
    ])
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '一个本地待办清单')
    expect(card(el)).toBeTruthy()
    await act(async () => {
      button(el, 'Skip — draft now')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    expect(optimizeApi).toHaveBeenCalledWith('Demo', '一个本地待办清单', [])
  })

  it('重新起草期间卡位换成像素格 + 计时，编辑面不可用，落定后恢复', async () => {
    // 票 10 改写：旧断言是「一句话与草稿两个文本框都 disabled」；components E10「出题 / 起草：
    // 卡位像素格」，在途时编辑面整个换成加载块（不再并排显示禁用的文本框）。
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...seeded, brief: '一个本地待办清单', agentsMd: '# 旧草稿', genAgents: true,
    }))
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([])
    let resolveDraft: (v: string) => void = () => {}
    vi.spyOn(api, 'optimizeAgentsMd').mockImplementation(
      () => new Promise((r) => { resolveDraft = r }),
    )
    const el = await renderWizard()
    await advance(el, 'brief')
    expect(draftBox(el)!.value).toBe('# 旧草稿')
    await act(async () => {
      button(el, 'Redraft')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
    const status = el.querySelector('[role="status"]')!
    expect(status).toBeTruthy()
    expect(status.textContent).toContain('Drafting AGENTS.md')
    expect(status.textContent).toMatch(/\d+\.\ds/)
    expect(el.querySelectorAll('textarea')).toHaveLength(0)
    // 起草在途锁跳步（owner 2026-09-25）
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    await act(async () => {
      resolveDraft(MODEL_DRAFT)
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(el.querySelector('[role="status"]')).toBeNull()
    expect(draftBox(el)!.value).toBe(MODEL_DRAFT)
    expect(draftBox(el)!.disabled).toBe(false)
    expect(isOff(backBtn(el))).toBe(false)
    expect(isOff(nextBtn(el))).toBe(false)
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
    await advance(el, 'flow')
    expect(el.querySelector('[data-flow-draft]')).toBeTruthy()
    // 2026-09-30 原生验收：取消架构师后生成流程仍带架构师；请求必须携带当前名单。
    expect(api.draftFlow).toHaveBeenCalledWith('一个本地待办', seeded.roles)
    expect(el.textContent).not.toContain('npm test')
    expect(el.textContent).not.toContain('backfill')
    expect([...el.querySelectorAll('input, textarea')].every((n) => (n as HTMLInputElement).value !== 'npm test')).toBe(true)
    const names = () => [...el.querySelectorAll<HTMLInputElement>('input[aria-label="Stage name"]')].map((n) => n.value)
    await act(async () => { setInput(el.querySelector('input[aria-label="Stage name"]')!, '改过的阶段') })
    expect(names()).toEqual(['改过的阶段'])
    // 票 10 改写：盖章点由复选框改为 aria-pressed 切换（components E10「盖章 / 自动」chip）
    const stamp = el.querySelector('[data-stamp-toggle]')!
    expect(stamp.getAttribute('aria-pressed')).toBe('true')
    await click(stamp)
    expect(el.querySelector('[data-stamp-toggle]')!.getAttribute('aria-pressed')).toBe('false')
    await click(el.querySelector('[data-add-stage]'))
    expect(el.querySelectorAll('[data-stage-row]')).toHaveLength(2)
    // 票 10 改写：「上移 / 下移」按钮换成拖拽 + 键盘（moveStageDown 默认 ⌥↓），焦点跟随该行
    const firstRow = el.querySelectorAll('[data-stage-row]')[0]
    await act(async () => {
      firstRow.querySelector('button')!.focus()
      firstRow.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', altKey: true, bubbles: true }))
    })
    expect(names()).toEqual(['New stage', '改过的阶段'])
    expect(document.activeElement).toBe(el.querySelectorAll('[data-stage-row]')[1].querySelector('button'))
    await act(async () => {
      el.querySelectorAll('[data-stage-row]')[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, bubbles: true }))
    })
    expect(names()).toEqual(['改过的阶段', 'New stage'])
    expect(el.querySelector('[data-stage-row] button')!.getAttribute('title')).toMatch(/↑ \/ .*↓/)
    await click(el.querySelector('button[aria-label="Remove stage"]'))
    expect(el.querySelectorAll('[data-stage-row]')).toHaveLength(1)
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
    await advance(el, 'flow')
    expect(el.querySelector('[role="status"]')).toBeTruthy()
    expect(el.textContent).toContain('Regenerating the flow draft for this project')
    expect(isOff(nextBtn(el))).toBe(true)
    await act(async () => {
      resolveFlow({ ...calc, name: '魂斗罗' })
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(el.textContent).not.toContain('Regenerating the flow draft')
    await advance(el, 'confirm')
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
    await advance(el, 'flow')
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
    await advance(el, 'flow')
    expect(el.textContent).toContain('unselected role')
    expect(isOff(nextBtn(el))).toBe(true)
  })

  it('流程起草在途锁跳步，落地才放行', async () => {
    // draft_flow 也是 AI 调用：在途 Back/Next 同锁（owner 2026-09-25）
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...seeded, brief: '一个本地待办' }))
    let resolveFlow: (v: PackDef) => void = () => {}
    vi.spyOn(api, 'draftFlow').mockImplementation(
      () => new Promise((r) => { resolveFlow = r }),
    )
    const el = await renderWizard()
    await advance(el, 'flow')
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
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
    expect(isOff(backBtn(el))).toBe(false)
    expect(isOff(nextBtn(el))).toBe(false)
  })

  it('阶段角色走勾选弹窗：头像叠展示、名单来自勾上的角色、编外名保留', async () => {
    // 票 10 改写：激活角色由 chip 行改为头像叠按钮（components E10 流程行），可读名列出角色
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
    await advance(el, 'flow')
    const rolesBtn = () => el.querySelector('[data-stage-roles]')!
    expect(rolesBtn().getAttribute('aria-label')).toBe('Active roles: 产品策划, 编外角色')
    expect(rolesBtn().querySelectorAll('[data-slot=avatar]')).toHaveLength(2)
    await click(rolesBtn())
    const dialog = el.querySelector('[role="dialog"]')!
    expect(dialog).toBeTruthy()
    // ui-redesign 票 11：EntityPicker 换皮，条目名不再挂旧 .mono 类，按 data-picker-id 找
    const rowFor = (name: string) =>
      [...dialog.querySelectorAll('label')].find((l) => l.querySelector('[data-picker-id]')?.textContent === name)!
    // 名单=第三步勾选角色；编外名以 custom 行出现可取消
    expect(rowFor('QA')).toBeTruthy()
    await act(async () => { (rowFor('QA').querySelector('input') as HTMLInputElement).click() })
    await act(async () => { (rowFor('编外角色').querySelector('input') as HTMLInputElement).click() })
    await clickIn(dialog, 'Done')
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    // 落库按勾选名单序（产品策划→QA），编外名勾掉后消失
    expect(rolesBtn().getAttribute('aria-label')).toBe('Active roles: 产品策划, QA')
  })

  it('说明步文本框走 A9 Field 外壳', async () => {
    const el = await renderWizard()
    await advance(el, 'brief')
    expect(el.querySelectorAll('input.btn, textarea.btn, select.btn')).toHaveLength(0)
    expect(el.querySelectorAll('[data-slot=textarea] textarea').length).toBeGreaterThanOrEqual(1)
  })

  it('优化后离开向导不会创建项目', async () => {
    vi.spyOn(api, 'briefQuestions').mockResolvedValue([])
    vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await renderWizard()
    await advance(el, 'brief')
    await optimize(el, '一个本地待办清单')
    expect(draftBox(el)!.value).toContain('未知')
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
    const optimizeApi = vi.spyOn(api, 'optimizeAgentsMd').mockResolvedValue(MODEL_DRAFT)
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    const el = await renderWizard()
    await advance(el, 'brief')
    expect(el.textContent).toContain('Instructions file detected')
    expect(el.textContent).toContain(file)
    expect(button(el, 'Optimize')).toBeUndefined()
    expect(el.textContent).not.toContain('SHOULD NOT LAND')
    expect(optimizeApi).not.toHaveBeenCalled()
    await advance(el, 'confirm')
    await clickIn(el, 'Create project')
    expect(create).toHaveBeenCalledTimes(1)
    expect(create.mock.calls[0][0].agentsMd).toBeNull()
  })
})
