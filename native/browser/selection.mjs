/** Owner-only page picker. Each exported function is self-contained so the fixed
 * browser worker can serialize it into its Playwright MCP execution sandbox.
 * Page candidates are untrusted; Rust supplies scope and persists references. */
export async function installSelection(page, scope, labels = {}, sourceBundle = null) {
  if (sourceBundle && ['localhost','127.0.0.1','[::1]'].includes(new URL(page.url()).hostname)) {
    await page.evaluate(sourceBundle)
  }
  return page.evaluate(({ scope, labels }) => {
    const key = '__hexagonElementPicker'
    const previous = window[key]
    if (previous?.scope?.session_id === scope.session_id && previous.scope.navigation_generation === scope.navigation_generation) {
      previous.labels(labels)
      if (scope.active) previous.activate('element')
      return { installed: true }
    }
    previous?.dispose?.()
    const host = document.createElement('div')
    host.setAttribute('data-hexagon-picker', '')
    host.setAttribute('aria-label', String(labels.element ?? 'Select element').slice(0,80))
    Object.assign(host.style, { position: 'fixed', right: '20px', bottom: '20px', zIndex: '2147483647' })
    const shadow = host.attachShadow({ mode: 'closed' })
    const style = document.createElement('style')
    style.textContent = ':host{all:initial}section{display:flex;gap:5px;padding:6px;border-radius:10px;background:#202127;box-shadow:0 2px 14px #0005;font:12px system-ui;color:#fff}button{font:inherit;border:1px solid #ffffff44;border-radius:6px;padding:8px;background:transparent;color:inherit;cursor:pointer}button[aria-pressed=true]{background:#485fa7}span{align-self:center;max-width:170px}'
    const bar = document.createElement('section')
    const pick = document.createElement('button')
    const region = document.createElement('button')
    const done = document.createElement('button')
    const hint = document.createElement('span')
    pick.textContent = String(labels.element ?? 'Select element').slice(0, 80)
    region.textContent = String(labels.region ?? 'Select region').slice(0, 80)
    done.textContent = String(labels.done ?? 'Done').slice(0, 80)
    bar.append(pick, region, done, hint); shadow.append(style, bar); document.documentElement.append(host)
    let mode = null
    let start = null
    const boxes = []
    const state = { scope, queue: [], pending: [], labels: value => {
      labels = value; host.setAttribute('aria-label', String(labels.element ?? 'Select element').slice(0,80)); pick.textContent = String(labels.element ?? 'Select element').slice(0,80); region.textContent = String(labels.region ?? 'Select region').slice(0,80); done.textContent = String(labels.done ?? 'Done').slice(0,80);
    }, activate: value => {
      mode = value; state.active = Boolean(value); pick.setAttribute('aria-pressed', String(value === 'element')); region.setAttribute('aria-pressed', String(value === 'region'))
      hint.textContent = value ? String(labels.hint ?? 'Select, then send in Hexagon').slice(0, 100) : ''
    }, dispose: () => {
      host.remove(); boxes.forEach(box => box.remove())
      document.removeEventListener('click', click, true)
      document.removeEventListener('pointerdown', down, true)
      document.removeEventListener('pointerup', up, true)
      document.removeEventListener('keydown', keydown, true)
      if (window[key] === state) delete window[key]
    } }
    const limit = value => Math.round(Math.max(0, value))
    const clipped = rect => {
      const x = limit(Math.min(innerWidth - 1, rect.x)); const y = limit(Math.min(innerHeight - 1, rect.y))
      return { x, y, width: Math.min(1024, innerWidth - x, Math.max(1, limit(rect.width))), height: Math.min(768, innerHeight - y, Math.max(1, limit(rect.height))) }
    }
    const enqueue = (target, rectangle, kind) => {
      if (state.queue.length >= 8) return
      const rect = clipped(rectangle)
      let text = '', selector = '', tag = '', role = '', source_hint = null
      if (kind === 'element' && target instanceof Element) {
        tag = target.localName.slice(0, 40)
        role = (target.getAttribute('role') ?? '').slice(0, 40)
        // Never inspect input value/defaultValue, storage or whole-page HTML.
        // Text nodes under forms/editable/private containers are excluded before
        // traversal; tree walking is bounded to avoid giant page selection work.
        const blocked = 'input,textarea,select,[contenteditable],script,style,noscript,[data-private],[autocomplete]'
        if (!target.closest(blocked)) {
          const walker = document.createTreeWalker(target, NodeFilter.SHOW_TEXT)
          let node, count = 0
          while ((node = walker.nextNode()) && count++ < 120 && text.length < 600) {
            if (!node.parentElement?.closest(blocked)) text += ` ${node.textContent ?? ''}`.slice(0, 600 - text.length)
          }
          text = text.replace(/\s+/g, ' ').trim()
        }
        const parts = []; let ancestor = target
        for (let depth = 0; ancestor && depth < 5; depth++, ancestor = ancestor.parentElement) {
          const index = ancestor.parentElement ? [...ancestor.parentElement.children].indexOf(ancestor) + 1 : 1
          parts.unshift(`${ancestor.localName}:nth-child(${index})`)
        }
        selector = parts.join(' > ').slice(0, 400)
        // React's development source is only a hint. Do not call React Grab's
        // full getElementContext: it also captures HTML/values/styles. A host
        // canonicalization check decides whether this hint names a local file.
        const fiberKey = Object.keys(target).find(name => name.startsWith('__reactFiber$'))
        let fiber = fiberKey ? target[fiberKey] : null
        for (let depth = 0; fiber && depth < 12; depth++, fiber = fiber.return) {
          const source = fiber._debugSource
          if (source && typeof source.fileName === 'string' && Number.isSafeInteger(source.lineNumber)) {
            source_hint = { file: source.fileName.slice(0, 400), line: source.lineNumber }; break
          }
        }
      }
      const candidate = { kind, tag, role, text, selector, rect, source_hint }
      state.queue.push(candidate)
      // Issue13: use only core stack locations; the full React Grab context API
      // includes page HTML. Optional local-dev source lookup cannot auto-send.
      if (kind === 'element' && !source_hint && window.__hexagonReactSource) {
        state.pending.push(Promise.resolve(window.__hexagonReactSource.sourceForElement(target)).then(hint => { candidate.source_hint = hint }).catch(() => {}))
      }
      const box = document.createElement('div'); box.setAttribute('data-hexagon-picker', '')
      Object.assign(box.style, { position: 'fixed', pointerEvents: 'none', zIndex: '2147483646', boxSizing: 'border-box', border: '2px solid #719aff', left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` })
      document.documentElement.append(box); boxes.push(box)
      if (boxes.length > 8) boxes.shift().remove()
    }
    const isOwn = event => event.composedPath().includes(host)
    const click = event => {
      if (!mode || isOwn(event)) return
      event.preventDefault(); event.stopImmediatePropagation()
      if (mode !== 'element') return
      const target = event.composedPath().find(node => node instanceof Element)
      if (!target) return
      const kind = ['iframe', 'canvas', 'video'].includes(target.localName) || target.shadowRoot ? 'region' : 'element'
      enqueue(target, target.getBoundingClientRect(), kind)
    }
    const down = event => {
      if (mode !== 'region' || isOwn(event)) return
      event.preventDefault(); event.stopImmediatePropagation(); start = { x: event.clientX, y: event.clientY }
    }
    const up = event => {
      if (mode !== 'region' || !start) return
      event.preventDefault(); event.stopImmediatePropagation()
      enqueue(null, { x: Math.min(start.x, event.clientX), y: Math.min(start.y, event.clientY), width: Math.abs(start.x - event.clientX), height: Math.abs(start.y - event.clientY) }, 'region')
      start = null
    }
    const keydown = event => { if (mode && event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); state.activate(null) } }
    pick.onclick = () => state.activate(mode === 'element' ? null : 'element')
    region.onclick = () => state.activate(mode === 'region' ? null : 'region')
    done.onclick = () => state.activate(null)
    document.addEventListener('click', click, true); document.addEventListener('pointerdown', down, true); document.addEventListener('pointerup', up, true); document.addEventListener('keydown', keydown, true)
    window[key] = state
    if (scope.active) state.activate('element')
    return { installed: true }
  }, { scope, labels })
}

export async function pollSelection(page, scope) {
  const candidates = await page.evaluate(async scope => {
    const state = window.__hexagonElementPicker
    if (!state || state.scope.session_id !== scope.session_id || state.scope.navigation_generation !== scope.navigation_generation) return []
    await Promise.allSettled(state.pending.splice(0))
    return state.queue.splice(0, 8)
  }, scope)
  const output = []
  for (const candidate of candidates) {
    const rect = candidate.rect
    if (!rect || ![rect.x, rect.y, rect.width, rect.height].every(Number.isFinite) || rect.x < 0 || rect.y < 0 || rect.width < 1 || rect.height < 1 || rect.width > 1024 || rect.height > 768) continue
    const screenshot = await page.screenshot({ type: 'png', clip: rect, mask: page.frames().map(frame => frame.locator('input,textarea,select,[contenteditable],[data-private],[autocomplete]')), style: '[data-hexagon-picker]{visibility:hidden!important}', timeout: 5000 })
    if (screenshot.length > 1024 * 1024) continue
    output.push({ ...candidate, png_base64: screenshot.toString('base64') })
  }
  return { candidates: output }
}

export async function stopSelection(page, _scope) {
  return page.evaluate(() => { window.__hexagonElementPicker?.dispose(); return { stopped: true } })
}
