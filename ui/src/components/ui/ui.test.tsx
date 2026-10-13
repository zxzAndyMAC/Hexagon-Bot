// ui-redesign 票 01：基础组件（components A1–A17）的外部可见行为——可读名、键盘、禁用仍可聚焦、
// 加载不可再点、模态拦背景裁决快捷键。视觉取值由标本页 + 原型截图比对验收，不在这里断言 CSS。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { Button } from './button'
import { IconButton } from './icon-button'
import { Segmented, SegmentedItem } from './toggle-group'
import { Switch } from './switch'
import { Checkbox } from './checkbox'
import { RadioGroup, RadioGroupItem } from './radio-group'
import { Field, Input } from './input'
import { CodeChip } from './code-chip'
import { Progress } from './progress'
import { Avatar } from './avatar'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from './dialog'
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogTitle } from './alert-dialog'
import { TooltipProvider } from './tooltip'
import { formatBinding, isMac } from '@/keymap'
import { agentSwatch } from '@/colors'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: Root | null = null
let host: HTMLElement

async function render(node: React.ReactNode) {
  host = document.createElement('div')
  document.body.appendChild(host)
  root = createRoot(host)
  await act(async () => { root!.render(<TooltipProvider delayDuration={0}>{node}</TooltipProvider>) })
  return host
}

const $ = <T extends Element = HTMLElement>(sel: string) => document.querySelector<T>(sel)!
const click = async (el: Element) => { await act(async () => { (el as HTMLElement).click() }) }
const keydown = async (el: EventTarget, key: string, init: KeyboardEventInit = {}) => {
  await act(async () => { el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })) })
}

beforeEach(() => localStorage.clear())
afterEach(async () => {
  await act(async () => root?.unmount())
  root = null
  document.body.innerHTML = ''
})

describe('Button（A1）', () => {
  it('禁用：aria-disabled 而非原生 disabled——仍可聚焦读原因，点击不触发', async () => {
    const onClick = vi.fn()
    await render(<Button disabled onClick={onClick}>发布</Button>)
    const b = $<HTMLButtonElement>('button')
    expect(b.disabled).toBe(false)
    expect(b.getAttribute('aria-disabled')).toBe('true')
    b.focus()
    expect(document.activeElement).toBe(b)
    await click(b)
    expect(onClick).not.toHaveBeenCalled()
  })

  it('加载：前置环形、aria-busy、不可再点，但不显示成禁用', async () => {
    const onClick = vi.fn()
    await render(<Button variant="primary" loading onClick={onClick}>保存中</Button>)
    const b = $('button')
    expect(b.getAttribute('aria-busy')).toBe('true')
    expect(b.hasAttribute('aria-disabled')).toBe(false)
    expect(b.querySelector('[data-slot="spinner"]')).not.toBeNull()
    await click(b)
    expect(onClick).not.toHaveBeenCalled()
  })

  it('默认 type=button，不会误提交表单', async () => {
    const onSubmit = vi.fn((e: React.FormEvent) => e.preventDefault())
    await render(<form onSubmit={onSubmit}><Button>次操作</Button></form>)
    await click($('button'))
    expect(onSubmit).not.toHaveBeenCalled()
  })
})

describe('IconButton（A2）', () => {
  it('可读名 + tooltip 显示用户当前绑定（ADR 0051），开态 aria-pressed', async () => {
    localStorage.setItem('hexagon.key.toggleSidebar', 'mod+shift+j')
    await render(<IconButton label="开关侧栏" action="toggleSidebar" on>x</IconButton>)
    const b = $('button[aria-label="开关侧栏"]')
    expect(b.getAttribute('aria-pressed')).toBe('true')
    await act(async () => { b.focus() })
    const tip = await vi.waitFor(() => {
      const t = document.querySelector('[data-slot="tooltip-content"]')
      if (!t) throw new Error('tooltip not open')
      return t
    })
    expect(tip.textContent).toContain('开关侧栏')
    expect(tip.textContent).toContain(formatBinding('mod+shift+j'))
  })

  it('禁用时 tooltip 写原因，点击不触发', async () => {
    const onClick = vi.fn()
    await render(<IconButton label="检测" disabled disabledReason="先填写 API Key" onClick={onClick}>x</IconButton>)
    const b = $('button[aria-label="检测"]')
    await act(async () => { b.focus() })
    const tip = await vi.waitFor(() => {
      const t = document.querySelector('[data-slot="tooltip-content"]')
      if (!t) throw new Error('tooltip not open')
      return t
    })
    expect(tip.textContent).toContain('先填写 API Key')
    await click(b)
    expect(onClick).not.toHaveBeenCalled()
  })
})

describe('Segmented（A7）', () => {
  it('单选必有一项：点别的项切换，再点当前项不取消', async () => {
    const onChange = vi.fn()
    await render(
      <Segmented value="all" onValueChange={onChange} aria-label="过滤">
        <SegmentedItem value="all">全部</SegmentedItem>
        <SegmentedItem value="msg">消息</SegmentedItem>
      </Segmented>,
    )
    const [all, msg] = [...document.querySelectorAll('[data-slot="segmented-item"]')]
    expect(all.getAttribute('data-state')).toBe('on')
    await click(msg)
    expect(onChange).toHaveBeenLastCalledWith('msg')
    onChange.mockClear()
    await click(all)
    expect(onChange).not.toHaveBeenCalled()
  })
})

describe('Switch（A10）', () => {
  it('切换回调；加载中不乐观翻转、不可点', async () => {
    const onChange = vi.fn()
    await render(<><Switch aria-label="托管" checked={false} onCheckedChange={onChange} /><Switch aria-label="托管2" checked={false} loading onCheckedChange={onChange} /></>)
    await click($('[aria-label="托管"]'))
    expect(onChange).toHaveBeenCalledWith(true)
    onChange.mockClear()
    const busy = $('[aria-label="托管2"]')
    expect(busy.getAttribute('aria-busy')).toBe('true')
    await click(busy)
    expect(onChange).not.toHaveBeenCalled()
    expect(busy.getAttribute('data-state')).toBe('unchecked')
  })
})

describe('Checkbox（A11）', () => {
  it('点击勾选；必勾未勾透传 aria-invalid', async () => {
    const onChange = vi.fn()
    await render(<Checkbox aria-label="我了解" aria-invalid onCheckedChange={onChange} />)
    const c = $('[aria-label="我了解"]')
    expect(c.getAttribute('aria-invalid')).toBe('true')
    await click(c)
    expect(onChange).toHaveBeenCalledWith(true)
  })
})

describe('禁用仍可聚焦（components §0）', () => {
  it('Switch / Checkbox / Radio / 分段单项禁用：可聚焦、aria-disabled、点击不切换', async () => {
    const onChange = vi.fn()
    await render(
      <>
        <Switch aria-label="s" disabled checked={false} onCheckedChange={onChange} />
        <Checkbox aria-label="c" disabled onCheckedChange={onChange} />
        <RadioGroup aria-label="r" disabled value="a" onValueChange={onChange}><RadioGroupItem value="b" aria-label="rb" /></RadioGroup>
        <Segmented value="a" onValueChange={onChange} aria-label="g">
          <SegmentedItem value="a">A</SegmentedItem>
          <SegmentedItem value="b" disabled disabledReason="只有一个版本">B</SegmentedItem>
        </Segmented>
      </>,
    )
    const segB = [...document.querySelectorAll('[data-slot="segmented-item"]')][1]
    for (const el of [$('[aria-label="s"]'), $('[aria-label="c"]'), $('[aria-label="rb"]'), segB as HTMLElement]) {
      expect(el.getAttribute('aria-disabled')).toBe('true')
      expect((el as HTMLButtonElement).disabled).toBe(false)
      await act(async () => { (el as HTMLElement).focus() })
      expect(document.activeElement).toBe(el)
      await click(el)
    }
    expect(onChange).not.toHaveBeenCalled()
    expect($('[aria-label="c"]').getAttribute('data-state')).toBe('unchecked')
  })
})

describe('Field / Input（A9）', () => {
  it('错误：role=alert 写原因、控件 aria-invalid；加载：后置位进行时文案', async () => {
    await render(
      <>
        <Field label="API Key" htmlFor="k" error="密钥格式不对"><Input id="k" aria-invalid /></Field>
        <Input aria-label="b" loadingLabel="检测中…" />
      </>,
    )
    expect($('[role="alert"]').textContent).toBe('密钥格式不对')
    expect($('#k').getAttribute('aria-invalid')).toBe('true')
    expect($('#k').getAttribute('aria-describedby')).toBe($('[role="alert"]').id)
    expect($('label').getAttribute('for')).toBe('k')
    expect($('[aria-label="b"]').getAttribute('aria-busy')).toBe('true')
    expect(host.textContent).toContain('检测中…')
  })
})

describe('CodeChip（A5）', () => {
  it('可点击时是按钮；路径已不存在时 title 写原因', async () => {
    const onClick = vi.fn()
    await render(<><CodeChip file added={3} removed={1} onClick={onClick}>src/a.rs</CodeChip><CodeChip missing missingLabel="文件已移除">src/b.rs</CodeChip></>)
    const btn = $('button[data-slot="code-chip"]')
    expect(btn.textContent).toBe('src/a.rs+3−1')
    await click(btn)
    expect(onClick).toHaveBeenCalled()
    expect($('span[data-slot="code-chip"]').getAttribute('title')).toBe('文件已移除')
  })
})

describe('Progress（A12）', () => {
  it('数值钳到 0–100；加载时不出填充', async () => {
    await render(<><Progress aria-label="p" value={140} /><Progress aria-label="q" loading /></>)
    expect($('[aria-label="p"]').getAttribute('aria-valuenow')).toBe('100')
    expect($('[aria-label="q"] [data-slot="progress-indicator"]')).toBeNull()
  })
})

describe('Avatar（A13）', () => {
  it('首字 + 按 agent id 定 c1–c8；负责人固定 c8；状态点', async () => {
    await render(<><Avatar agentId="pm-1" name="产品" status="online" /><Avatar owner name="我" /></>)
    const [agent, owner] = [...document.querySelectorAll('[data-slot="avatar"]')]
    expect(agent.textContent).toBe('产')
    expect(agent.getAttribute('data-swatch')).toBe(String(agentSwatch('pm-1')))
    expect(owner.getAttribute('data-swatch')).toBe('8')
    expect($('[data-slot="avatar-status"]').getAttribute('data-status')).toBe('online')
  })
})

describe('Dialog / AlertDialog（A17）', () => {
  it('打开期间背景裁决快捷键（⌘↵ 批准）被拦，关掉后恢复', async () => {
    const background = vi.fn()
    window.addEventListener('keydown', background)
    try {
      await render(
        <Dialog open>
          <DialogContent><DialogTitle>放大</DialogTitle><DialogDescription>图</DialogDescription></DialogContent>
        </Dialog>,
      )
      await keydown(document.body, 'Enter', isMac ? { metaKey: true } : { ctrlKey: true })
      expect(background).not.toHaveBeenCalled()
      await act(async () => root!.unmount())
      root = null
      await keydown(document.body, 'Enter', isMac ? { metaKey: true } : { ctrlKey: true })
      expect(background).toHaveBeenCalled()
    } finally {
      window.removeEventListener('keydown', background)
    }
  })

  it('确认层首焦落在「取消」；提交中 Esc 不关', async () => {
    const onOpenChange = vi.fn()
    await render(
      <AlertDialog open onOpenChange={onOpenChange}>
        <AlertDialogContent busy>
          <AlertDialogTitle>发布？</AlertDialogTitle>
          <AlertDialogDescription>不可撤回</AlertDialogDescription>
          <AlertDialogCancel>取消</AlertDialogCancel>
          <AlertDialogAction>发布</AlertDialogAction>
          <AlertDialogAction variant="danger" loading>发布中</AlertDialogAction>
        </AlertDialogContent>
      </AlertDialog>,
    )
    expect(document.activeElement?.textContent).toContain('取消')
    await keydown(document.activeElement!, 'Escape')
    expect(onOpenChange).not.toHaveBeenCalled()
    // 提交中所有按钮禁用；正在提交的主按钮显示加载而非禁用
    const buttons = [...document.querySelectorAll('[data-slot="alert-dialog-content"] button')]
    expect(buttons.map((b) => [b.getAttribute('aria-disabled'), b.getAttribute('aria-busy')])).toEqual([
      ['true', null], ['true', null], [null, 'true'],
    ])
  })
})
