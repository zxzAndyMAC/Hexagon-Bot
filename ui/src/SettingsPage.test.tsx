import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { SettingsPage } from './components/SettingsPage'
import { api, type DiagRecord } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

// 启动页「设置」入口无项目上下文——六个项目作用域分区不发必败 IPC
//（with_conn → "no project open" → internal toast；本次回归的原始事故）。
describe('SettingsPage projectless 门（收口回归）', () => {
  beforeEach(() => {
    vi.spyOn(api, 'logEnabled').mockResolvedValue(true)
  })
  afterEach(() => vi.restoreAllMocks())

  const clickNav = async (el: HTMLElement, label: string) => {
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }

  it('projectless：项目态分区渲染提示且零 IPC；团队=模板库照常可用', async () => {
    const team = vi.spyOn(api, 'team')
    const rules = vi.spyOn(api, 'permissionRules')
    const skills = vi.spyOn(api, 'listSkills')
    const mcp = vi.spyOn(api, 'mcpServices')
    const reviewer = vi.spyOn(api, 'reviewerMode')
    const usage = vi.spyOn(api, 'usage')
    const tpls = vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([
      { def: { name: '产品策划', duty: '需求', reviewer: null, model_slot: 'chat', globs: [], skills: [] }, origin: 'builtin' },
      { def: { name: '插画师', duty: '出图', reviewer: null, model_slot: 'chat', globs: [], skills: [] }, origin: 'custom' },
    ])
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    skills.mockResolvedValue([
      { name: 'spec-writing', description: '规格书写规范', origin: 'builtin', enabled: true },
      { name: 'my-lint', description: '自建', origin: 'global', enabled: false },
    ])
    // 票 05：MCP 全局清单无项目可列；实况（mcpServices）仍项目态不该被调
    const mcpEntries = vi.spyOn(api, 'mcpEntries').mockResolvedValue([
      { name: 'termius', command: 'ssh-mcp', args: [], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, origin: 'global' },
    ])
    const { el, root } = await render(<SettingsPage onBack={() => {}} projectless />)
    // 团队分区不再是项目态——模板库（内置∪自定义）在无项目时列出
    await clickNav(el, 'Team')
    expect(el.textContent).toContain('产品策划')
    expect(el.textContent).toContain('插画师')
    expect(el.textContent).not.toContain('needs an open project')
    expect(tpls).toHaveBeenCalled()
    // 技能分区同理——全局层直接可用（ADR 0057）
    await clickNav(el, 'Skills')
    expect(el.textContent).toContain('spec-writing')
    expect(el.textContent).toContain('my-lint')
    expect(el.textContent).not.toContain('needs an open project')
    expect(skills).toHaveBeenCalled()
    // MCP 分区同理——全局清单列出；实况接口（宿主是项目级的）不调
    await clickNav(el, 'MCP services')
    expect(el.textContent).toContain('termius')
    expect(el.textContent).not.toContain('needs an open project')
    expect(mcpEntries).toHaveBeenCalled()
    for (const label of ['Permissions', 'Autonomy', 'Usage']) {
      await clickNav(el, label)
      expect(el.textContent).toContain('needs an open project')
    }
    for (const spy of [team, rules, mcp, reviewer, usage]) {
      expect(spy).not.toHaveBeenCalled()
    }
    root.unmount()
  })

  it('有项目上下文（默认）：分区照常取数', async () => {
    const team = vi.spyOn(api, 'team').mockResolvedValue([])
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Team')
    expect(team).toHaveBeenCalled()
    root.unmount()
  })
})

// settings-3col 票 02/03：list|detail 形态回归——选中出详情、项目层只读、
// 模板添加走 createRole 复制物化。
describe('SettingsPage list|detail 分区', () => {
  beforeEach(() => {
    vi.spyOn(api, 'logEnabled').mockResolvedValue(true)
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
  })
  afterEach(() => vi.restoreAllMocks())

  const clickNav = async (el: HTMLElement, label: string) => {
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }
  const clickText = async (el: HTMLElement, text: string) => {
    const btn = [...el.querySelectorAll<HTMLElement>('[role="button"], button')]
      .find((b) => b.textContent?.includes(text))!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }
  const type = async (el: HTMLElement, placeholder: string, value: string) => {
    const input = [...el.querySelectorAll('input')].find((i) => i.placeholder === placeholder)!
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      setter.call(input, value)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
  }

  const mcpEntries = [
    { name: 'termius', command: 'ssh-mcp', args: [], env: {}, cwd: null, disabled: false, transport: 'stdio' as const, url: null, origin: 'global' as const },
    { name: 'proj-svc', command: 'p-svc', args: [], env: {}, cwd: null, disabled: false, transport: 'stdio' as const, url: null, origin: 'project' as const },
  ]

  it('MCP：选中行出详情表单；项目层条目只读不给保存', async () => {
    vi.spyOn(api, 'mcpEntries').mockResolvedValue(mcpEntries)
    vi.spyOn(api, 'mcpServices').mockResolvedValue([])
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'MCP services')
    // 全局条目可编辑：command 字段回填
    await clickText(el, 'termius')
    const cmdInput = [...el.querySelectorAll('input')].find((i) => i.value === 'ssh-mcp')
    expect(cmdInput).toBeTruthy()
    // 项目层条目只读：提示语出现、无保存钮
    await clickText(el, 'proj-svc')
    expect(el.textContent).toContain('edit .hexagon/mcp.json')
    const saveBtn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Save')
    expect(saveBtn).toBeUndefined()
    root.unmount()
  })

  it('MCP：搜索框过滤清单', async () => {
    vi.spyOn(api, 'mcpEntries').mockResolvedValue(mcpEntries)
    vi.spyOn(api, 'mcpServices').mockResolvedValue([])
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'MCP services')
    expect(el.textContent).toContain('termius')
    await type(el, 'Filter services…', 'proj')
    expect(el.textContent).not.toContain('termius')
    expect(el.textContent).toContain('proj-svc')
    root.unmount()
  })

  it('Team（有项目）：选中 Agent 出 RoleEditor；从模板添加走 createRole', async () => {
    vi.spyOn(api, 'team').mockResolvedValue([
      { id: 'a1', role: 'reviewer', model_slot: 'chat', status: 'active', avatar_hash: null },
    ])
    const detail = vi.spyOn(api, 'agentDetail').mockResolvedValue({
      agent_id: 'a1', role: 'reviewer', status: 'active', model_slot: 'chat', custom: false,
      def: { duty: '审', reviewer: null, model_slot: 'chat', skills: [] }, globs: [], grants: [],
    })
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([
      { def: { name: '插画师', duty: '出图', reviewer: null, model_slot: 'chat', globs: [], skills: [] }, origin: 'custom' },
    ])
    const createRole = vi.spyOn(api, 'createRole').mockResolvedValue('a2')
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Team')
    // 中列 Agent 行 → 右列 RoleEditor（agentDetail 被调）
    await clickText(el, 'reviewer')
    expect(detail).toHaveBeenCalledWith('a1')
    // 「从模板添加」→ 选择面板 → Add 物化进项目
    await clickText(el, 'From template')
    expect(el.textContent).toContain('插画师')
    const addBtn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Add')!
    await act(async () => { addBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(createRole).toHaveBeenCalledWith(expect.objectContaining({ name: '插画师' }))
    root.unmount()
  })

  it('Skills：选中出详情；行内 switch 切启停', async () => {
    vi.spyOn(api, 'listSkills').mockResolvedValue([
      { name: 'my-lint', description: '自建', origin: 'global', enabled: false },
    ])
    vi.spyOn(api, 'skillFiles').mockResolvedValue(['SKILL.md'])
    vi.spyOn(api, 'readSkillFile').mockResolvedValue('# x')
    const mute = vi.spyOn(api, 'setSkillMuted').mockResolvedValue(undefined)
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Skills')
    const sw = el.querySelector('input.switch')!
    await act(async () => { sw.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(mute).toHaveBeenCalledWith('my-lint', true)
    root.unmount()
  })

  // settings-density 02：名单字段=chip 行 + 弹窗勾选（owner 裁决，取代逗号输入框）
  it('模板编辑器：技能字段 chip 化，弹窗勾选追加', async () => {
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([
      { def: { name: '插画师', duty: '出图', reviewer: null, model_slot: 'chat', globs: [], skills: ['my-lint'] }, origin: 'custom' },
    ])
    const listSkills = vi.spyOn(api, 'listSkills').mockResolvedValue([
      { name: 'spec-writing', description: '规格书写规范', origin: 'builtin', enabled: true },
      { name: 'my-lint', description: '自建', origin: 'global', enabled: true },
    ])
    const { el, root } = await render(<SettingsPage onBack={() => {}} projectless />)
    await clickNav(el, 'Team')
    await clickText(el, '插画师')
    // 已选值渲染为 chip，不再逗号文本框
    const chip = [...el.querySelectorAll('.chip')].find((c) => c.textContent?.includes('my-lint'))
    expect(chip).toBeTruthy()
    // 「+ Add」开弹窗——惰性加载清单
    const addBtn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === '+ Add')!
    await act(async () => { addBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(listSkills).toHaveBeenCalled()
    expect(el.querySelector('[role="dialog"]')).toBeTruthy()
    // 勾选 spec-writing → Done → chip 追加
    const row = [...el.querySelectorAll('label')].find((l) => l.textContent?.includes('spec-writing'))!
    const box = row.querySelector<HTMLInputElement>('input[type=checkbox]')!
    await act(async () => { box.click() })
    const done = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Done')!
    await act(async () => { done.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    const chips = [...el.querySelectorAll('.chip')].map((c) => c.textContent)
    expect(chips.some((c) => c?.includes('spec-writing'))).toBe(true)
    root.unmount()
  })
})

// diagnostic-records 票 01：「日志」分区——开关从通用页搬来、四类筛选、
// projectless 宿主可见 + 非宿主提示先开项目、行展开出 id。
describe('SettingsPage 日志分区（diagnostic-records 票 01）', () => {
  const rec = (over: Partial<DiagRecord>): DiagRecord => ({
    ts: '2026-09-18T11:05:12Z', class: '判定', level: 'debug',
    project: 'p1', agent: 'a0', activation: 'sr1', trace: '512',
    branch: 'permission', code: 'autonomy_allow', ms: 3, ...over,
  })
  const ROWS: DiagRecord[] = [
    rec({ class: '宿主', branch: 'startup', code: 'app_start', project: null, agent: null, activation: null, trace: null }),
    rec({ class: '宿主', branch: 'credentials', code: 'dev_file', project: null, agent: null, activation: null, trace: null }),
    rec({ class: '拒绝', level: 'warn', branch: 'set_autonomy', code: 'gears_removed', agent: null, activation: null, trace: null }),
    rec({}),
    rec({ class: '槽位', level: 'warn', branch: 'execute_judgment', code: 'jev_unbound', trace: null }),
  ]

  beforeEach(() => {
    vi.spyOn(api, 'logEnabled').mockResolvedValue(true)
    // 过滤口径与 core records() 对齐——mock 不是替身语义，是同一份形状。
    vi.spyOn(api, 'diagnosticRecords').mockImplementation(async (proj, cls) =>
      ROWS.filter((r) => (cls ? r.class === cls : true)).filter((r) =>
        proj ? r.project === proj || r.class === '宿主' : r.class === '宿主',
      ),
    )
  })
  afterEach(() => vi.restoreAllMocks())

  const clickNav = async (el: HTMLElement, label: string) => {
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
  }

  it('左栏有「日志」；开关在此页而不在通用页', async () => {
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    // 通用页不再有诊断开关（票 01：搬家）。
    expect(el.textContent).not.toContain('Diagnostic logging')
    expect(el.querySelector('input[type=checkbox]')).toBeNull()
    await clickNav(el, 'Logs')
    expect(el.textContent).toContain('Diagnostic logging')
    const box = el.querySelector('input[type=checkbox]')
    expect(box).toBeTruthy()
    root.unmount()
  })

  it('行渲染：全部四类同列；展开出项目/Agent/激活/轨迹 id', async () => {
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Logs')
    expect(el.textContent).toContain('app_start')
    expect(el.textContent).toContain('gears_removed')
    expect(el.textContent).toContain('autonomy_allow')
    expect(el.textContent).toContain('jev_unbound')
    // 未展开时看不到 id 值——展开 permission 行。
    const row = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('autonomy_allow'))!
    await act(async () => { row.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.textContent).toContain('a0')
    expect(el.textContent).toContain('sr1')
    expect(el.textContent).toContain('512')
    root.unmount()
  })

  it('分类筛选把词表字面量发给 core', async () => {
    const spy = vi.spyOn(api, 'diagnosticRecords')
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Logs')
    await clickNav(el, 'Rejected')
    expect(spy).toHaveBeenLastCalledWith('p1', '拒绝')
    expect(el.textContent).toContain('gears_removed')
    expect(el.textContent).not.toContain('autonomy_allow')
    await clickNav(el, 'Host')
    expect(spy).toHaveBeenLastCalledWith('p1', '宿主')
    expect(el.textContent).toContain('app_start')
    root.unmount()
  })

  it('projectless：宿主记录照常列出；选非宿主类提示先开项目', async () => {
    const { el, root } = await render(<SettingsPage onBack={() => {}} projectless />)
    await clickNav(el, 'Logs')
    // project=null → 只剩宿主；提示的不是空列表。
    expect(el.textContent).toContain('app_start')
    expect(el.textContent).toContain('dev_file')
    expect(el.textContent).not.toContain('gears_removed')
    expect(el.textContent).not.toContain('autonomy_allow')
    await clickNav(el, 'Decision')
    expect(el.textContent).toContain('Open a project first')
    // 宿主类仍能看。
    await clickNav(el, 'Host')
    expect(el.textContent).toContain('app_start')
    root.unmount()
  })

  it('只有记录列表滚动：开关与筛选条钉在滚动区外', async () => {
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Logs')
    // 滚动容器 = 含记录行且 overflowY:auto 的那一层。
    const scroller = [...el.querySelectorAll('div')].find(
      (d) => d.style.overflowY === 'auto' && d.textContent?.includes('app_start'),
    )!
    expect(scroller).toBeTruthy()
    // 开关和筛选按钮不在滚动区里。
    expect(scroller.querySelector('input[type=checkbox]')).toBeNull()
    const refresh = [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Refresh')!
    expect(scroller.contains(refresh)).toBe(false)
    // 记录行在滚动区里。
    const row = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('app_start'))!
    expect(scroller.contains(row)).toBe(true)
    root.unmount()
  })

  it('拨动开关发 set_log_enabled 并重拉记录', async () => {
    const setSpy = vi.spyOn(api, 'setLogEnabled').mockResolvedValue(undefined)
    const { el, root } = await render(<SettingsPage onBack={() => {}} />)
    await clickNav(el, 'Logs')
    const box = el.querySelector<HTMLInputElement>('input[type=checkbox]')!
    await act(async () => {
      box.click()
      // onChange 是异步链（setLogEnabled → refresh）——给一轮微任务再断言，
      // 否则 setRows 落在 act 外。
      await new Promise((r) => setTimeout(r, 0))
    })
    expect(setSpy).toHaveBeenCalledWith(false)
    root.unmount()
  })
})
