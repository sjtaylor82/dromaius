'use strict';

// Dromaius front end: renders the mirrored Android screen as plain HTML
// so screen readers can use browse mode, quick navigation and say all.
//
// The DOM is updated in place, keyed by Android node ids, so the screen
// reader's position survives the constant small updates Android sends.

const tauri = window.__TAURI__;
const $ = (id) => document.getElementById(id);

function invoke(cmd, args) {
  return tauri.core.invoke(cmd, args).catch((e) => announce(String(e)));
}

let mode = 'starting';
let lastScreen = null;
let apps = [];
let pendingPageFocus = null;
let pageFocusFallback = null;
/** Values we recently sent per edit field, to ignore Android echoing them back late. */
const sentValues = new Map();

// ------------------------------------------------------------------ helpers

let announceClear = null;

function announce(text) {
  const live = $('live');
  live.textContent = '';
  clearTimeout(announceClear);
  // A short delay makes repeated identical messages announce again.
  setTimeout(() => { live.textContent = text; }, 60);
  // Screen readers announce the change immediately; clear it soon after so
  // it doesn't linger at the end of the page, where browse mode reads it.
  announceClear = setTimeout(() => { live.textContent = ''; }, 1500);
}

function setAttr(el, name, value) {
  if (value === null || value === undefined || value === false) {
    if (el.hasAttribute(name)) el.removeAttribute(name);
  } else if (el.getAttribute(name) !== String(value)) {
    el.setAttribute(name, String(value));
  }
}

function setText(el, text) {
  if (el.textContent !== text) el.textContent = text;
}

function currentHeading() {
  return document.querySelector(`#${mode}-view h1`);
}

function updateTitle() {
  const part = mode === 'apps' ? 'Your apps' : mode === 'app' ? (lastScreen?.title || 'Android') : 'Starting';
  document.title = `${part} - Dromaius`;
}

function setMode(next) {
  if (next === mode) return;
  mode = next;
  $('starting-view').hidden = mode !== 'starting';
  $('apps-view').hidden = mode !== 'apps';
  $('app-view').hidden = mode !== 'app';
  if (mode === 'app' && lastScreen) renderScreen(lastScreen);
  updateTitle();
  currentHeading()?.focus();
}

function focusNode(id) {
  const el = document.querySelector(`#screen [data-id="${id}"]`);
  if (!el) return false;
  focusScreenElement(el);
  return true;
}

function focusScreenElement(el) {
  // Clickable headings contain their real button inside the heading element.
  const target = el.querySelector?.(':scope > button[data-id]') || el;
  const natural = ['BUTTON', 'INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
  if (!natural) target.tabIndex = -1;
  target.focus();
}

// ------------------------------------------------------------------ apps view

function renderApps(list) {
  apps = list;
  const ul = $('apps-list');
  $('apps-count').textContent = list.length === 1 ? '1 app' : `${list.length} apps`;
  const existing = new Map([...ul.children].map((li) => [li.dataset.key, li]));
  list.forEach((app, i) => {
    let li = existing.get(app.package);
    if (li) {
      existing.delete(app.package);
    } else {
      li = document.createElement('li');
      li.dataset.key = app.package;
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.package = app.package;
      li.append(button);
    }
    setText(li.firstChild, app.label);
    if (ul.children[i] !== li) ul.insertBefore(li, ul.children[i] || null);
  });
  existing.forEach((li) => li.remove());
}

function launchApp(pkg) {
  const label = apps.find((a) => a.package === pkg)?.label || pkg;
  lastScreen = { title: `Opening ${label}`, nodes: [] };
  invoke('launch', { package: pkg });
}

function showApps() {
  invoke('show_apps');
  setMode('apps');
}

// ------------------------------------------------------------------ screen view

function renderScreen(screen) {
  lastScreen = screen;
  if (mode !== 'app') return;
  setText($('screen-title'), screen.title);
  updateTitle();
  patch($('screen'), screen.nodes.map(nodeItem));
  if (screen.focus) {
    const focused = focusNode(screen.focus);
    if (focused && pendingPageFocus) finishPageFocus();
  }
  else if (screen.newScreen) $('screen-title').focus();
}

/**
 * Reconciles parent's children with `items` ({key, create, update}),
 * reusing elements with the same key so the screen reader keeps its place.
 */
function patch(parent, items) {
  const existing = new Map();
  for (const el of parent.children) existing.set(el.dataset.key, el);
  items.forEach((item, i) => {
    let el = existing.get(item.key);
    if (el) {
      existing.delete(item.key);
    } else {
      el = item.create();
      el.dataset.key = item.key;
    }
    item.update(el);
    const current = parent.children[i];
    if (current !== el) parent.insertBefore(el, current || null);
  });
  for (const el of existing.values()) el.remove();
}

function tagFor(d) {
  switch (d.kind) {
    case 'edit': return d.multiline ? 'textarea' : 'input';
    case 'slider': return 'input';
    case 'progress': return 'progress';
    case 'heading': return 'h2';
    case 'list': return 'ul';
    case 'group': return 'div';
    case 'grid': return 'div';
    case 'text': return d.clickable ? 'button' : 'p';
    case 'image': return d.clickable ? 'button' : 'div';
    case 'listitem': return d.clickable ? 'button' : 'div';
    // button, checkbox, switch, radio, tab, combobox: only a real button
    // when Android says it can be activated (or toggled).
    default: return d.clickable || d.checked !== undefined ? 'button' : 'p';
  }
}

/** Whether Android reports that tapping this element does something. */
function activatable(d) {
  return !!d.clickable || d.checked !== undefined;
}

function nodeItem(d) {
  const tag = tagFor(d);
  return {
    // Kind is part of the key: an element whose role changes is replaced,
    // not patched, so no attribute from its old role can linger.
    key: `${d.kind}:${tag}:${d.id}:${d.clickable ? 1 : 0}`,
    create() {
      const el = document.createElement(tag);
      el.dataset.id = d.id;
      if (tag === 'button') el.type = 'button';
      if (d.kind === 'slider') el.type = 'range';
      if (tag === 'input' || tag === 'textarea') {
        el.autocomplete = 'off';
        el.spellcheck = false;
      }
      return el;
    },
    update: (el) => updateNode(el, d),
  };
}

function scrollItem(container, forward) {
  return {
    key: `${forward ? 'more' : 'less'}:${container.id}`,
    create() {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'scroll';
      b.dataset.scroll = forward ? 'more' : 'less';
      b.dataset.container = container.id;
      b.textContent = forward
        ? 'Show more items (Alt+Page Down)'
        : 'Show earlier items (Alt+Page Up)';
      return b;
    },
    update() {},
  };
}

function liItem(item, pos) {
  return {
    key: `li:${item.key}`,
    create: () => document.createElement('li'),
    update(li) {
      patch(li, [item]);
      setAttr(li, 'aria-posinset', pos ? pos[0] : null);
      setAttr(li, 'aria-setsize', pos ? pos[1] : null);
    },
  };
}

/** Attributes whose meaning depends on the element's role. */
const ROLE_ATTRS = ['role', 'aria-checked', 'aria-selected', 'aria-haspopup', 'aria-rowcount', 'aria-colcount'];

/** Sets the role-specific attributes in `wanted` and removes all others. */
function setRoleAttrs(el, wanted) {
  for (const name of ROLE_ATTRS) setAttr(el, name, wanted[name] ?? null);
}

function updateNode(el, d) {
  el._desc = d;
  const hints = [];
  if (d.longClickable && !d.clickable) hints.push(`More actions with ${IS_MAC ? 'VoiceOver+Shift+M' : 'Shift+F10'}`);
  const description = [d.description, d.error && `Error: ${d.error}`, ...hints].filter(Boolean).join('. ');
  setAttr(el, 'aria-description', description || null);
  setAttr(el, 'aria-roledescription', d.roleDescription || null);
  setAttr(el, 'aria-disabled', d.disabled ? 'true' : null);
  setAttr(el, 'aria-expanded', d.expanded === undefined ? null : String(d.expanded));
  const clickable = el.tagName === 'BUTTON';
  if (clickable) el.dataset.act = 'click';
  // Long-press-only items aren't buttons, but must be focusable for Shift+F10.
  // Remove that again when they stop being long-pressable, so inert content
  // doesn't linger in the Tab order. (tabindex="-1", set when focus is moved
  // to plain content, is left alone.)
  if (!clickable && d.longClickable) {
    if (el.getAttribute('tabindex') !== '0') el.tabIndex = 0;
  } else if (el.getAttribute('tabindex') === '0') {
    el.removeAttribute('tabindex');
  }

  const role = {};
  switch (d.kind) {
    case 'checkbox':
    case 'switch':
    case 'radio':
      if (clickable) {
        role.role = d.kind;
        role['aria-checked'] = String(!!d.checked);
      }
      setText(el, d.label || d.kind);
      break;
    case 'tab':
      if (clickable) {
        role.role = 'tab';
        role['aria-selected'] = String(!!d.selected);
      }
      setText(el, d.label);
      break;
    case 'combobox':
      if (clickable) role['aria-haspopup'] = 'listbox';
      setText(el, d.label);
      break;
    case 'edit':
      updateEdit(el, d);
      break;
    case 'slider':
      el.min = d.range?.[0] ?? 0;
      el.max = d.range?.[1] ?? 100;
      if (document.activeElement !== el) el.value = d.range?.[2] ?? 0;
      setAttr(el, 'aria-label', d.label || 'Slider');
      break;
    case 'progress':
      if (d.range && d.range[1] > d.range[0]) {
        el.max = d.range[1] - d.range[0];
        el.value = d.range[2] - d.range[0];
      } else {
        el.removeAttribute('value');
      }
      setAttr(el, 'aria-label', d.label || 'Progress');
      break;
    case 'heading':
      if (d.clickable) {
        patch(el, [{
          key: 'heading-button',
          create() {
            const b = document.createElement('button');
            b.type = 'button';
            b.dataset.act = 'click';
            b.dataset.id = d.id;
            return b;
          },
          update: (b) => setText(b, d.label),
        }]);
      } else {
        setText(el, d.label);
      }
      break;
    case 'image':
      if (clickable) {
        setText(el, d.label || 'Image');
      } else {
        role.role = 'img';
        setAttr(el, 'aria-label', d.label || 'Image');
      }
      break;
    case 'list': {
      setAttr(el, 'aria-label', d.label || null);
      const items = (d.children || []).map((c) => liItem(nodeItem(c), c.pos));
      if (d.less) items.unshift(liItem(scrollItem(d, false)));
      if (d.more) items.push(liItem(scrollItem(d, true)));
      patch(el, items);
      break;
    }
    case 'grid':
      // A static table rather than ARIA grid: screen readers keep browse
      // mode in tables and offer table navigation (e.g. NVDA Ctrl+Alt+arrows).
      role.role = 'table';
      if (d.gridSize) {
        role['aria-rowcount'] = d.gridSize[0] > 0 ? d.gridSize[0] : null;
        role['aria-colcount'] = d.gridSize[1] > 0 ? d.gridSize[1] : null;
      }
      setAttr(el, 'aria-label', d.label || null);
      patch(el, gridRows(d));
      break;
    case 'group': {
      role.role = d.label ? 'group' : undefined;
      setAttr(el, 'aria-label', d.label || null);
      const items = (d.children || []).map(nodeItem);
      if (d.less) items.unshift(scrollItem(d, false));
      if (d.more) items.push(scrollItem(d, true));
      patch(el, items);
      break;
    }
    default:
      setText(el, d.label || (clickable ? 'Unlabelled button' : ''));
  }
  setRoleAttrs(el, role);
}

/** Grid children grouped into table rows by their row index. */
function gridRows(d) {
  const rows = new Map();
  let lastRow = 0;
  for (const c of d.children || []) {
    const row = c.cell ? c.cell[0] : lastRow;
    lastRow = row;
    if (!rows.has(row)) rows.set(row, []);
    rows.get(row).push(c);
  }
  const items = [...rows.entries()].sort((a, b) => a[0] - b[0]).map(([row, cells]) => ({
    key: `row:${d.id}:${row}`,
    create() {
      const r = document.createElement('div');
      r.setAttribute('role', 'row');
      return r;
    },
    update(r) {
      setAttr(r, 'aria-rowindex', d.gridSize && row >= 0 ? row + 1 : null);
      patch(r, cells.map((c) => cellItem(nodeItem(c), c.cell)));
    },
  }));
  for (const forward of [false, true]) {
    if (forward ? d.more : d.less) {
      const item = {
        key: `row:${forward ? 'more' : 'less'}:${d.id}`,
        create() {
          const r = document.createElement('div');
          r.setAttribute('role', 'row');
          return r;
        },
        update: (r) => patch(r, [cellItem(scrollItem(d, forward))]),
      };
      if (forward) items.push(item); else items.unshift(item);
    }
  }
  return items;
}

function cellItem(item, cell) {
  return {
    key: `cell:${item.key}`,
    create() {
      const c = document.createElement('div');
      c.setAttribute('role', 'cell');
      return c;
    },
    update(c) {
      setAttr(c, 'aria-colindex', cell && cell[1] >= 0 ? cell[1] + 1 : null);
      patch(c, [item]);
    },
  };
}

function updateEdit(el, d) {
  if (el.tagName === 'INPUT') {
    const type = d.password ? 'password' : 'text';
    if (el.type !== type) el.type = type;
  }
  setAttr(el, 'aria-label', d.label || d.placeholder || 'Edit');
  setAttr(el, 'placeholder', d.placeholder || null);
  setAttr(el, 'aria-invalid', d.error ? 'true' : null);
  el.readOnly = !!d.disabled;

  const focused = document.activeElement === el;
  const incoming = d.value ?? '';
  // Never overwrite what the user is typing with Android's masked password,
  // or with a late echo of text we sent ourselves.
  if (el.value === incoming || (focused && d.password)) return;
  if (focused && (sentValues.get(d.id) || []).includes(incoming)) return;
  el.value = incoming;
}

function rememberSent(id, value) {
  const list = sentValues.get(id) || [];
  list.push(value);
  if (list.length > 40) list.shift();
  sentValues.set(id, list);
}

// ------------------------------------------------------------------ actions menu

function openActions(el) {
  const d = el._desc;
  if (!d) return;
  const actions = [];
  if (activatable(d)) actions.push(['Activate', () => invoke('act', { id: d.id, action: 'click' })]);
  if (d.longClickable) actions.push(['Long press', () => invoke('act', { id: d.id, action: 'longClick' })]);
  for (const [actionId, label] of d.customActions || []) {
    actions.push([label, () => invoke('custom_action', { id: d.id, actionId })]);
  }
  if (d.expanded === false) actions.push(['Expand', () => invoke('act', { id: d.id, action: 'expand' })]);
  if (d.expanded === true) actions.push(['Collapse', () => invoke('act', { id: d.id, action: 'collapse' })]);

  if (actions.length === 0) {
    announce('No actions for this item');
    return;
  }
  const dialog = $('actions-dialog');
  setText($('actions-title'), `Actions for ${d.label || 'this item'}`);
  const ul = $('actions-list');
  ul.replaceChildren(...actions.map(([label, run]) => {
    const li = document.createElement('li');
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = label;
    b.addEventListener('click', () => {
      dialog.close();
      el.focus();
      run();
    });
    li.append(b);
    return li;
  }));
  dialog.showModal();
}

// ------------------------------------------------------------------ install dialog

function openInstall() {
  const dialog = $('install-dialog');
  if (dialog.open) return;
  $('install-link').value = '';
  dialog.returnValue = '';
  dialog.showModal();
  $('install-link').focus();
}

$('install-dialog').addEventListener('close', () => {
  const dialog = $('install-dialog');
  const link = $('install-link').value.trim();
  if (dialog.returnValue === 'open' && link) {
    lastScreen = { title: 'Opening Google Play', nodes: [] };
    invoke('install_link', { link });
  } else {
    currentHeading()?.focus();
  }
});

// ------------------------------------------------------------------ events

$('apps-list').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-package]');
  if (b) launchApp(b.dataset.package);
});

const screen = $('screen');

/** Page the Android collection without first finding its synthetic button. */
function scrollPage(forward) {
  const direction = forward ? 'more' : 'less';
  const control = screen.querySelector(`[data-scroll="${direction}"]`);
  if (!control) {
    announce(forward ? 'No more items' : 'No earlier items');
    return;
  }
  invoke('scroll', { id: control.dataset.container, forward });
  announce(forward ? 'Loading more items' : 'Loading earlier items');
}

/** Scroll one particular Android collection. */
function scrollCollection(container, forward, silent = false) {
  const direction = forward ? 'more' : 'less';
  const control = container?.querySelector(`[data-scroll="${direction}"]`);
  if (!control) return false;
  if (pendingPageFocus?.inFlight) return true;
  pendingPageFocus = {
    container: control.dataset.container,
    forward,
    inFlight: true,
  };
  const current = document.activeElement.closest?.('[data-id]');
  const anchor = $('scroll-focus-anchor');
  setText(anchor, current?._desc?.label || 'Android screen');
  anchor.hidden = false;
  anchor.focus();
  clearTimeout(pageFocusFallback);
  pageFocusFallback = setTimeout(() => {
    if (!pendingPageFocus) return;
    pendingPageFocus = null;
    anchor.hidden = true;
    $('screen-title').focus();
  }, 3000);
  invoke('scroll', { id: control.dataset.container, forward });
  if (!silent) announce(forward ? 'Loading more items' : 'Loading earlier items');
  return true;
}

function finishPageFocus() {
  clearTimeout(pageFocusFallback);
  pageFocusFallback = null;
  pendingPageFocus = null;
  $('scroll-focus-anchor').hidden = true;
}

/**
 * Move through the mirrored Android stops when browse mode is off. At either
 * end, perform the same Android paging action as Alt+Page Up/Down.
 */
function moveScreenFocus(forward, origin) {
  const containers = new Set(['list', 'grid', 'group']);
  let current = origin?.closest?.('[data-id]') || document.activeElement;
  while (current && !current._desc) current = current.parentElement;

  // Treat a scrollable collection as its own navigation sequence. Otherwise,
  // reaching its final item could move into unrelated controls elsewhere on
  // the Android screen instead of performing the collection's scroll action.
  let collection = current?.parentElement;
  while (collection && collection !== screen
      && !(collection._desc && containers.has(collection._desc.kind))) {
    collection = collection.parentElement;
  }
  const scope = collection && collection !== screen ? collection : screen;
  const stops = [...scope.querySelectorAll('[data-id]')]
    .filter((el) => el._desc && !containers.has(el._desc.kind));
  if (!stops.length) return;
  const index = stops.indexOf(current);
  const nextIndex = index < 0
    ? (forward ? 0 : stops.length - 1)
    : index + (forward ? 1 : -1);
  const next = stops[nextIndex];
  if (next) {
    focusScreenElement(next);
    return;
  }
  if (scope !== screen && scrollCollection(scope, forward, true)) return;

  // This collection has reached its real beginning/end. Continue into the
  // surrounding Android screen (for example, from EatClub's content into its
  // Home/Explore/Earn/Favourites/My Offers navigation bar) without requiring Tab.
  const allStops = [...screen.querySelectorAll('[data-id]')]
    .filter((el) => el._desc && !containers.has(el._desc.kind));
  const globalIndex = allStops.indexOf(current);
  const outside = allStops[globalIndex + (forward ? 1 : -1)];
  if (outside) focusScreenElement(outside);
}

screen.addEventListener('click', (e) => {
  const more = e.target.closest('[data-scroll]');
  if (more) {
    invoke('scroll', { id: more.dataset.container, forward: more.dataset.scroll === 'more' });
    announce('Loading');
    return;
  }
  const el = e.target.closest('[data-act="click"]');
  if (el && el.getAttribute('aria-disabled') !== 'true') {
    invoke('act', { id: el.dataset.id, action: 'click' });
  }
});

screen.addEventListener('input', (e) => {
  const el = e.target;
  if (!el.dataset.id) return;
  if (el.type === 'range') return;
  rememberSent(el.dataset.id, el.value);
  invoke('set_text', { id: el.dataset.id, text: el.value, start: el.selectionStart ?? el.value.length, end: el.selectionEnd ?? el.value.length });
});

screen.addEventListener('change', (e) => {
  const el = e.target;
  if (el.type === 'range' && el.dataset.id) invoke('set_progress', { id: el.dataset.id, value: Number(el.value) });
});

screen.addEventListener('keydown', (e) => {
  const el = e.target;
  const editing = el.matches?.('input, textarea, select, [contenteditable="true"]');
  if (!editing && !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey
      && (e.key === 'ArrowDown' || e.key === 'ArrowUp') && el.closest?.('[data-id]')) {
    e.preventDefault();
    moveScreenFocus(e.key === 'ArrowDown', el);
    return;
  }
  if (e.key === 'Enter' && el.tagName === 'INPUT' && el.type !== 'range' && el.dataset.id) {
    e.preventDefault();
    invoke('act', { id: el.dataset.id, action: 'imeEnter' });
  }
});

screen.addEventListener('focusin', (e) => {
  const el = e.target;
  if ((el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') && el.type !== 'range' && el.dataset.id) {
    invoke('act', { id: el.dataset.id, action: 'focus' });
  }
});

screen.addEventListener('contextmenu', (e) => {
  const el = e.target.closest('[data-id]');
  if (!el || el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') return;
  e.preventDefault();
  openActions(el);
});

// The web view's own context menu (Back, Refresh, Inspect…) is just noise here.
document.addEventListener('contextmenu', (e) => {
  if (!e.target.closest('input, textarea')) e.preventDefault();
});

// Keyboard shortcuts. macOS gets Mac conventions: Option+arrows move by word
// in text fields there, and many Mac keyboards have no F-keys or Home.
const IS_MAC = /Mac/.test(navigator.platform || navigator.userAgent);

const SHORTCUTS = [
  {
    id: 'apps', what: 'Show your apps', run: () => showApps(),
    // Same key on both: Alt+H (Option+H on a Mac). The physical key code is
    // used because Option changes the character typed on a Mac. Alt+Home
    // still works on Windows.
    win: ['Alt+H', (e) => (e.altKey && !e.shiftKey && e.code === 'KeyH') || (e.altKey && e.key === 'Home')],
    mac: ['Option+H', (e) => e.altKey && !e.shiftKey && e.code === 'KeyH'],
  },
  {
    id: 'back', what: 'Android Back', run: () => invoke('global', { action: 'back' }),
    win: ['Alt+Left', (e) => e.altKey && e.key === 'ArrowLeft'],
    mac: ['Cmd+[', (e) => e.metaKey && e.key === '['],
  },
  {
    id: 'notifications', what: 'Android notifications (new ones are also announced as they arrive)',
    run: () => invoke('global', { action: 'notifications' }),
    // Alt+N on both (Option+N on a Mac, matched by key code: Option changes
    // the character typed).
    win: ['Alt+N', (e) => e.altKey && !e.shiftKey && e.code === 'KeyN'],
    mac: ['Option+N', (e) => e.altKey && !e.shiftKey && e.code === 'KeyN'],
  },
  {
    id: 'next-items', what: 'Next screen of items in a long Android list', run: () => scrollPage(true),
    win: ['Alt+Page Down', (e) => e.altKey && e.key === 'PageDown'],
    mac: ['Option+Page Down', (e) => e.altKey && e.key === 'PageDown'],
  },
  {
    id: 'previous-items', what: 'Previous screen of items in a long Android list', run: () => scrollPage(false),
    win: ['Alt+Page Up', (e) => e.altKey && e.key === 'PageUp'],
    mac: ['Option+Page Up', (e) => e.altKey && e.key === 'PageUp'],
  },
  {
    id: 'install', what: 'Find an app on Google Play: type its name, or paste a link', run: () => openInstall(),
    win: ['Ctrl+L', (e) => e.ctrlKey && e.key.toLowerCase() === 'l'],
    mac: ['Cmd+L', (e) => e.metaKey && e.key.toLowerCase() === 'l'],
  },
  {
    id: 'refresh', what: 'Refresh the screen', run: () => { invoke('refresh'); announce('Refreshing'); },
    win: ['F5', (e) => e.key === 'F5' || (e.ctrlKey && !e.shiftKey && e.key.toLowerCase() === 'r')],
    mac: ['Cmd+R', (e) => e.metaKey && !e.shiftKey && e.key.toLowerCase() === 'r'],
  },
  {
    id: 'talk', what: "Push to talk, like a phone's PTT button: press once to start, again to stop. In apps such as Zello, assign it as the PTT button by pressing it when asked",
    run: () => toggleTalk(),
    // Function keys pass through screen readers' browse mode, unlike
    // letters and punctuation. (F7 is also Edge's Caret Browsing key; this
    // handler takes it first.)
    win: ['F7', (e) => plainKey(e, 'F7')],
    mac: ['F7 (Fn+F7 on most Mac keyboards)', (e) => plainKey(e, 'F7')],
  },
  {
    id: 'hold', what: 'Tap and hold (long press) the current item',
    run: () => tapAndHold(),
    win: ['F8', (e) => plainKey(e, 'F8')],
    mac: ['F8 (Fn+F8 on most Mac keyboards)', (e) => plainKey(e, 'F8')],
  },
  {
    id: 'help', what: 'Keyboard help', run: () => $('help-dialog').showModal(),
    win: ['F1', (e) => e.key === 'F1'],
    mac: ['Cmd+?', (e) => e.metaKey && (e.key === '?' || (e.shiftKey && e.key === '/'))],
  },
];

/** The key pressed on its own (no Ctrl, Alt, Cmd or Shift). */
function plainKey(e, key) {
  return e.key === key && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
}

/** The mirrored Android item the user is on, if any. */
function currentNode() {
  return document.activeElement?.closest?.('#screen [data-id]') || null;
}

// Push-to-talk: F7 works like a phone's physical PTT button. The first
// press holds Android's PTT key (F12) down, the next releases it. Apps such
// as Zello let you assign it as their PTT button.
let talking = false;

function toggleTalk() {
  talking = !talking;
  invoke('ptt_key', { down: talking });
  announce(talking ? 'Talking' : 'Stopped talking');
}

function tapAndHold() {
  const node = currentNode();
  if (!node) {
    announce('Move to an item first');
    return;
  }
  invoke('act', { id: node.dataset.id, action: 'longClick' });
}

/** Shortcuts handled by the screen reader or browser rather than by us. */
const OTHER_KEYS = IS_MAC
  ? [['VoiceOver+Shift+M', 'More actions for the current item, such as long press'],
    ['Return in an edit field', 'Submit, for example run a search']]
  : [['Shift+F10 or the Applications key', 'More actions for the current item, such as long press'],
    ['Enter on an edit field', 'Submit, for example run a search']];

function shortcutLabel(id) {
  const s = SHORTCUTS.find((x) => x.id === id);
  return s ? (IS_MAC ? s.mac : s.win)[0] : '';
}

document.addEventListener('keydown', (e) => {
  for (const s of SHORTCUTS) {
    if ((IS_MAC ? s.mac : s.win)[1](e)) {
      e.preventDefault();
      s.run();
      return;
    }
  }
}, true);

function renderShortcuts() {
  const rows = [...SHORTCUTS.map((s) => [shortcutLabel(s.id), s.what]), ...OTHER_KEYS];
  $('shortcut-list').replaceChildren(...rows.flatMap(([keys, what]) => {
    const dt = document.createElement('dt');
    dt.textContent = keys;
    const dd = document.createElement('dd');
    dd.textContent = what;
    return [dt, dd];
  }));
  for (const [button, id] of [['btn-apps', 'apps'], ['btn-back', 'back'], ['btn-notifications', 'notifications'],
    ['btn-install', 'install'], ['btn-help', 'help']]) {
    $(button).title = shortcutLabel(id);
  }
}
renderShortcuts();

$('btn-apps').addEventListener('click', showApps);
$('btn-back').addEventListener('click', () => invoke('global', { action: 'back' }));
$('btn-notifications').addEventListener('click', () => invoke('global', { action: 'notifications' }));
$('btn-install').addEventListener('click', openInstall);
$('btn-help').addEventListener('click', () => $('help-dialog').showModal());
for (const id of ['btn-apps', 'btn-notifications', 'btn-install', 'btn-help']) {
  $(id).addEventListener('click', () => { $('main-menu').open = false; });
}

let displayModeLoaded = false;

function showDisplayMode(mode) {
  if (mode !== 'phone' && mode !== 'tablet') return;
  displayModeLoaded = true;
  $('btn-phone-mode').disabled = mode === 'phone';
  $('btn-tablet-mode').disabled = mode === 'tablet';
  setAttr($('btn-phone-mode'), 'aria-pressed', String(mode === 'phone'));
  setAttr($('btn-tablet-mode'), 'aria-pressed', String(mode === 'tablet'));
}

async function readDisplayMode() {
  const current = await invoke('display_mode');
  showDisplayMode(current);
}

async function chooseDisplayMode(mode) {
  // Move focus before the chosen button becomes disabled.
  $('main-menu').open = false;
  $('main-menu').querySelector('summary').focus();
  $('btn-phone-mode').disabled = true;
  $('btn-tablet-mode').disabled = true;
  const applied = await invoke('set_display_mode', { mode });
  showDisplayMode(applied);
  if (!applied) readDisplayMode();
}

$('btn-phone-mode').addEventListener('click', () => chooseDisplayMode('phone'));
$('btn-tablet-mode').addEventListener('click', () => chooseDisplayMode('tablet'));

// ------------------------------------------------------------------ first-run setup

let lastDecile = -1;

function renderSetup(setup) {
  const license = $('setup-license');
  const progress = $('setup-progress');
  license.hidden = setup.stage !== 'license';
  progress.hidden = !['downloading', 'unpacking', 'checking'].includes(setup.stage);
  // Only show a bar while something is actually downloading.
  $('setup-bar').hidden = setup.stage !== 'downloading';
  const message = $('starting-message');

  switch (setup.stage) {
    case 'checking':
      setText(message, 'Checking what needs to be installed');
      setText($('setup-detail'), '');
      break;
    case 'license': {
      const gb = (setup.downloadMb / 1000).toFixed(1);
      setText(message, 'Android needs to be installed before first use.');
      setText($('license-intro'), `Dromaius will download ${setup.android} with Google Play from Google, ${gb} GB in total:`);
      $('license-items').replaceChildren(...setup.items.map(([label, mb]) => {
        const li = document.createElement('li');
        li.textContent = `${label}: ${mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`}`;
        return li;
      }));
      setText($('license-space'),
        `You need about ${Math.ceil(gb * 2.5 + 4)} GB of free disk space. Afterwards Android uses about ` +
        `${Math.ceil(gb * 1.5)} GB, plus about 4 GB for the snapshot that lets it start in seconds, plus the apps you install. ` +
        'Google requires you to accept its licence first. ' +
        'The licence text follows, then Accept and Decline buttons.');
      if (!$('license-text').textContent) {
        $('license-text').replaceChildren(...setup.text.split(/\n\s*\n/).map((para) => {
          const p = document.createElement('p');
          p.textContent = para.replace(/\s+/g, ' ').trim();
          return p;
        }));
      }
      license.querySelector('h2').tabIndex = -1;
      license.querySelector('h2').focus();
      break;
    }
    case 'downloading': {
      setText(message, 'Installing Android');
      $('setup-bar').value = setup.percent;
      setText($('setup-detail'), `Downloading ${setup.label}: ${setup.percent}% (${setup.doneMb} of ${setup.totalMb} MB)`);
      const decile = Math.floor(setup.percent / 10);
      if (decile !== lastDecile) {
        lastDecile = decile;
        announce(`Downloading Android, ${setup.percent} percent`);
      }
      break;
    }
    case 'unpacking':
      setText(message, 'Installing Android');
      setText($('setup-detail'), `Unpacking ${setup.label}`);
      announce(`Unpacking ${setup.label}`);
      break;
    case 'virtualization':
    case 'failed':
      setText(message, setup.message);
      announce(setup.message);
      currentHeading()?.focus();
      break;
    case 'done':
      setText($('setup-detail'), '');
      announce('Android is installed. Starting it for the first time, this takes about a minute.');
      break;
  }
}

$('license-accept').addEventListener('click', () => {
  invoke('answer_license', { accepted: true });
  $('setup-license').hidden = true;
  announce('Starting download');
});
$('license-decline').addEventListener('click', () => {
  invoke('answer_license', { accepted: false });
  $('setup-license').hidden = true;
});

// ------------------------------------------------------------------ versions

function renderAbout(about) {
  const rows = [
    ['Dromaius', about.dromaius],
    ['Android', about.android || 'Unknown'],
    ['Android emulator', about.emulator ? `Version ${about.emulator}` : 'Unknown'],
    ['Android files', about.sdkSize ? `${about.sdk} (${about.sdkSize})` : about.sdk],
    ['Your Android data', about.dataSize || 'Unknown'],
    ['Updates', about.updates.length ? about.updates.map((u) => u.text).join(' ') : 'Everything is up to date'],
  ];
  $('about-list').replaceChildren(...rows.flatMap(([term, value]) => {
    const dt = document.createElement('dt');
    dt.textContent = term;
    const dd = document.createElement('dd');
    dd.textContent = value;
    return [dt, dd];
  }));
  $('update-notice').hidden = about.updates.length === 0;
  $('update-list').replaceChildren(...about.updates.map((update) => {
    const li = document.createElement('li');
    const p = document.createElement('p');
    p.textContent = update.text;
    li.append(p);
    if (update.action) {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = update.action;
      b.addEventListener('click', () => offerUpdate(update));
      li.append(b);
    }
    return li;
  }));
}

let pendingUpdate = null;

function offerUpdate(update) {
  if (update.kind === 'emulator') {
    invoke('start_update', { kind: update.kind });
    return;
  }
  // Everything else restarts Android or deletes data: confirm first.
  pendingUpdate = update;
  setText($('update-title'), update.action);
  setText($('update-text'), update.text);
  const upgrade = update.kind === 'android';
  $('update-keep-row').hidden = !upgrade;
  $('update-keep').checked = true;
  if (upgrade && update.backupSize) {
    setText($('update-keep-label'), `Keep my current Android as a backup (about ${update.backupSize}), so I can switch back`);
  }
  setText($('update-note'), {
    android: 'Android restarts during the upgrade. The first start on the new version can take several minutes.',
    switchBackup: 'Android restarts, which can take a minute. Your current Android is kept as the backup.',
    deleteBackup: 'This permanently deletes the backup, including its apps, their data and sign-ins.',
  }[update.kind] || '');
  setText($('update-confirm'), { android: 'Upgrade', switchBackup: 'Switch', deleteBackup: 'Delete backup' }[update.kind] || 'OK');
  $('update-dialog').returnValue = '';
  $('update-dialog').showModal();
}

$('update-dialog').addEventListener('close', () => {
  if ($('update-dialog').returnValue === 'confirm' && pendingUpdate) {
    invoke('start_update', { kind: pendingUpdate.kind, keepBackup: $('update-keep').checked });
  } else {
    currentHeading()?.focus();
  }
  pendingUpdate = null;
});

function applyState(state) {
  setText($('status'), state.status);
  setText($('starting-message'), state.status);
  setMode(state.mode);
  if (state.connected && !displayModeLoaded) readDisplayMode();
}

async function start() {
  const { listen } = tauri.event;
  await listen('state', (e) => applyState(e.payload));
  await listen('announce', (e) => announce(e.payload));
  await listen('apps', (e) => renderApps(e.payload));
  await listen('screen', (e) => renderScreen(e.payload));
  await listen('setup', (e) => renderSetup(e.payload));
  await listen('about', (e) => renderAbout(e.payload));
  const init = await tauri.core.invoke('init');
  if (init.setup) renderSetup(init.setup);
  if (init.about) renderAbout(init.about);
  renderApps(init.apps);
  if (init.screen) lastScreen = init.screen;
  applyState(init.state);
  updateTitle();
  currentHeading()?.focus();
}

start();
