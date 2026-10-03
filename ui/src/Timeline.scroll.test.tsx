import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import './i18n'
import { Timeline } from './components/Timeline'
import { useUiStore } from './store'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ scrollerRef }: { scrollerRef: (node: HTMLElement | null) => void }) =>
    <div ref={scrollerRef} data-scroller />,
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('virtualizer position corrections cannot recursively trigger tail writes', async () => {
  useUiStore.setState({ timeline: [], pending: [], team: [], streams: {}, thinkings: {}, plans: {}, streamDone: {} })
  const el = document.createElement('div')
  const root = createRoot(el)
  await act(async () => root.render(<Timeline />))
  const scroller = el.querySelector('[data-scroller]') as HTMLElement
  Object.defineProperties(scroller, { scrollHeight: { value: 1000 }, clientHeight: { value: 200 } })
  let writes = 0
  Object.defineProperty(scroller, 'scrollTop', {
    get: () => 500,
    set: () => {
      // WebKit/Virtuoso can correct a write after the synchronous ownScroll guard
      // has reset. Bound this fixture so the broken implementation fails, not hangs.
      if (++writes < 5) queueMicrotask(() => scroller.dispatchEvent(new Event('scroll')))
    },
  })
  await act(async () => { scroller.dispatchEvent(new Event('scroll')) })
  expect(writes).toBe(0)
  await act(async () => root.unmount())
})
