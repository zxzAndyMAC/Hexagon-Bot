import test from 'node:test'
import assert from 'node:assert/strict'
import { chromium } from 'playwright'
import { installSelection, pollSelection, stopSelection } from './selection.mjs'

const scope = { session_id: 'test-session', tab_id: 'tab-one', navigation_generation: 1, active: true }
test('picker queues bounded owner selections without sending or reading input values', async () => {
  const browser = await chromium.launch({ headless: true })
  try {
    const page = await browser.newPage({ viewport: { width: 900, height: 700 } })
    await page.setContent('<main id="card"><h1>Safe heading</h1><p>Visible explanation</p><input type="password" value="SUPER_SECRET"><textarea>PRIVATE_TEXT</textarea><div contenteditable>PRIVATE_EDIT</div><button id="target">Public action</button></main>')
    await installSelection(page, scope, { element: '选择网页元素', region: '框选区域', done: '完成', hint: '选择后发送' })
    assert.equal(await page.evaluate(() => window.__hexagonElementPicker.active), true)
    await page.locator('#card').click({ position: { x: 4, y: 4 } })
    await page.locator('#target').click()
    const selected = await pollSelection(page, scope)
    assert.equal(selected.candidates.length, 2)
    const text = JSON.stringify(selected.candidates.map(({ png_base64, ...candidate }) => candidate))
    assert.match(text, /Safe heading/)
    assert.doesNotMatch(text, /SUPER_SECRET|PRIVATE_TEXT|PRIVATE_EDIT/)
    assert.doesNotMatch(text, /<input|<textarea|outerHTML|innerHTML/)
    for (const candidate of selected.candidates) {
      assert.equal(candidate.kind, 'element')
      assert.ok(Buffer.from(candidate.png_base64, 'base64').subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10])))
    }
    assert.equal((await pollSelection(page, scope)).candidates.length, 0)
    await stopSelection(page, scope)
    assert.equal(await page.locator('[data-hexagon-picker]').count(), 0)
  } finally { await browser.close() }
})

test('region fallback contains no DOM/source and a new navigation cannot consume old picks', async () => {
  const browser = await chromium.launch({ headless: true })
  try {
    const page = await browser.newPage({ viewport: { width: 900, height: 700 } })
    await page.setContent('<canvas width="200" height="100"></canvas>')
    await installSelection(page, scope)
    await page.locator('canvas').click()
    const picked = await pollSelection(page, scope)
    assert.equal(picked.candidates[0].kind, 'region')
    for (const key of ['tag','role','text','selector']) assert.equal(picked.candidates[0][key], '')
    assert.equal(picked.candidates[0].source_hint, null)
    await page.evaluate(() => window.__hexagonElementPicker.activate('region'))
    await page.mouse.move(250,150); await page.mouse.down(); await page.mouse.move(400,260); await page.mouse.up()
    assert.equal((await pollSelection(page,{...scope,navigation_generation:2})).candidates.length,0)
    assert.equal((await pollSelection(page,scope)).candidates.length,1)
    await page.goto('about:blank')
    assert.equal((await pollSelection(page,scope)).candidates.length,0)
  } finally { await browser.close() }
})

test('scrolled page region screenshot matches the selected element and refreshed labels', async () => {
  const browser = await chromium.launch({ headless: true })
  try {
    const page = await browser.newPage({ viewport: { width: 900, height: 700 } })
    await page.setContent('<style>body{margin:0;height:2600px}#target{position:absolute;top:1600px;left:50px;width:120px;height:80px;background:rgb(255,0,0)}</style><div id="target"></div>')
    await page.evaluate(() => scrollTo(0,1500))
    await installSelection(page,{...scope,active:false})
    await installSelection(page,scope,{element:'选择网页元素',region:'框选区域',done:'完成',hint:'选择后发送'})
    // Closed overlay exposes its visible text via accessibility only.
    assert.equal(await page.locator('[data-hexagon-picker]').getAttribute('aria-label'), '选择网页元素')
    await page.locator('#target').click()
    const selected = (await pollSelection(page,scope)).candidates[0]
    const expected = await page.locator('#target').screenshot({style:'[data-hexagon-picker]{visibility:hidden!important}'})
    assert.deepEqual(Buffer.from(selected.png_base64,'base64'),expected)
  } finally { await browser.close() }
})
