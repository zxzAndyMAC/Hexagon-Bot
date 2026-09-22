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

  it('已有钥匙时绑定 default 槽走现有 API，且不重传钥匙', async () => {
    const doc: ProvidersView = {
      providers: [prov({
        id: 'or', name: 'OpenRouter',
        models: [{ id: 'm1', name: null, group: null, caps: [] }],
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
