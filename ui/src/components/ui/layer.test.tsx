// gap C5（2026-10-15，foundations §4「模态内弹层」）：弹层 20 / tooltip 25 低于模态 30；从模态里打开的浮层抬到 35 / 36，
// 否则被模态本身盖住（gap B3 在向导里实测过：打开了但看不见）。
import { afterEach, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { Dialog, DialogContent, DialogTitle } from './dialog'
import { Popover, PopoverContent, PopoverTrigger } from './popover'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from './tooltip'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: Root | null = null
afterEach(async () => {
  await act(async () => root?.unmount())
  root = null
  document.body.innerHTML = ''
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.append(el)
  root = createRoot(el)
  await act(async () => root!.render(<TooltipProvider>{node}</TooltipProvider>))
}

const popover = (
  <Popover open>
    <PopoverTrigger>open</PopoverTrigger>
    <PopoverContent data-testid="pop">menu</PopoverContent>
  </Popover>
)
const tooltip = (
  <Tooltip open>
    <TooltipTrigger>t</TooltipTrigger>
    <TooltipContent data-testid="tip">hint</TooltipContent>
  </Tooltip>
)

it('页面上的弹层与 tooltip 用默认 z 20 / 25', async () => {
  await render(<>{popover}{tooltip}</>)
  expect(document.querySelector('[data-testid="pop"]')!.className).toContain('z-20')
  expect(document.querySelector('[data-testid="tip"]')!.className).toContain('z-[25]')
})

it('模态里打开的弹层与 tooltip 抬到 35 / 36，压过模态 30', async () => {
  await render(
    <Dialog open>
      <DialogContent aria-describedby={undefined}>
        <DialogTitle>d</DialogTitle>
        {popover}{tooltip}
      </DialogContent>
    </Dialog>,
  )
  const pop = document.querySelector('[data-testid="pop"]')!.className
  expect(pop).toContain('z-[35]')
  expect(pop).not.toContain('z-20')
  expect(document.querySelector('[data-testid="tip"]')!.className).toContain('z-[36]')
})
