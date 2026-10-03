import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { randomUUID } from 'node:crypto';
import { Runtime, validate } from './runtime.mjs';

const base = () => ({ id: 1, session_id: randomUUID(), deadline: Date.now() + 60000, params: {} });
test('closed protocol rejects script tools, file navigation, expired scope and excessive actions', () => {
  assert.throws(() => validate({ ...base(), method: 'evaluate', params: { code: 'danger()' } }));
  assert.throws(() => validate({ ...base(), method: 'navigate', params: { url: 'file:///etc/passwd' } }));
  assert.throws(() => validate({ ...base(), method: 'observe', deadline: 0 }));
  assert.throws(() => validate({ ...base(), method: 'scroll', params: { direction: 'up', amount: 99, snapshot_id: 'bs1-a' } }));
});

test('real managed Chromium: literal fill/click, no replay, stale navigation, private input and preview budget', { timeout: 120000 }, async t => {
  let clicks = 0;
  const server = createServer((req, res) => {
    if (req.url === '/hit') { clicks++; res.end('ok'); return; }
    res.setHeader('content-type', 'text/html');
    res.end('<title>Browser boundary fixture</title><h1>Fixture</h1><input placeholder="Task"><input type="password" value="secret-marker"><button onclick="fetch(\'/hit\')">Create once</button><p>Safe text</p>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  const r = new Runtime({ headless: true });
  const session_id = randomUUID(); let id = 0;
  const run = (method, params = {}) => r.execute({ id: ++id, method, session_id, deadline: Date.now() + 60000, params });
  t.after(async () => { try { await run('detach'); } catch {} await r.client?.close(); });
  const opened = await run('open', { mode: 'managed' });
  assert.equal(opened.connected, true);
  let observed = await run('observe');
  const receipt = observed.snapshot_id;
  await run('navigate', { snapshot_id: receipt, url: `http://127.0.0.1:${server.address().port}/` });
  await assert.rejects(run('navigate', { snapshot_id: receipt, url: 'http://127.0.0.1/' }), error => error.beforeDispatch === true);
  observed = await run('observe');
  assert.ok(!observed.text.includes('secret-marker'));
  assert.match(observed.image_base64, /^iVBOR/);
  const field = observed.elements.find(e => e.name === 'Task');
  await run('type', { snapshot_id: observed.snapshot_id, element_id: field.element_id, text: '中文 & literal `$(x)`' });
  observed = await run('observe');
  const button = observed.elements.find(e => e.name === 'Create once');
  const action = { snapshot_id: observed.snapshot_id, element_id: button.element_id };
  await run('click', action);
  await assert.rejects(run('click', action), error => error.beforeDispatch === true);
  assert.equal(clicks, 1);
  observed = await run('observe');
  const secret = observed.elements.find(e => e.protected);
  await assert.rejects(run('type', { snapshot_id: observed.snapshot_id, element_id: secret.element_id, text: 'no' }), error => error.beforeDispatch === true);
  const frame = await run('preview');
  assert.ok(frame.data_url.startsWith('data:image/jpeg;base64,'));
  assert.ok(frame.data_url.length < 1400000);
  await assert.rejects(run('preview'));
  await assert.rejects(run('observe', { tab_id: 'foreign', navigation_generation: 0 }), error => error.beforeDispatch === true);
  const metadata = await run('metadata');
  assert.ok(metadata.navigation_generation > opened.navigation_generation);
});

test('manual same-tab navigation permits fresh observe but old mutation receipts remain rejected; DOM text excludes private editable descendants', { timeout: 120000 }, async t => {
  let clicks=0;
  const server=createServer((req,res)=>{if(req.url==='/hit'){clicks++;res.end('ok');return}res.setHeader('content-type','text/html');res.end('<div contenteditable>EDITABLE_PRIVATE</div><div contenteditable="plaintext-only">PLAIN_PRIVATE</div><div data-private>PRIVATE_SUBTREE</div><div role="button"><textarea>TEXTAREA_PRIVATE</textarea>Safe label</div><button onclick="fetch(\'/hit\')">Action</button>')});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));t.after(()=>server.close());
  const r=new Runtime({headless:true});const session_id=randomUUID();let id=0;
  const run=(method,params={})=>r.execute({id:++id,method,session_id,deadline:Date.now()+60000,params});
  t.after(async()=>{try{await run('detach')}catch{}await r.client?.close()});
  const opened=await run('open',{mode:'managed'});const old=await run('observe');
  // Public Page API simulates owner navigation, outside the model protocol.
  await r.boundPage.goto(`http://127.0.0.1:${server.address().port}/manual`);
  const fresh=await run('observe',{tab_id:opened.tab_id,navigation_generation:old.navigation_generation});
  assert.ok(fresh.navigation_generation>old.navigation_generation);
  assert.doesNotMatch(fresh.text,/PRIVATE/);assert.doesNotMatch(JSON.stringify(fresh.elements),/PRIVATE/);
  assert.ok(fresh.elements.some(element=>element.name==='Safe label'));
  const button=fresh.elements.find(element=>element.name==='Action');
  await assert.rejects(run('click',{tab_id:opened.tab_id,navigation_generation:old.navigation_generation,snapshot_id:old.snapshot_id,element_id:button.element_id}),error=>error.beforeDispatch===true);
  assert.equal(clicks,0);
  await run('click',{tab_id:fresh.tab_id,navigation_generation:fresh.navigation_generation,snapshot_id:fresh.snapshot_id,element_id:button.element_id});
  assert.equal(clicks,1);
});

test('closing the bound page disconnects before any subsequent MCP ensureTab call', { timeout: 120000 }, async t => {
  let disconnects=0;
  const r=new Runtime({headless:true,onDisconnected:()=>{disconnects++}});const session_id=randomUUID();let id=0;
  const run=(method,params={})=>r.execute({id:++id,method,session_id,deadline:Date.now()+60000,params});
  await run('open',{mode:'managed'});
  const browser=r.boundPage.context().browser();
  t.after(async()=>{await browser.close();await r.client?.close()});
  const context=r.boundPage.context();await r.boundPage.close();
  assert.equal(disconnects,1);assert.equal(r.closed,true);assert.equal(context.pages().length,0);
  let calls=0;const call=r.client.callTool.bind(r.client);r.client.callTool=(...args)=>{calls++;return call(...args)};
  for(const method of ['metadata','observe','preview','focus','selection.poll']) await assert.rejects(run(method),error=>error.beforeDispatch===true);
  assert.equal(calls,0);assert.equal(context.pages().length,0);
  assert.equal(Object.keys(Buffer).filter(key=>key.startsWith('hexagon-page-')).length,0);
});
