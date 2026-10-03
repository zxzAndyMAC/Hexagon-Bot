import test from 'node:test'
import assert from 'node:assert/strict'
import { chromium } from 'playwright'
import { createServer } from 'node:http'
import source from './react-source.generated.mjs'
import { installSelection, pollSelection } from './selection.mjs'

test('source-only React Grab core is inert, optional, and never reads selected HTML/form values', async () => {
  const server=createServer((_req,res)=>res.end('<button id="target">Visible</button><input value="PRIVATE">'))
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))
  const browser=await chromium.launch({headless:true})
  try {
    const page=await browser.newPage()
    await page.goto(`http://127.0.0.1:${server.address().port}`)
    await page.evaluate(() => {
      for(const key of ['innerHTML','outerHTML']) Object.defineProperty(Element.prototype,key,{get(){throw Error('HTML forbidden')}})
      Object.defineProperty(HTMLInputElement.prototype,'value',{get(){throw Error('value forbidden')}})
    })
    await page.evaluate(source)
    assert.equal(await page.evaluate(()=>typeof window.__hexagonReactSource.sourceForElement),'function')
    assert.equal(await page.evaluate(()=>Boolean(window.__REACT_GRAB__)),false)
    assert.equal(await page.evaluate(()=>window.__hexagonReactSource.sourceForElement(document.querySelector('#target'))),null)
    // React development's authoritative DOM fiber location remains useful when
    // optional getStack instrumentation/source maps are unavailable.
    await page.evaluate(()=>{document.querySelector('#target').__reactFiber$fixture={_debugSource:{fileName:'src/App.tsx',lineNumber:42}}})
    const scope={session_id:'s',tab_id:'t',navigation_generation:1,active:true}
    await installSelection(page,scope,{},source)
    await page.locator('#target').click()
    const refs=await pollSelection(page,scope)
    assert.deepEqual(refs.candidates[0].source_hint,{file:'src/App.tsx',line:42})
    assert.equal(refs.candidates[0].text,'Visible')
  } finally {await browser.close();await new Promise(resolve=>server.close(resolve))}
})

test('pinned source adapter rejects cross-origin, mutation, credential URLs and oversized responses', async () => {
  const server=createServer((_req,res)=>res.end('<div></div>'))
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))
  const browser=await chromium.launch({headless:true})
  try {
    const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}`)
    await page.evaluate(()=>{window.calls=[];window.fetch=async (url,options)=>{window.calls.push({url,options});return new Response('export default 1')}})
    // Expose the actual compiled lexical guard only inside this test sandbox.
    await page.evaluate(source.replace('globalThis.__hexagonReactSource =','globalThis.testSourceFetch = fetch; globalThis.__hexagonReactSource ='))
    const result=await page.evaluate(async()=>{
      const denied=[]
      for(const [url,options] of [['https://external.invalid/a.js',{}],['/api',{method:'POST'}],['/a.js',{method:'POST'}],[`http://user:pass@${location.host}/a.js`,{}],['/private.txt',{}]]) {
        try {await window.testSourceFetch(url,options);denied.push(false)} catch {denied.push(true)}
      }
      await window.testSourceFetch('/src/App.tsx')
      return {denied,calls:window.calls.map(({url,options})=>({url,method:options.method,credentials:options.credentials,redirect:options.redirect}))}
    })
    assert.deepEqual(result.denied,[true,true,true,true,true]);assert.equal(result.calls.length,1)
    await page.evaluate(()=>{window.fetch=async()=>new Response('large',{headers:{'content-length':'2097153'}})})
    await page.evaluate(source.replace('globalThis.__hexagonReactSource =','globalThis.testSourceFetch = fetch; globalThis.__hexagonReactSource ='))
    assert.equal(await page.evaluate(async()=>{try {await window.testSourceFetch('/large.js');return false}catch{return true}}),true)
    assert.deepEqual(result.calls[0],{url:`http://127.0.0.1:${server.address().port}/src/App.tsx`,method:'GET',credentials:'omit',redirect:'error'})
  } finally {await browser.close();await new Promise(resolve=>server.close(resolve))}
})
