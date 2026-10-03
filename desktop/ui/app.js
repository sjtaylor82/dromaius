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
  // Clear it afterwards so old messages don't linger at the end of the
  // page, where browse mode would read them as page content.
  announceClear = setTimeout(() => { live.textContent = ''; }, 6000);
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
  if (!el) return;
  const natural = ['BUTTON', 'INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName);
  if (!natural) el.tabIndex = -1;
  el.focus();
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
  if (screen.focus) focusNode(screen.focus);
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
    case 'text': return d.clickable ? 'button' : 'p';
    case 'image': return d.clickable ? 'button' : 'div';
    case 'listitem': return d.clickable || d.longClickable ? 'button' : 'div';
    default: return 'button'; // button, checkbox, switch, radio, tab, combobox
  }
}

function nodeItem(d) {
  const tag = tagFor(d);
  return {
    key: `${tag}:${d.id}:${d.clickable ? 1 : 0}`,
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
      b.textContent = forward ? 'Show more items' : 'Show earlier items';
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

function updateNode(el, d) {
  el._desc = d;
  const description = [d.description, d.error && `Error: ${d.error}`].filter(Boolean).join('. ');
  setAttr(el, 'aria-description', description || null);
  setAttr(el, 'aria-roledescription', d.roleDescription || null);
  setAttr(el, 'aria-disabled', d.disabled ? 'true' : null);
  setAttr(el, 'aria-expanded', d.expanded === undefined ? null : String(d.expanded));
  const clickable = el.tagName === 'BUTTON';
  if (clickable) el.dataset.act = 'click';

  switch (d.kind) {
    case 'checkbox':
    case 'switch':
    case 'radio':
      setAttr(el, 'role', d.kind);
      setAttr(el, 'aria-checked', String(!!d.checked));
      setText(el, d.label || d.kind);
      break;
    case 'tab':
      setAttr(el, 'role', 'tab');
      setAttr(el, 'aria-selected', String(!!d.selected));
      setText(el, d.label);
      break;
    case 'combobox':
      setAttr(el, 'aria-haspopup', 'listbox');
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
        setAttr(el, 'role', 'img');
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
    case 'group': {
      setAttr(el, 'role', d.label ? 'group' : null);
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
  const actions = [['Activate', () => invoke('act', { id: d.id, action: 'click' })]];
  if (d.longClickable) actions.push(['Long press', () => invoke('act', { id: d.id, action: 'longClick' })]);
  for (const [actionId, label] of d.customActions || []) {
    actions.push([label, () => invoke('custom_action', { id: d.id, actionId })]);
  }
  if (d.expanded === false) actions.push(['Expand', () => invoke('act', { id: d.id, action: 'expand' })]);
  if (d.expanded === true) actions.push(['Collapse', () => invoke('act', { id: d.id, action: 'collapse' })]);

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

document.addEventListener('keydown', (e) => {
  const key = e.key.toLowerCase();
  if (e.key === 'F5' || (e.ctrlKey && !e.shiftKey && key === 'r')) {
    e.preventDefault();
    invoke('refresh');
    announce('Refreshing');
  } else if (e.altKey && e.key === 'Home') {
    e.preventDefault();
    showApps();
  } else if (e.altKey && e.key === 'ArrowLeft') {
    e.preventDefault();
    invoke('global', { action: 'back' });
  } else if (e.ctrlKey && key === 'l') {
    e.preventDefault();
    openInstall();
  } else if (e.altKey && key === 'n') {
    e.preventDefault();
    invoke('global', { action: 'notifications' });
  } else if (e.key === 'F1') {
    e.preventDefault();
    $('help-dialog').showModal();
  }
}, true);

$('btn-apps').addEventListener('click', showApps);
$('btn-back').addEventListener('click', () => invoke('global', { action: 'back' }));
$('btn-install').addEventListener('click', openInstall);
$('btn-help').addEventListener('click', () => $('help-dialog').showModal());

function applyState(state) {
  setText($('status'), state.status);
  setText($('starting-message'), state.status);
  setMode(state.mode);
}

async function start() {
  const { listen } = tauri.event;
  await listen('state', (e) => applyState(e.payload));
  await listen('announce', (e) => announce(e.payload));
  await listen('apps', (e) => renderApps(e.payload));
  await listen('screen', (e) => renderScreen(e.payload));
  const init = await tauri.core.invoke('init');
  renderApps(init.apps);
  if (init.screen) lastScreen = init.screen;
  applyState(init.state);
  updateTitle();
  currentHeading()?.focus();
}

start();
