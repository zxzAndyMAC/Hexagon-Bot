import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import i18n from './i18n'
import { api } from './api'
import {
  advance, backBtn, button, click, clickIn, draft, isOff, nextBtn, readyDoc, renderWizard, report,
  setInput, setTextArea, settle, storedDraft, unmount,
} from './wizardTestUtils'

// ui-redesign 票 10 · ADR 0091：角色步改为卡内「列表 | 详情」，定制即改即生效写入向导草稿，
// 「恢复模板」取代「保存 / 放弃」；roleHandTuned 判定改为「详情区发生过人工编辑」；
// 新建角色仍写全局模板库，并需要显式「创建角色」。从 Wizard.test.tsx 拆出。

const prevLang = i18n.language

const ROLE_TPLS = [
  { origin: 'builtin', def: { name: '项目经理', duty: '派活', reviewer: null, model_slot: 'chat', globs: [], skills: [] } },
  { origin: 'builtin', def: { name: '后端', duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', globs: [], skills: [] } },
  { origin: 'custom', def: { name: '插画师', duty: '出图', reviewer: null, model_slot: 'chat', globs: [], skills: [] } },
]

const row = (el: HTMLElement, name: string) => el.querySelector(`[data-role-row="${name}"]`) as HTMLElement
const detail = (el: HTMLElement) => el.querySelector('[data-role-customize]') as HTMLElement | null
const duty = (el: HTMLElement) => detail(el)!.querySelector('textarea[aria-label="Duty"]') as HTMLTextAreaElement
const aiDraftBtn = (el: HTMLElement) => el.querySelector('[data-ai-draft]') as HTMLButtonElement
const checkboxes = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('[role="listbox"] [role="checkbox"]')]

async function clickAiDraft(el: HTMLElement) {
  await act(async () => {
    aiDraftBtn(el).dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await new Promise((r) => setTimeout(r, 50))
  })
}

describe('向导角色步：列表 | 详情（ADR 0091）', () => {
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
  const onRoles = (el: HTMLElement) => advance(el, 'roles')

  it('全新草稿默认全选所有角色模板', async () => {
    // 全新草稿没有存档可带 brief——非空目录走「生成 AGENTS.md」勾选项过 brief 门
    vi.spyOn(api, 'agentsMdDraft').mockResolvedValue('# skel')
    const el = await renderWizard()
    await click(nextBtn(el))
    const fields = [...el.querySelectorAll<HTMLInputElement>('[data-slot=input] input')]
    await act(async () => { setInput(fields[0], '/repo/demo'); setInput(fields[1], 'Demo') })
    await settle()
    await advance(el, 'brief')
    await click(el.querySelector('[data-gen-agents]'))
    await settle()
    await onRoles(el)
    expect(checkboxes(el)).toHaveLength(ROLE_TPLS.length)
    expect(checkboxes(el).every((b) => b.getAttribute('aria-checked') === 'true')).toBe(true)
    expect(el.textContent).toContain('Templates · 3 / 3 selected')
    expect(el.textContent).toContain('3 selected')
  })

  it('存档草稿的勾选不被默认全选盖掉；未勾行半透明', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const el = await renderWizard()
    await onRoles(el)
    const picked = checkboxes(el).filter((b) => b.getAttribute('aria-checked') === 'true')
    expect(picked).toHaveLength(1)
    expect(picked[0].getAttribute('aria-label')).toBe('后端')
    expect(row(el, '项目经理').className).toContain('opacity-60')
  })

  it('勾选与定制分开：点复选框只改名单，点整行才打开该角色的详情', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const el = await renderWizard()
    await onRoles(el)
    // 默认选中第一个已勾角色（components E10 补设计）
    expect(detail(el)!.getAttribute('aria-label')).toBe('后端')
    await click(row(el, '项目经理').querySelector('[role="checkbox"]'))
    expect(storedDraft().roles).toEqual(['后端', '项目经理'])
    expect(detail(el)!.getAttribute('aria-label')).toBe('后端')
    await click(row(el, '插画师'))
    expect(detail(el)!.getAttribute('aria-label')).toBe('插画师')
    expect(row(el, '插画师').getAttribute('aria-selected')).toBe('true')
    expect(storedDraft().roles).toEqual(['后端', '项目经理'])
  })

  it('定制即改即生效：详情区编辑直接写入草稿并标已定制，没有保存 / 放弃', async () => {
    // 票 10 改写（ADR 0091）：旧断言是「定制弹窗取消丢弃编辑，保存才落 override」。
    // 现在详情区每次编辑都写进向导草稿（创建项目前不落盘），撤销入口是「恢复模板」。
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const el = await renderWizard()
    await onRoles(el)
    await click(row(el, '后端'))
    expect(detail(el)!.closest('[data-wizard]')).toBeTruthy()
    expect(detail(el)!.getAttribute('role')).not.toBe('dialog')
    expect(button(detail(el)!, 'Save')).toBeUndefined()
    expect(button(detail(el)!, 'Cancel')).toBeUndefined()
    await act(async () => { setTextArea(duty(el), '改过的职责') })
    expect(storedDraft().roleOverrides['后端'].duty).toBe('改过的职责')
    expect(storedDraft().roleHandTuned).toEqual(['后端'])
    expect(row(el, '后端').textContent).toContain('Customized')
    expect(row(el, '后端').textContent).toContain('改过的职责')
    expect(detail(el)!.textContent).toContain('Saved to the wizard draft · this project only')
    // 换行再回来：编辑仍在（不是弹窗本地副本）
    await click(row(el, '项目经理'))
    await click(row(el, '后端'))
    expect(duty(el).value).toBe('改过的职责')
    // 创建项目时随 role_overrides 提交
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    vi.spyOn(api, 'draftFlow').mockResolvedValue({
      name: 'f', version: 1, knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{ name: 's', roles: ['后端'], due: [], checks: [], quality_checks: {}, reviews: [], stamp_point: false, backfill_edges: [], consult_wake: [] }],
    })
    await advance(el, 'confirm')
    await clickIn(el, 'Create project')
    expect(create.mock.calls[0][0].roleOverrides).toEqual([expect.objectContaining({ name: '后端', duty: '改过的职责' })])
  })

  // owner-followups-1015 票 17：向导角色详情换头像——图只在向导内存（不进 localStorage 草稿），
  // 勾选角色的图随建项目单 roleAvatars 下发；未勾的角色不带；「恢复默认」后不带。
  it('角色头像：换图随建项目下发、不进草稿存档；恢复默认后不带', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const IMG = 'data:image/png;base64,iVBORw0KGgo='
    vi.stubGlobal('FileReader', class {
      onload: (() => Promise<void>) | null = null
      result = IMG
      readAsDataURL() { void Promise.resolve().then(() => this.onload?.()) }
    })
    const el = await renderWizard()
    await onRoles(el)
    const pick = async (name: string) => {
      await click(row(el, name))
      const input = detail(el)!.querySelector<HTMLInputElement>('input[type="file"]')!
      Object.defineProperty(input, 'files', { value: [new File(['x'], 'a.png', { type: 'image/png' })], configurable: true })
      await act(async () => { input.dispatchEvent(new Event('change', { bubbles: true })) })
      await settle()
    }
    await pick('项目经理') // 未勾选：不随单
    await pick('后端')
    expect(detail(el)!.querySelector('[data-avatar-restore]')).toBeTruthy()
    expect(localStorage.getItem('hexagon.wizard')).not.toContain('iVBORw0KGgo')
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    vi.spyOn(api, 'draftFlow').mockResolvedValue({
      name: 'f', version: 1, knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{ name: 's', roles: ['后端'], due: [], checks: [], quality_checks: {}, reviews: [], stamp_point: false, backfill_edges: [], consult_wake: [] }],
    })
    await advance(el, 'confirm')
    await clickIn(el, 'Create project')
    expect(create.mock.calls[0][0].roleAvatars).toEqual([{ role: '后端', data_url: IMG }])
    vi.unstubAllGlobals()
  })

  it('角色头像「恢复默认」后建项目不带图', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    vi.stubGlobal('FileReader', class {
      onload: (() => Promise<void>) | null = null
      result = 'data:image/png;base64,iVBORw0KGgo='
      readAsDataURL() { void Promise.resolve().then(() => this.onload?.()) }
    })
    const el = await renderWizard()
    await onRoles(el)
    const input = detail(el)!.querySelector<HTMLInputElement>('input[type="file"]')!
    Object.defineProperty(input, 'files', { value: [new File(['x'], 'a.png', { type: 'image/png' })], configurable: true })
    await act(async () => { input.dispatchEvent(new Event('change', { bubbles: true })) })
    await settle()
    await click(detail(el)!.querySelector('[data-avatar-restore]'))
    expect(detail(el)!.querySelector('[data-avatar-restore]')).toBeNull()
    const create = vi.spyOn(api, 'createProject').mockResolvedValue(undefined)
    vi.spyOn(api, 'draftFlow').mockResolvedValue({
      name: 'f', version: 1, knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      stages: [{ name: 's', roles: ['后端'], due: [], checks: [], quality_checks: {}, reviews: [], stamp_point: false, backfill_edges: [], consult_wake: [] }],
    })
    await advance(el, 'confirm')
    await clickIn(el, 'Create project')
    expect(create.mock.calls[0][0].roleAvatars).toEqual([])
    vi.unstubAllGlobals()
  })

  it('恢复模板：还原成模板原样、清掉已定制与人手标记，之后 AI 可再起草它', async () => {
    // 票 10 改写（ADR 0091）：旧断言是「还原模板再保存」两步；现在「恢复模板」一步生效。
    localStorage.setItem('hexagon.wizard', JSON.stringify({
      ...draft,
      roles: ['后端'],
      roleOverrides: { 后端: { ...ROLE_TPLS[1].def, duty: 'custom duty' } },
      roleHandTuned: ['后端'],
      roleDrafted: [],
    }))
    const seeds = vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([{ name: '后端', duty: '菜谱 API', globs: [] }])
    const el = await renderWizard()
    await onRoles(el)
    expect(row(el, '后端').textContent).toContain('Customized')
    // 全部勾选角色都是人手定制时 AI 起草无事可做
    expect(isOff(aiDraftBtn(el))).toBe(true)
    await click(detail(el)!.querySelector('[data-restore-template]'))
    expect(row(el, '后端').textContent).not.toContain('Customized')
    expect(duty(el).value).toBe('服务端实现')
    expect(storedDraft().roleOverrides).toEqual({})
    expect(storedDraft().roleHandTuned).toEqual([])
    expect(isOff(aiDraftBtn(el))).toBe(false)
    await clickAiDraft(el)
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[1].def])
  })

  it('roleHandTuned = 详情区发生过人工编辑：改回模板原样也不再被 AI 覆盖', async () => {
    // ADR 0091：判定从「定制卡点过保存」改为「详情区发生过人工编辑」。
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端', '项目经理'] }))
    const seeds = vi.spyOn(api, 'draftRoleDefs').mockResolvedValue([{ name: '项目经理', duty: '起草的派活', globs: [] }])
    const el = await renderWizard()
    await onRoles(el)
    await click(row(el, '后端'))
    await act(async () => { setTextArea(duty(el), '临时改一下') })
    await act(async () => { setTextArea(duty(el), '服务端实现') })
    // 与模板同形不写 override（「已定制」只标真实差异），但人手标记保留
    expect(storedDraft().roleOverrides).toEqual({})
    expect(row(el, '后端').textContent).not.toContain('Customized')
    expect(storedDraft().roleHandTuned).toEqual(['后端'])
    await clickAiDraft(el)
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def])
  })

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
    await clickAiDraft(el)
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def, ROLE_TPLS[1].def])
    // 票 10 改写：AI 落的稿标「AI 起草」chip（components E10），人手编辑才标「已定制」
    expect(row(el, '后端').textContent).toContain('菜谱 API 与服务端实现')
    expect(row(el, '后端').textContent).toContain('AI draft')
    expect(el.textContent).toContain('Updated duties for 2 roles')
    // 职责一行省略；全文经 title 与详情区可达
    const dutyLine = row(el, '后端').querySelector('[title="菜谱 API 与服务端实现"]')
    expect(dutyLine?.textContent).toBe('菜谱 API 与服务端实现')
    // 详情：globs 收进高级折叠，展开后是模板生效值（空）而非 AI 起草值
    await click(row(el, '后端'))
    expect(duty(el).value).toBe('菜谱 API 与服务端实现')
    expect(detail(el)!.querySelectorAll('textarea')).toHaveLength(1)
    await click(detail(el)!.querySelector('[data-globs-toggle]'))
    const areas = [...detail(el)!.querySelectorAll('textarea')]
    expect(areas[1].value).toBe('')
  })

  it('AI 起草在途：职责框换像素格、字段只读，步骤轨与上一步 / 下一步锁定', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    let resolve: (v: { name: string; duty: string; globs: string[] }[]) => void = () => {}
    vi.spyOn(api, 'draftRoleDefs').mockImplementation(() => new Promise((r) => { resolve = r }))
    const el = await renderWizard()
    await onRoles(el)
    await clickAiDraft(el)
    expect(detail(el)!.querySelector('[role="status"]')).toBeTruthy()
    expect(detail(el)!.querySelector('textarea[aria-label="Duty"]')).toBeNull()
    expect((detail(el)!.querySelector('select') as HTMLSelectElement).disabled).toBe(true)
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    expect(el.querySelector('[data-step="brief"] button')).toBeNull()
    await act(async () => {
      resolve([{ name: '后端', duty: '新稿', globs: [] }])
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(duty(el).value).toBe('新稿')
    expect(isOff(nextBtn(el))).toBe(false)
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
    await clickAiDraft(el)
    // 起草过一次按钮仍可点且换标签——AI 落的 override 不算人手定制
    expect(isOff(aiDraftBtn(el))).toBe(false)
    expect(aiDraftBtn(el).textContent).toBe('AI re-draft duties')
    // 人手编辑「后端」（ADR 0091：详情区即改即生效），之后起草不再碰它
    await click(row(el, '后端'))
    await act(async () => { setTextArea(duty(el), '人手改过的职责') })
    await clickAiDraft(el)
    // 再起草只入参可再起草的角色（项目经理），后端不碰。
    // 不把上一轮职责附进说明——附进去会把旧项目原文抄回来。
    expect(seeds).toHaveBeenLastCalledWith('demo brief', [expect.objectContaining({ name: '项目经理' })])
    expect(row(el, '项目经理').textContent).toContain('重起草派活')
    expect(row(el, '后端').textContent).toContain('人手改过的职责')
    expect(row(el, '后端').textContent).not.toContain('重起草')
    // 「项目经理」仍是 AI 来源——按钮继续可用，可无限重复起草
    expect(isOff(aiDraftBtn(el))).toBe(false)
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
    await clickAiDraft(el)
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
    expect(isOff(aiDraftBtn(el))).toBe(false)
    expect(aiDraftBtn(el).textContent).toBe('AI re-draft duties')
    await clickAiDraft(el)
    expect(seeds).toHaveBeenCalled()
    expect(row(el, '后端').textContent).toContain('新稿 API')
  })

  it('新建角色在详情区打开：取消不建不存', async () => {
    // 票 10 改写：旧版是右侧并列弹卡（role=dialog）；现在新建角色在详情区打开，与定制互斥
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const save = vi.spyOn(api, 'saveRoleTemplate').mockResolvedValue(undefined)
    const el = await renderWizard()
    await onRoles(el)
    await click(el.querySelector('[data-new-role]'))
    const form = el.querySelector('[data-new-role-form]') as HTMLElement
    expect(form).toBeTruthy()
    expect(form.getAttribute('role')).not.toBe('dialog')
    expect(detail(el)).toBeNull()
    expect(form.textContent).toContain('global template library')
    // 填了名也不落库——取消整份丢弃
    await act(async () => { setInput(form.querySelector('input')!, '新角色') })
    await clickIn(form, 'Cancel')
    expect(el.querySelector('[data-new-role-form]')).toBeNull()
    expect(save).not.toHaveBeenCalled()
    expect(el.textContent).not.toContain('新角色')
  })

  it('新建角色写入全局模板库，必须显式点「创建角色」', async () => {
    // ADR 0091 例外条：定制即改即生效，但新建角色写 ~/.hexagon/roles.json，保留显式确认
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    const save = vi.spyOn(api, 'saveRoleTemplate').mockResolvedValue(undefined)
    const el = await renderWizard()
    await onRoles(el)
    await click(el.querySelector('[data-new-role]'))
    const form = el.querySelector('[data-new-role-form]') as HTMLElement
    const create = form.querySelector('[data-create-role]') as HTMLButtonElement
    expect(isOff(create)).toBe(true)
    await act(async () => { setInput(form.querySelector('input')!, '数据分析') })
    await act(async () => { setTextArea(form.querySelector('textarea')!, '出报表') })
    expect(save).not.toHaveBeenCalled()
    // 次要样式：不与「下一步」抢主位
    expect(create.getAttribute('data-variant')).toBe('default')
    await click(create)
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ name: '数据分析', duty: '出报表' }))
    expect(el.querySelector('[data-new-role-form]')).toBeNull()
    expect(row(el, '数据分析')).toBeTruthy()
    expect(storedDraft().roles).toContain('数据分析')
    expect(storedDraft().roleHandTuned).toContain('数据分析')
    expect(detail(el)!.getAttribute('aria-label')).toBe('数据分析')
  })

  it('新建角色里 AI 起草职责在途时锁跳步，落定才放行', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['后端'] }))
    let resolveDuty: (v: string) => void = () => {}
    vi.spyOn(api, 'draftRoleDuty').mockImplementation(
      () => new Promise((r) => { resolveDuty = r }),
    )
    const el = await renderWizard()
    await onRoles(el)
    await click(el.querySelector('[data-new-role]'))
    const form = el.querySelector('[data-new-role-form]') as HTMLElement
    await act(async () => { setInput(form.querySelector('input')!, '插画师2') })
    await act(async () => {
      button(form, 'Draft with AI')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(isOff(backBtn(el))).toBe(true)
    expect(isOff(nextBtn(el))).toBe(true)
    await act(async () => {
      resolveDuty('起草的职责')
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(isOff(backBtn(el))).toBe(false)
    expect(isOff(nextBtn(el))).toBe(false)
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
    await clickAiDraft(el)
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def, ROLE_TPLS[1].def])
    expect(row(el, '后端').textContent).toContain('魂斗罗的存档')
    expect(row(el, '后端').textContent).not.toContain('俄罗斯方块')
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
    await clickAiDraft(el)
    expect(row(el, '项目经理').textContent).toContain('魂斗罗')
    expect(row(el, '后端').textContent).not.toContain('俄罗斯方块')
    expect(row(el, '后端').textContent).toContain('服务端实现')
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
    expect(aiDraftBtn(el).textContent).toBe('AI re-draft duties')
    await click(backBtn(el))
    const brief = el.querySelector('[data-brief-compose] textarea') as HTMLTextAreaElement
    await act(async () => { setTextArea(brief, '新项目：本地待办') })
    await onRoles(el)
    expect(row(el, '项目经理').textContent).toContain('派活')
    expect(row(el, '项目经理').textContent).not.toContain('菜谱项目的派活')
    expect(row(el, '后端').textContent).toContain('人手改过的职责')
    expect(aiDraftBtn(el).textContent).toBe('AI-draft duties')
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
    await clickAiDraft(el)
    // 已定制的「后端」不入参，AI 碰不到它
    expect(seeds).toHaveBeenCalledWith('demo brief', [ROLE_TPLS[0].def])
    expect(row(el, '后端').textContent).toContain('人手定制')
    expect(row(el, '后端').textContent).not.toContain('服务端实现')
  })

  it('列表是 listbox：↑↓ / Home / End 切换选中行', async () => {
    // components E2：列表为 listbox，↑↓ / Home / End
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, roles: ['项目经理'] }))
    const el = await renderWizard()
    await onRoles(el)
    const list = el.querySelector('[role="listbox"]')!
    const key = (k: string) => act(async () => { list.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true })) })
    await key('ArrowDown')
    expect(detail(el)!.getAttribute('aria-label')).toBe('后端')
    await key('End')
    expect(detail(el)!.getAttribute('aria-label')).toBe('插画师')
    await key('Home')
    expect(detail(el)!.getAttribute('aria-label')).toBe('项目经理')
  })
})

describe('启动页一句话带入项目说明步（票 10，interaction §11）', () => {
  beforeEach(async () => {
    localStorage.clear()
    await i18n.changeLanguage('en')
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue(ROLE_TPLS)
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue(readyDoc)
    vi.spyOn(api, 'inspectDir').mockResolvedValue(report({ empty: true }))
  })
  afterEach(async () => {
    await unmount()
    vi.restoreAllMocks()
    document.body.replaceChildren()
    localStorage.clear()
    await i18n.changeLanguage(prevLang)
  })

  it('种子写入草稿的 brief 与目录，第一步仍是模型服务商，项目说明步预填', async () => {
    const el = await renderWizard({ brief: '离线待办 PWA', dir: '/code/todo-pwa' })
    expect(el.querySelector('[aria-current="step"]')!.getAttribute('data-step')).toBe('providers')
    expect(storedDraft()).toMatchObject({ brief: '离线待办 PWA', dir: '/code/todo-pwa', name: 'todo-pwa' })
    // 新草稿仍默认全选（种子不算存档）
    expect(storedDraft().roles).toEqual(ROLE_TPLS.map((tp) => tp.def.name))
    await advance(el, 'brief')
    const box = el.querySelector('[data-brief-compose] textarea') as HTMLTextAreaElement
    expect(box.value).toBe('离线待办 PWA')
  })

  it('种子覆盖存档草稿的一句话，未选目录时保留存档目录', async () => {
    localStorage.setItem('hexagon.wizard', JSON.stringify({ ...draft, brief: '旧的一句话' }))
    await renderWizard({ brief: '新的一句话' })
    expect(storedDraft()).toMatchObject({ brief: '新的一句话', dir: '/repo/demo', name: 'Demo' })
  })
})
