// DOM tests for ui/app.js: the page is loaded in jsdom with a fake Tauri
// bridge, fed screens the way the backend sends them, and the resulting HTML
// is checked for what a screen reader would get.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { JSDOM } from 'jsdom';

const ui = path.resolve(import.meta.dirname, '../ui');

async function load({ mac = false } = {}) {
  const html = fs.readFileSync(path.join(ui, 'index.html'), 'utf8').replace(/<script[^>]*><\/script>/, '');
  const dom = new JSDOM(html, { runScripts: 'outside-only', pretendToBeVisual: true });
  const w = dom.window;
  const calls = [];
  const handlers = {};
  w.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        calls.push([cmd, args === undefined ? undefined : JSON.parse(JSON.stringify(args))]);
        if (cmd === 'init') {
          return { apps: [], screen: null, setup: null, about: null, state: { connected: true, status: 'Connected', mode: 'app' } };
        }
        if (cmd === 'display_mode') return 'phone';
        if (cmd === 'set_display_mode') return args.mode;
        return null;
      },
    },
    event: { listen: async (name, fn) => { handlers[name] = fn; } },
  };
  // jsdom doesn't implement modal dialogs.
  w.HTMLDialogElement.prototype.showModal = function showModal() { this.open = true; };
  w.HTMLDialogElement.prototype.close = function close(value) {
    this.open = false;
    if (value !== undefined) this.returnValue = value;
    this.dispatchEvent(new w.Event('close'));
  };
  if (mac) Object.defineProperty(w.navigator, 'platform', { value: 'MacIntel' });
  w.eval(fs.readFileSync(path.join(ui, 'app.js'), 'utf8'));
  await new Promise((r) => setTimeout(r, 20));
  const screen = (nodes, extra = {}) => handlers.screen({ payload: { title: 'Test', package: 'test', nodes, ...extra } });
  const el = (id) => w.document.querySelector(`#screen [data-id="${id}"]`);
  return { w, doc: w.document, calls, screen, el, handlers };
}

test('an element whose role changes is replaced, leaving no stale ARIA', async () => {
  const { screen, el } = await load();
  screen([{ id: 'a', kind: 'checkbox', label: 'Wi-Fi', clickable: true, checked: true }]);
  const first = el('a');
  assert.equal(first.getAttribute('role'), 'checkbox');
  assert.equal(first.getAttribute('aria-checked'), 'true');

  screen([{ id: 'a', kind: 'tab', label: 'Wi-Fi', clickable: true, selected: true }]);
  const second = el('a');
  assert.notEqual(second, first, 'a new element for the new role');
  assert.equal(second.getAttribute('role'), 'tab');
  assert.equal(second.getAttribute('aria-selected'), 'true');
  assert.equal(second.hasAttribute('aria-checked'), false);
});

test('updates to the same element keep it, so the screen reader keeps its place', async () => {
  const { screen, el } = await load();
  screen([{ id: 'a', kind: 'switch', label: 'Bluetooth', clickable: true, checked: false }]);
  const first = el('a');
  screen([{ id: 'a', kind: 'switch', label: 'Bluetooth', clickable: true, checked: true }]);
  assert.equal(el('a'), first);
  assert.equal(first.getAttribute('aria-checked'), 'true');
});

test('only elements Android reports as clickable become buttons', async () => {
  const { screen, el, calls } = await load();
  screen([
    { id: 'b', kind: 'button', label: 'OK', clickable: true },
    { id: 't', kind: 'text', label: 'Just text' },
    { id: 'n', kind: 'button', label: 'Not tappable' },
  ]);
  assert.equal(el('b').tagName, 'BUTTON');
  assert.equal(el('t').tagName, 'P');
  assert.equal(el('n').tagName, 'P', 'a button-like node without a click action is not offered as a button');

  el('b').click();
  el('t').click();
  assert.deepEqual(calls.filter(([c]) => c === 'act'), [['act', { id: 'b', action: 'click' }]]);
});

test('Activate is only offered for activatable elements', async () => {
  const { w, doc, screen, el } = await load();
  screen([
    { id: 't', kind: 'text', label: 'Plain', longClickable: true },
    { id: 'b', kind: 'button', label: 'Go', clickable: true },
  ]);
  el('t').dispatchEvent(new w.MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  let labels = [...doc.querySelectorAll('#actions-list button')].map((b) => b.textContent);
  assert.deepEqual(labels, ['Long press']);

  el('b').dispatchEvent(new w.MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  labels = [...doc.querySelectorAll('#actions-list button')].map((b) => b.textContent);
  assert.deepEqual(labels, ['Activate']);
});

test('long-press-only list items are focusable but not buttons', async () => {
  const { screen, el } = await load();
  screen([{ id: 'l', kind: 'list', label: '', children: [{ id: 'i', kind: 'listitem', label: 'Photo', longClickable: true, pos: [1, 3] }] }]);
  const item = el('i');
  assert.equal(item.tagName, 'DIV');
  assert.equal(item.tabIndex, 0);
  assert.match(item.getAttribute('aria-description'), /Shift\+F10/);
  assert.equal(item.parentElement.getAttribute('aria-posinset'), '1');
  assert.equal(item.parentElement.getAttribute('aria-setsize'), '3');
});

test('grids become tables with rows and cells', async () => {
  const { screen, el } = await load();
  screen([{
    id: 'g', kind: 'grid', label: 'Calendar', gridSize: [2, 2], more: true,
    children: [
      { id: 'c1', kind: 'button', label: '1', clickable: true, cell: [0, 0] },
      { id: 'c2', kind: 'button', label: '2', clickable: true, cell: [0, 1] },
      { id: 'c3', kind: 'button', label: '3', clickable: true, cell: [1, 0] },
    ],
  }]);
  const grid = el('g');
  assert.equal(grid.getAttribute('role'), 'table');
  assert.equal(grid.getAttribute('aria-colcount'), '2');
  const rows = [...grid.querySelectorAll('[role=row]')];
  assert.equal(rows.length, 3, 'two rows plus the "Show more items" row');
  assert.equal(rows[0].querySelectorAll('[role=cell]').length, 2);
  assert.equal(el('c2').parentElement.getAttribute('aria-colindex'), '2');
  assert.match(rows[2].textContent, /^Show more items/);
});

test('list paging shortcuts work without finding the scroll buttons', async () => {
  const { w, calls, screen } = await load();
  screen([{
    id: 'l', kind: 'list', label: 'Restaurants', more: true, less: true,
    children: [{ id: 'i', kind: 'listitem', label: 'Restaurant', clickable: true }],
  }]);

  w.document.dispatchEvent(new w.KeyboardEvent('keydown', {
    key: 'PageDown', altKey: true, bubbles: true,
  }));
  w.document.dispatchEvent(new w.KeyboardEvent('keydown', {
    key: 'PageUp', altKey: true, bubbles: true,
  }));

  assert.deepEqual(calls.filter(([cmd]) => cmd === 'scroll'), [
    ['scroll', { id: 'l', forward: true }],
    ['scroll', { id: 'l', forward: false }],
  ]);
});

test('focus-mode arrows work only when focus is inside the Android screen', async () => {
  const { w, doc, calls, screen, el } = await load();
  screen([
    {
      id: 'l', kind: 'list', label: 'Restaurants', more: true,
      children: [
        { id: 'a', kind: 'listitem', label: 'Alpha', clickable: true },
        { id: 'b', kind: 'listitem', label: 'Bravo', clickable: true },
      ],
    },
    { id: 'outside', kind: 'button', label: 'Outside the list', clickable: true },
  ]);

  el('a').focus();
  el('a').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  assert.equal(doc.activeElement, el('b'));

  el('b').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  el('b').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  assert.deepEqual(calls.filter(([cmd]) => cmd === 'scroll').at(-1),
    ['scroll', { id: 'l', forward: true }]);
  assert.equal(calls.filter(([cmd]) => cmd === 'scroll').length, 1,
    'key repeat cannot start a second scroll before focus arrives');
  assert.equal(doc.activeElement, doc.querySelector('#scroll-focus-anchor'),
    'parks focus on a stable anchor while Android changes the collection');
  assert.equal(doc.querySelector('#live').textContent, '', 'vertical navigation scrolls silently');

  // RecyclerViews can replace the focused node in a follow-up snapshot after
  // the backend's one-time focus hint. Focus must remain in the new page.
  screen([{
    id: 'l', kind: 'list', label: 'Restaurants', more: true, less: true,
    children: [
      { id: 'c', kind: 'listitem', label: 'Charlie', clickable: true },
      { id: 'd', kind: 'listitem', label: 'Delta', clickable: true },
    ],
  }]);
  assert.equal(doc.activeElement, doc.querySelector('#scroll-focus-anchor'),
    'does not focus an unconfirmed snapshot');

  screen([{
    id: 'l', kind: 'list', label: 'Restaurants', more: true, less: true,
    children: [
      { id: 'c', kind: 'listitem', label: 'Charlie', clickable: true },
      { id: 'd', kind: 'listitem', label: 'Delta', clickable: true },
    ],
  }], { focus: 'd' });
  assert.equal(doc.activeElement, el('d'), 'uses the backend target once it is stable');
  assert.equal(doc.querySelector('#scroll-focus-anchor').hidden, true);

  doc.querySelector('#screen-title').focus();
  doc.querySelector('#screen-title').dispatchEvent(
    new w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
  );
  assert.equal(doc.activeElement, doc.querySelector('#screen-title'));
});

test('secondary commands are grouped behind the menu and it closes after use', async () => {
  const { doc } = await load();
  const menu = doc.querySelector('#main-menu');
  assert.ok(menu);
  assert.equal(menu.querySelector('summary').textContent, 'Menu');
  assert.deepEqual([...menu.querySelectorAll('button')].map((button) => button.textContent), [
    'Apps', 'Notifications', 'Install from Google Play', 'Keyboard help',
    'Phone mode, about 6 inches', 'Tablet mode, about 11 inches',
  ]);
  assert.equal(menu.querySelector('.menu-items').lastElementChild.id, 'status');
  menu.open = true;
  doc.querySelector('#btn-help').click();
  assert.equal(menu.open, false);
});

test('phone and tablet choices show the current mode and apply the other one', async () => {
  const { doc, calls } = await load();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(doc.querySelector('#btn-phone-mode').disabled, true);
  assert.equal(doc.querySelector('#btn-phone-mode').getAttribute('aria-pressed'), 'true');
  assert.equal(doc.querySelector('#btn-tablet-mode').disabled, false);

  doc.querySelector('#main-menu').open = true;
  doc.querySelector('#btn-tablet-mode').click();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(calls.filter(([cmd]) => cmd === 'set_display_mode').at(-1),
    ['set_display_mode', { mode: 'tablet' }]);
  assert.equal(doc.querySelector('#btn-tablet-mode').disabled, true);
  assert.equal(doc.querySelector('#btn-tablet-mode').getAttribute('aria-pressed'), 'true');
  assert.equal(doc.querySelector('#main-menu').open, false);
  assert.equal(doc.activeElement, doc.querySelector('#main-menu summary'));
});

test('focus-mode arrows remain native inside Android edit fields', async () => {
  const { w, doc, calls, screen, el } = await load();
  screen([
    { id: 'e', kind: 'edit', label: 'Search', value: 'text' },
    { id: 'b', kind: 'button', label: 'Search', clickable: true },
  ]);
  el('e').focus();
  el('e').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  assert.equal(doc.activeElement, el('e'));
  assert.equal(calls.filter(([cmd]) => cmd === 'scroll').length, 0);
});

test('arrows leave a finished collection without requiring Tab', async () => {
  const { w, doc, calls, screen, el } = await load();
  screen([
    {
      id: 'l', kind: 'list', label: 'Offers',
      children: [
        { id: 'a', kind: 'listitem', label: 'First offer', clickable: true },
        { id: 'b', kind: 'listitem', label: 'Last offer', clickable: true },
      ],
    },
    { id: 'home', kind: 'button', label: 'Home', clickable: true },
    { id: 'offers', kind: 'button', label: 'My Offers', clickable: true },
  ]);

  el('b').focus();
  el('b').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  assert.equal(doc.activeElement, el('home'));
  assert.equal(calls.filter(([cmd]) => cmd === 'scroll').length, 0);

  el('home').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
  assert.equal(doc.activeElement, el('b'));
});

test('typing is not overwritten by late echoes, but app changes are shown', async () => {
  const { w, doc, screen, el, calls } = await load();
  screen([{ id: 'e', kind: 'edit', label: 'Search', value: '' }]);
  const input = el('e');
  input.focus();
  assert.equal(doc.activeElement, input);

  input.value = 'ab';
  input.dispatchEvent(new w.Event('input', { bubbles: true }));
  assert.deepEqual(calls.at(-1), ['set_text', { id: 'e', text: 'ab', start: 2, end: 2 }]);

  input.value = 'abc';
  input.dispatchEvent(new w.Event('input', { bubbles: true }));
  screen([{ id: 'e', kind: 'edit', label: 'Search', value: 'ab' }]); // Android catching up
  assert.equal(input.value, 'abc');

  screen([{ id: 'e', kind: 'edit', label: 'Search', value: 'abc@example.com' }]); // app autocompleted
  assert.equal(input.value, 'abc@example.com');
});

test('a new screen moves focus to its heading; a focus hint wins', async () => {
  const { doc, screen } = await load();
  screen([{ id: 'x', kind: 'button', label: 'One', clickable: true }], { newScreen: true });
  assert.equal(doc.activeElement.id, 'screen-title');
  screen([{ id: 'x', kind: 'button', label: 'One', clickable: true }], { focus: 'x' });
  assert.equal(doc.activeElement.dataset.id, 'x');
});

test('upgrade offers keep a backup by default, and the choice is passed on', async () => {
  const { doc, calls, handlers } = await load();
  // The About payload drives the Updates list on the Your apps page.
  const about = {
    dromaius: '0.1.0', android: 'Android 16 (API 36) with Google Play', emulator: '37.2.12', sdk: 'x',
    updates: [
      { kind: 'android', text: 'A newer Android is available.', action: 'Upgrade to Android 17 (API 37)', backupSize: '3.8 GB' },
      { kind: 'deleteBackup', text: 'Deleting frees space.', action: 'Delete the Android 15 backup' },
    ],
  };
  handlers.about({ payload: about });
  const buttons = [...doc.querySelectorAll('#update-list button')];
  assert.deepEqual(buttons.map((b) => b.textContent), ['Upgrade to Android 17 (API 37)', 'Delete the Android 15 backup']);

  buttons[0].click();
  assert.equal(doc.getElementById('update-keep-row').hidden, false);
  assert.equal(doc.getElementById('update-keep').checked, true);
  assert.match(doc.getElementById('update-keep-label').textContent, /3\.8 GB/);
  doc.getElementById('update-keep').checked = false;
  doc.getElementById('update-dialog').close('confirm');
  assert.deepEqual(calls.at(-1), ['start_update', { kind: 'android', keepBackup: false }]);

  buttons[1].click();
  assert.equal(doc.getElementById('update-keep-row').hidden, true, 'no backup choice when deleting');
  assert.equal(doc.getElementById('update-confirm').textContent, 'Delete backup');
  doc.getElementById('update-dialog').close('cancel');
  assert.equal(calls.filter(([c]) => c === 'start_update').length, 1, 'cancel does nothing');
});

test('an item that stops being long-pressable leaves the Tab order', async () => {
  const { screen, el } = await load();
  screen([{ id: 'p', kind: 'text', label: 'Photo', longClickable: true }]);
  const item = el('p');
  assert.equal(item.getAttribute('tabindex'), '0');

  screen([{ id: 'p', kind: 'text', label: 'Photo' }]);
  assert.equal(el('p'), item, 'same element, updated in place');
  assert.equal(item.hasAttribute('tabindex'), false);

  // Focus moved there by Dromaius (tabindex -1) is not undone by updates.
  screen([{ id: 'p', kind: 'text', label: 'Photo' }], { focus: 'p' });
  screen([{ id: 'p', kind: 'text', label: 'Photo' }]);
  assert.equal(item.getAttribute('tabindex'), '-1');
});

function press(w, init) {
  const e = new w.KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  w.document.body.dispatchEvent(e);
  return e.defaultPrevented;
}

test('Windows shortcuts', async () => {
  const { w, doc, calls } = await load();
  assert.equal(press(w, { key: 'ArrowLeft', altKey: true }), true);
  assert.deepEqual(calls.at(-1), ['global', { action: 'back' }]);
  assert.equal(doc.getElementById('btn-back').title, 'Alt+Left');
  assert.equal(press(w, { key: 'H', code: 'KeyH', altKey: true, shiftKey: true }), true);
  assert.equal(doc.getElementById('btn-apps').title, 'Alt+Shift+H');
  assert.match(doc.getElementById('shortcut-list').textContent, /Ctrl\+L/);
});

test('Mac shortcuts follow Mac conventions and leave Option+arrows alone', async () => {
  const { w, doc, calls } = await load({ mac: true });
  assert.equal(press(w, { key: '[', metaKey: true }), true);
  assert.deepEqual(calls.at(-1), ['global', { action: 'back' }]);
  const before = calls.length;
  assert.equal(press(w, { key: 'ArrowLeft', altKey: true }), false, 'Option+Left still moves by word');
  assert.equal(calls.length, before);
  assert.equal(doc.getElementById('btn-install').title, 'Cmd+L');
  assert.match(doc.getElementById('shortcut-list').textContent, /VoiceOver\+Shift\+M/);
  // Option+Shift+H types a symbol on a Mac, so the key code is what matters.
  assert.equal(press(w, { key: 'Ó', code: 'KeyH', altKey: true, shiftKey: true }), true);
  assert.equal(doc.getElementById('btn-apps').title, 'Option+Shift+H');
});
