// Trusted, self-contained helper executed only by the host wrapper. No caller
// supplies code, selectors, file paths, cookies, storage or arbitrary MCP tools.
export async function pageOperation(page, input) {
  let dispatched = false;
  try {
  if (!page.__hexagonBrowser) {
    page.__hexagonBrowser = { tab_id: input.new_tab_id, navigation_generation: 0, targets: new Map(), console: [], network: [], snapshot: null };
    const state = page.__hexagonBrowser;
    page.on('framenavigated', frame => { if (frame === page.mainFrame()) { state.navigation_generation++; state.snapshot = null; for (const handle of state.targets.values()) void handle.dispose().catch(() => {}); state.targets.clear(); } });
    page.on('console', message => { state.console.push({ level: message.type(), text: message.text().slice(0, 1000) }); if (state.console.length > 40) state.console.shift(); });
    page.on('requestfailed', request => { let url; try { const u = new URL(request.url()); u.search = ''; u.hash = ''; url = u.href; } catch { url = ''; } state.network.push({ url, method: request.method(), error: String(request.failure()?.errorText || '').slice(0, 200) }); if (state.network.length > 40) state.network.shift(); });
    page.on('download', download => { void download.cancel(); });
  }
  const state = page.__hexagonBrowser;
  const metadata = async () => ({ tab_id: state.tab_id, navigation_generation: state.navigation_generation, url: page.url().slice(0, 2048), title: (await page.title()).slice(0, 256) });
  if (input.tab_id && (input.tab_id !== state.tab_id || (!['metadata', 'observe'].includes(input.op) && input.navigation_generation !== state.navigation_generation))) throw new Error('NOT_EXECUTED: browser target changed; observe again');
  if (input.deadline < Date.now()) throw new Error('NOT_EXECUTED: request deadline expired');
  if (input.op === 'metadata') return metadata();
  if (input.op === 'focus') { await page.bringToFront(); return metadata(); }
  if (input.op === 'preview') {
    // Owner incident 2026-10-02: on Retina macOS, CSS-scaled capture briefly
    // paints a half-size surface over the live page's upper-left corner. Keep
    // device pixels (and the caret); byte budgets still bound the returned image.
    // Do not restore scale:'css' to reduce payloads: it changes visible capture.
    const image = await page.screenshot({ type: 'jpeg', quality: 35, timeout: 3000, scale: 'device', caret: 'initial' });
    if (image.length > 1000000) throw new Error('NOT_EXECUTED: preview frame exceeds bound');
    return { ...await metadata(), data_url: 'data:image/jpeg;base64,' + image.toString('base64'), captured_at: Date.now() };
  }
  if (input.op === 'detach') {
    state.snapshot = null;
    if (input.mode === 'managed') await page.context().browser().close();
    // Extension: do not call page/context/browser.close. The worker process
    // exits and disconnects its debugger transport, preserving personal tabs.
    return { detached: true };
  }
  if (input.op === 'diagnostics') return { ...await metadata(), console: state.console.slice(-20), failed_requests: state.network.slice(-20), page_content_untrusted: true };
  if (input.op === 'observe') {
    for (const handle of state.targets.values()) await handle.dispose().catch(() => {});
    state.targets.clear();
    const generation = state.navigation_generation;
    const handles = await page.$$('button,a,input,select,textarea,summary,[role="button"],[role="link"],[role="checkbox"]');
    const elements = [];
    for (const handle of handles.slice(0, 200)) {
      const info = await handle.evaluate(element => {
        const r = element.getBoundingClientRect();
        if (!r.width || !r.height || r.bottom < 0 || r.top > innerHeight) return null;
        const input = /^(INPUT|TEXTAREA|SELECT)$/.test(element.tagName);
        const password = element.matches('input[type=password],input[type=file]');
        return { tag: element.tagName.toLowerCase(), role: element.getAttribute('role'), name: (() => {
          const blocked = 'input,textarea,select,[contenteditable],[data-private],[autocomplete],script,style,noscript';
          if (element.closest('[contenteditable],[data-private]')) return '';
          if (input) return (element.getAttribute('aria-label') || element.getAttribute('placeholder') || '').trim().slice(0,160);
          const label = element.getAttribute('aria-label'); if (label) return label.trim().slice(0,160);
          const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT); let node, text = '', count = 0;
          while ((node = walker.nextNode()) && count++ < 200 && text.length < 160) if (!node.parentElement?.closest(blocked)) text += (node.textContent || '').slice(0,160-text.length);
          return text.trim();
        })(), protected: password };
      }).catch(() => null);
      if (!info) { await handle.dispose(); continue; }
      const id = 'be' + elements.length;
      state.targets.set(id, handle);
      elements.push({ element_id: id, ...info });
    }
    for (const handle of handles.slice(200)) await handle.dispose();
    const text = await page.evaluate(() => {
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT);
      let node, value = '', count = 0;
      while ((node = walker.nextNode()) && count++ < 2000 && value.length < 16000) {
        if (node.parentElement?.closest('script,style,noscript,input,textarea,select,[contenteditable],[data-private],[autocomplete]')) continue;
        if (!node.parentElement?.getClientRects().length) continue;
        value += node.textContent.trim() + '\n';
      }
      return value.slice(0, 16000);
    });
    // Same live-window capture boundary as preview: never CSS-scale the surface.
    const image = await page.screenshot({ type: 'png', timeout: 5000, scale: 'device', caret: 'initial' });
    if (image.length > 5000000 || generation !== state.navigation_generation) throw new Error('NOT_EXECUTED: observation changed or exceeds bound');
    state.snapshot = { id: input.new_snapshot_id, expires: Date.now() + 30000, generation };
    return { ...await metadata(), snapshot_id: state.snapshot.id, image_base64: image.toString('base64'), elements, text, page_content_untrusted: true };
  }
  if (await page.evaluate(() => Boolean(window.__hexagonElementPicker?.active))) throw new Error('NOT_EXECUTED: owner is selecting page elements');
  if (!state.snapshot || state.snapshot.id !== input.snapshot_id || state.snapshot.expires < Date.now() || state.snapshot.generation !== state.navigation_generation) throw new Error('NOT_EXECUTED: stale or consumed browser snapshot');
  state.snapshot = null;
  if (input.op === 'navigate') {
    const url = new URL(input.url);
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('NOT_EXECUTED: unsupported browser URL');
    dispatched = true;
    await page.goto(url.href, { waitUntil: 'domcontentloaded', timeout: 15000 });
  } else if (input.op === 'scroll') {
    dispatched = true;
    await page.mouse.wheel(0, (input.direction === 'up' ? -1 : 1) * Math.min(input.amount, 5) * 400);
  } else {
    const target = state.targets.get(input.element_id);
    if (!target) throw new Error('NOT_EXECUTED: element was not observed');
    const protectedField = await target.evaluate(element => element.matches('input[type=password],input[type=file]') || element.closest('[data-hexagon-selection]'));
    if (protectedField) throw new Error('NOT_EXECUTED: protected input');
    dispatched = true;
    if (input.op === 'click') await target.click({ timeout: 5000 });
    else if (input.op === 'type') await target.fill(input.text, { timeout: 5000 });
    else if (input.op === 'key') await target.press(input.key, { timeout: 5000 });
    else throw new Error('NOT_EXECUTED: unknown browser operation');
  }
  return { ...await metadata(), dispatched: true, requires_observation: true };
  } catch (error) { return { __host_error: { message: String(error.message || error).slice(0, 1000), dispatched } }; }
}
