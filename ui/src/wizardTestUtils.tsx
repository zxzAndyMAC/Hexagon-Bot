// 向导测试共用夹具（ui-redesign 票 10 拆分 Wizard.test 时抽出）。
// 断言面换到新外壳：步骤以步骤轨 aria-current="step" 的 data-step 判定（旧版靠「4 · Roles」这种
// 带序号的标题文字，E9 起步骤轨名不带序号）；A1 按钮禁用走 aria-disabled（components §0），
// 所以统一用 isOff 读禁用。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect } from 'vitest'
import { Wizard } from './components/Wizard'
import { TooltipProvider } from './components/ui/tooltip'
import type { DirReport } from './gen/DirReport'
import type { ProviderView } from './gen/ProviderView'
import type { ProvidersView } from './gen/ProvidersView'
import type { DraftSeed, Step } from './components/wizard/draft'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

export function prov(over: Partial<ProviderView> & Pick<ProviderView, 'id' | 'name'>): ProviderView {
  return {
    kind: 'openai',
    base_url: 'https://example.test/v1',
    models: [],
    enabled: true,
    key_set: true,
    ...over,
  }
}

export function report(over: Partial<DirReport>): DirReport {
  return {
    exists: true, empty: false, is_git: true, dirty: false, has_workbench: false, instructions: null,
    ...over,
  }
}

export const readyDoc: ProvidersView = {
  providers: [prov({ id: 'or', name: 'OpenRouter' })],
  slots: { default: { provider_id: 'or', model: 'm1' } },
}

export const draft = {
  dir: '/repo/demo', name: 'Demo', roles: [], roleOverrides: {},
  mode: 'pack', packName: '规格驱动', fastRole: '', initGit: false,
  genAgents: false, agentsMd: '',
  // brief 步提到角色前（2026-09-25）：存档草稿自带一句话，角色步直接过 canNext
  brief: 'demo brief',
}

export const MODEL_DRAFT = [
  '# Demo', '', '## 做什么', '一个本地待办清单。', '', '## Commands', '- Build:', '- Test:', '- Check:', '',
  '## Layout', '- 未知', '', '## Conventions', '-', '',
].join('\n')

let mounted: ReturnType<typeof createRoot> | null = null

export async function renderWizard(seed?: DraftSeed) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  mounted = root
  await act(async () => {
    root.render(<TooltipProvider><Wizard onDone={() => {}} seed={seed} /></TooltipProvider>)
    await new Promise((r) => setTimeout(r, 250))
  })
  return el
}

export async function unmount() {
  if (!mounted) return
  const root = mounted
  mounted = null
  await act(async () => { root.unmount() })
}

export const isOff = (b: Element | null | undefined) =>
  !!b && ((b as HTMLButtonElement).disabled || b.getAttribute('aria-disabled') === 'true')

export const currentStep = (el: HTMLElement) =>
  el.querySelector('[aria-current="step"]')?.getAttribute('data-step') ?? null

export function button(el: Element, label: string) {
  return [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined
}
export const nextBtn = (el: HTMLElement) => button(el, 'Next')!
export const backBtn = (el: HTMLElement) => button(el, 'Back')!

export async function click(node: Element | null | undefined) {
  if (!node) throw new Error('missing node')
  await act(async () => { node.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
}

export async function clickIn(el: Element, label: string) {
  const btn = button(el, label)
  if (!btn) throw new Error(`missing button ${label}`)
  await click(btn)
}

export async function settle(ms = 250) {
  await act(async () => { await new Promise((r) => setTimeout(r, ms)) })
}

export function setInput(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
  setter.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

export function setTextArea(el: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
  setter.call(el, value)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}

/** 按「下一步」直到步骤轨当前步为 step（放行条件未满足时等异步落定）。 */
export async function advance(el: HTMLElement, step: Step) {
  for (let i = 0; i < 10 && currentStep(el) !== step; i++) {
    const btn = nextBtn(el)
    if (isOff(btn)) {
      await settle()
      continue
    }
    await act(async () => {
      btn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 250))
    })
  }
  expect(currentStep(el)).toBe(step)
}

export const storedDraft = () => JSON.parse(localStorage.getItem('hexagon.wizard') || '{}')
