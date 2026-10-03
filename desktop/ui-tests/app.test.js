// DOM tests for ui/app.js: the page is loaded in jsdom with a fake Tauri
// bridge, fed screens the way the backend sends them, and the resulting HTML
// is checked for what a screen reader would get.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { JSDOM } from 'jsdom';

const ui = path.resolve(import.meta.dirname, '../ui');

async function load() {
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
  assert.equal(rows[2].textContent, 'Show more items');
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
