// CONCEPT: end-to-end click-through of web/oasis-v2.html in LIVE mode, driven over the Chrome DevTools protocol
//          (no npm packages; Node >= 22 has WebSocket). Checks: live mode hides sample-data controls, chat answers
//          through the pool, the model select switches the pool, a model that can't load restores the previous one,
//          the default model stays switchable, clear is disabled while streaming, `j` doesn't open the mock join.
// WHY: P4 07:51Z (live controls must act on the real pool) and Preston Sun 2am (switch models from the dashboard).
// DEPENDS ON: a scratch pool, never the live `pool` db: SpacetimeDB with crates/control as pool-smoke, two agents,
//             a coordinator whose coordinator.toml has model_id "qwen-0.5b" and
//             [models] "qwen-alt" = <same small GGUF>, "broken" = "/nonexistent/broken.gguf";
//             the repo served on :8001; headless Chrome with --remote-debugging-port=9333. Then:
//               U='http://127.0.0.1:8001/web/oasis-v2.html?stdb=http://127.0.0.1:3000&db=pool-smoke&llama=http://127.0.0.1:8089'
//               WS=$(curl -s -X PUT "http://127.0.0.1:9333/json/new?$(python3 -c 'import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1],safe=""))' "$U")" | python3 -c 'import sys,json;print(json.load(sys.stdin)["webSocketDebuggerUrl"])')
//               node web/e2e-oasis-v2.mjs "$WS"
//             The pool must be serving qwen-0.5b when it starts.
// AFFECTS: the scratch pool's model (it ends on qwen-0.5b). Exit code 1 if any check fails.
const ws = new WebSocket(process.argv[2]);
let id = 0; const pending = new Map();
ws.onmessage = e => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
const send = (method, params = {}) => new Promise(r => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const js = async expr => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); return r.result.result.value; };
const sleep = ms => new Promise(r => setTimeout(r, ms));
async function until(what, expr, secs = 120) {
  const t = Date.now();
  while (Date.now() - t < secs * 1000) { if (await js(expr)) return true; await sleep(500); }
  throw new Error(`timed out: ${what}`);
}
const results = [];
let lastSwitch = 0;                        // the control module allows one model switch per 30 s
async function pick(id) {
  const wait = lastSwitch + 31000 - Date.now();
  if (wait > 0) await sleep(wait);
  await js(`{ const s = document.getElementById('model-select'); s.value = '${id}'; s.dispatchEvent(new Event('change')); } true`);
  lastSwitch = Date.now();
}
const check = (name, ok, detail = '') => { results.push(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ' :: ' + detail : ''}`); };
await new Promise(r => ws.onopen = r);
await send('Runtime.enable');
await until('live poll', `document.body.classList.contains('live') && document.getElementById('model-select').options.length >= 3`, 30);

// 1. live mode is honest
check('footer says LIVE POOL', await js(`document.querySelector('footer').textContent.includes('LIVE POOL')`));
check('join preview hidden', await js(`document.getElementById('join-open').offsetParent === null && document.getElementById('inspect-join').offsetParent === null`));
check('capacity estimate hidden', await js(`document.getElementById('dock-fit').offsetParent === null && document.querySelector('.dock-budget').offsetParent === null`));
check('model list from coordinator', true, await js(`[...document.getElementById('model-select').options].map(o => o.value).join(',')`));
check('select shows live model', await js(`document.getElementById('model-select').value === 'qwen-0.5b'`));

// 2. chat answers through the pool
await js(`document.getElementById('chat-input').value = 'Say hi in three words.'; document.getElementById('chat-form').requestSubmit(); true`);
await until('chat answer', `!!document.querySelector('.chat-bot .chat-meta')`, 60);
check('chat answered', true, await js(`document.querySelector('.chat-bot p').textContent + ' | ' + document.querySelector('.chat-bot .chat-meta').textContent`));

// 3. switch model from the select
await pick('qwen-alt');
await until('switch starts', `document.getElementById('model-select').disabled`, 10);
check('dock shows switching', true, await js(`document.getElementById('dock-model').textContent`));
await until('switch settles', `!document.getElementById('model-select').disabled && document.getElementById('dock-model').textContent === 'qwen-alt'`, 180);
check('now serving qwen-alt', await js(`document.getElementById('model-select').value === 'qwen-alt'`), await js(`document.getElementById('event-log').textContent.slice(-160)`));
await js(`document.getElementById('chat-input').value = 'Name one color.'; document.getElementById('chat-form').requestSubmit(); true`);
await until('chat answer 2', `document.querySelectorAll('.chat-bot .chat-meta').length === 2`, 60);
check('chat after switch', true, await js(`[...document.querySelectorAll('.chat-bot p')].at(-1).textContent`));

// 3b. a second switch right away hits the cooldown: refused with the reducer's message, select stays usable
await js(`{ const s = document.getElementById('model-select'); s.value = 'broken'; s.dispatchEvent(new Event('change')); } true`);
await sleep(1500);
check('too-fast switch refused with cooldown message', await js(`document.getElementById('command-response').textContent.includes('one model switch per') && !document.getElementById('model-select').disabled`), await js(`document.getElementById('command-response').textContent`));

// 4. a model that can't load: the old one comes back, dock says so
await pick('broken');
await until('broken switch starts', `document.getElementById('model-select').disabled`, 10);
await until('broken switch settles', `!document.getElementById('model-select').disabled`, 180);
check('restored qwen-alt after failed load', await js(`document.getElementById('dock-model').textContent === 'qwen-alt' && document.getElementById('model-select').value === 'qwen-alt'`), await js(`document.getElementById('event-log').textContent.slice(-200)`));

// 4b. back to the default model from the select (it must still be listed)
check('default still listed', await js(`[...document.getElementById('model-select').options].some(o => o.value === 'qwen-0.5b')`));
await pick('qwen-0.5b');
await until('back-switch starts', `document.getElementById('model-select').disabled`, 10);
await until('back-switch settles', `!document.getElementById('model-select').disabled && document.getElementById('dock-model').textContent === 'qwen-0.5b'`, 180);
check('serving qwen-0.5b again', true);

// 5. clear is disabled while streaming; j does not open the mock join
await js(`document.getElementById('chat-input').value = 'Count from one to twenty.'; document.getElementById('chat-form').requestSubmit(); true`);
await sleep(150);
check('clear disabled while streaming', await js(`document.getElementById('chat-clear').disabled`));
await until('chat answer 3', `document.querySelectorAll('.chat-bot .chat-meta').length === 3`, 60);
await js(`document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', bubbles: true })); true`);
check('j does not open mock join', await js(`!document.getElementById('join-dialog').open`), await js(`document.getElementById('command-response').textContent`));

console.log(results.join('\n'));
ws.close();
process.exit(results.some(r => r.startsWith('FAIL')) ? 1 : 0);
