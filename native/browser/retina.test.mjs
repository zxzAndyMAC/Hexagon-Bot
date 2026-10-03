import test from 'node:test';
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { pageOperation } from './page.mjs';

test('Retina preview and observation keep device pixels without changing the focused page', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 900, height: 700 }, deviceScaleFactor: 2 });
    const page = await context.newPage();
    await page.setContent('<title>Retina fixture</title><input value="draft"><h1>Stable page</h1>');
    await page.locator('input').focus();
    await page.evaluate(() => {
      window.captureMutations = [];
      new MutationObserver(records => window.captureMutations.push(...records.map(record => record.attributeName)))
        .observe(document.querySelector('input'), { attributes: true });
    });
    // Owner incident 2026-10-02: CSS-scaled capture paints a half-size surface
    // in the live macOS window. The Retina GUI red/green probe is in scratch;
    // actual capture dimensions guard both production callers against relapse.
    for (const op of ['preview', 'observe']) {
      const result = await pageOperation(page, { op, deadline: Date.now() + 10000, new_tab_id: 'retina', new_snapshot_id: 'bs1-retina' });
      assert.equal(result.__host_error, undefined);
      const data = op === 'preview' ? result.data_url : `data:image/png;base64,${result.image_base64}`;
      const dimensions = await page.evaluate(async data => {
        const bitmap = await createImageBitmap(await (await fetch(data)).blob());
        const size = [bitmap.width, bitmap.height]; bitmap.close(); return size;
      }, data);
      assert.deepEqual(dimensions, [1800, 1400], `${op} must not scale the browser capture surface`);
      assert.deepEqual(await page.evaluate(() => ({ width: innerWidth, height: innerHeight, focus: document.activeElement.tagName, mutations: window.captureMutations })),
        { width: 900, height: 700, focus: 'INPUT', mutations: [] });
    }
  } finally { await browser.close(); }
});
