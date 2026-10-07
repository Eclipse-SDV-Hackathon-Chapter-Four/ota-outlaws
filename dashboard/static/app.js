/*
 * Copyright (c) 2026 Contributors to the Eclipse Foundation
 *
 * See the NOTICE file(s) distributed with this work for additional
 * information regarding copyright ownership.
 *
 * This program and the accompanying materials are made available under the
 * terms of the Eclipse Public License 2.0 which is available at
 * https://www.eclipse.org/legal/epl-2.0
 *
 * SPDX-License-Identifier: EPL-2.0
 */

/* AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5) */

// Dashboard front end: plain JavaScript, no build step. Polls the API every
// two seconds and renders the active tab.

'use strict';

const POLL_MS = 2000;
const SEVERITY_RANK = { Fatal: 5, Error: 4, Warn: 3, Info: 2, Debug: 1, Trace: 0, Unknown: -1 };

const state = {
  tab: location.hash.slice(1) || 'overview',
  overview: null,
  connected: false,
  detail: null,
  detailFetched: 0,
  logKind: 'input',
  logPaused: false,
  logFilter: '',
  logKey: '',
  sovd: null,
  sovdError: null,
  sovdFilter: 'all',
  expanded: new Set(),
  faultDetails: {},
  campaigns: [],
  campaignId: null,
  followLive: true,
  campaign: null,
  campaignError: null,
  collapsed: new Set(),
  scenarioList: null,
  pickAll: true,
  picked: new Set(),
  rebuild: false,
  runnerKey: '',
  busy: false,
  signals: new Map(),
  signalLoads: new Map(),
  hover: null,
};

// --- helpers -----------------------------------------------------------------

const $ = (id) => document.getElementById(id);

function esc(value) {
  return String(value ?? '').replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}

async function api(path, options = {}) {
  const response = await fetch(path, options);
  let body = null;
  try { body = await response.json(); } catch (_) { /* empty body */ }
  if (!response.ok) {
    throw new Error((body && body.error) || `${response.status} ${response.statusText}`);
  }
  return body;
}

// Changes need the X-Dashboard header (see api.rs).
function change(path, method = 'POST', body = undefined) {
  const headers = { 'X-Dashboard': '1' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  return api(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
}

function toast(text, bad = false) {
  const el = document.createElement('div');
  el.className = 'toast' + (bad ? ' bad' : '');
  el.textContent = text;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), bad ? 9000 : 5000);
}

function mib(bytes) {
  if (bytes == null) return '—';
  if (bytes >= 1024 ** 3) return (bytes / 1024 ** 3).toFixed(2) + ' GiB';
  return (bytes / 1024 ** 2).toFixed(1) + ' MiB';
}

function clock(ms) {
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour12: false }) + '.' + String(d.getMilliseconds()).padStart(3, '0');
}

function seconds(ms) {
  return ms == null ? '—' : (ms / 1000).toFixed(2) + ' s';
}

function stateBadge(s) {
  const cls = { running: 'ok', paused: 'warn', restarting: 'warn', exited: 'bad', dead: 'bad' }[s] || 'idle';
  return `<span class="badge ${cls}">${esc(s)}</span>`;
}

function components() {
  return (state.overview && state.overview.snapshot.components) || [];
}

function component(service) {
  return components().find((c) => c.service === service);
}

// --- tabs --------------------------------------------------------------------

function setTab(tab) {
  if (state.tab === tab) return;
  state.tab = tab;
  location.hash = tab;
  state.detail = null;
  state.detailFetched = 0;
  state.logKey = '';
  renderTabs();
  renderView();
  tick();
}

function renderTabs() {
  const tabs = [['overview', 'Overview', null]];
  for (const c of components()) tabs.push(['c:' + c.service, c.title, c.state]);
  const live = state.overview && state.overview.campaign.running;
  tabs.push(['campaign', 'Campaign', live ? 'live' : 'idle']);
  $('tabs').innerHTML = tabs.map(([id, title, s]) =>
    `<button class="${id === state.tab ? 'active' : ''}" data-tab="${esc(id)}">` +
    (s ? `<span class="dot ${esc(s)}"></span>` : '') + esc(title) + '</button>').join('');
}

$('tabs').addEventListener('click', (e) => {
  const button = e.target.closest('button[data-tab]');
  if (button) setTab(button.dataset.tab);
});

function renderView() {
  if (state.tab === 'overview') renderOverviewSkeleton();
  else if (state.tab === 'campaign') renderCampaignSkeleton();
  else if (state.tab.startsWith('c:')) renderComponentSkeleton(state.tab.slice(2));
  else { state.tab = 'overview'; renderOverviewSkeleton(); }
}

// --- polling -----------------------------------------------------------------

let ticking = false;

async function tick() {
  if (ticking) return;
  ticking = true;
  try {
    await refreshOverview();
    if (state.tab === 'overview') fillOverview();
    else if (state.tab === 'campaign') await refreshCampaign();
    else if (state.tab.startsWith('c:')) await refreshComponent(state.tab.slice(2));
  } finally {
    ticking = false;
  }
}

async function refreshOverview() {
  try {
    state.overview = await api('/api/overview');
    state.connected = true;
    // The tabs show each component's state; redraw them when one changes.
    const signature = components().map((c) => c.service + ':' + c.state).join()
      + (state.overview.campaign.running ? '|live' : '');
    if (signature !== state.tabSignature) {
      state.tabSignature = signature;
      renderTabs();
    }
  } catch (error) {
    state.connected = false;
  }
  const snap = state.overview && state.overview.snapshot;
  $('conn-dot').className = 'dot ' + (state.connected ? (snap && snap.error ? 'paused' : 'running') : 'exited');
  $('conn-text').textContent = !state.connected ? 'dashboard API unreachable'
    : snap && snap.error ? 'Docker: ' + snap.error
      : 'updated ' + new Date().toLocaleTimeString([], { hour12: false });
  if (snap) {
    const running = snap.components.filter((c) => c.state === 'running').length;
    $('subtitle').textContent = `Compose project ${snap.project} · ${running} of ${snap.components.length} components running`;
  }
}

// --- overview ----------------------------------------------------------------

function renderOverviewSkeleton() {
  $('view').innerHTML = `
    <div class="panel">
      <div class="row spread">
        <h2>Components</h2>
        <div class="row">
          <span class="muted small" id="ov-summary"></span>
          <button class="btn" id="ov-start-all">Start all</button>
          <button class="btn danger" id="ov-stop-all">Stop all</button>
        </div>
      </div>
      <table>
        <thead><tr><th>Component</th><th>Run state</th><th>Memory</th><th>Settings</th><th></th></tr></thead>
        <tbody id="ov-body"><tr><td colspan="5" class="muted">loading…</td></tr></tbody>
      </table>
      <div class="note">Click a row for its input and output logs and all settings. Memory is the
        container's memory without the page cache, as <span class="mono">docker stats</span> shows it.</div>
    </div>`;
  $('ov-start-all').onclick = () => allAction('start');
  $('ov-stop-all').onclick = () => {
    if (confirm('Stop every component of the stack?')) allAction('stop');
  };
  $('ov-body').addEventListener('click', onOverviewClick);
  fillOverview();
}

function settingsSummary(s) {
  if (!s) return '<span class="muted">—</span>';
  const parts = [`<div class="mono small">${esc(s.image)}</div>`];
  if (s.ports && s.ports.length) parts.push(`<div class="small">ports ${esc(s.ports.join(', '))}</div>`);
  parts.push(`<div class="small muted">restart: ${esc(s.restart_policy || 'no')} · ${s.env ? s.env.length : 0} env vars` +
    (s.ipc_mode && String(s.ipc_mode).startsWith('container:') ? ' · shares DFM IPC' : '') + '</div>');
  return parts.join('');
}

function memoryCell(c) {
  if (!c.memory) return '<span class="muted">—</span>';
  const pct = c.memory.limit_bytes ? Math.min(100, 100 * c.memory.usage_bytes / c.memory.limit_bytes) : 0;
  return `${mib(c.memory.usage_bytes)}<div class="membar"><div style="width:${Math.max(pct, 1).toFixed(1)}%"></div></div>` +
    `<div class="small muted">of ${mib(c.memory.limit_bytes)}</div>`;
}

function powerButtons(c) {
  if (c.state === 'missing') return '<span class="small muted">not created</span>';
  const on = c.state === 'running' || c.state === 'restarting' || c.state === 'paused';
  return `<div class="row">
    <button class="btn small ${on ? 'danger' : 'primary'}" data-act="${on ? 'stop' : 'start'}" data-svc="${esc(c.service)}">${on ? 'Stop' : 'Start'}</button>
    <button class="btn small" data-act="restart" data-svc="${esc(c.service)}" ${on ? '' : 'disabled'}>Restart</button>
  </div>`;
}

function fillOverview() {
  const body = $('ov-body');
  if (!body || !state.overview) return;
  const rows = components().map((c) => `
    <tr class="clickable" data-tab="c:${esc(c.service)}">
      <td><div class="row"><span class="dot ${esc(c.state)}"></span><strong>${esc(c.title)}</strong></div>
        <div class="small muted">${esc(c.role)}</div>
        <div class="small mono muted">${esc(c.container || c.service)}</div></td>
      <td>${stateBadge(c.state)}${c.health ? ` <span class="badge ${c.health === 'healthy' ? 'ok' : 'warn'}">${esc(c.health)}</span>` : ''}
        <div class="small muted">${esc(c.status)}</div>
        ${c.restart_count ? `<div class="small muted">${c.restart_count} restarts</div>` : ''}</td>
      <td>${memoryCell(c)}</td>
      <td>${settingsSummary(c.settings)}</td>
      <td>${powerButtons(c)}</td>
    </tr>`);
  const camp = state.overview.campaign;
  const live = camp.running;
  rows.push(`
    <tr class="clickable" data-tab="campaign">
      <td><div class="row"><span class="dot ${live ? 'live' : 'idle'}"></span><strong>Campaign Tool</strong></div>
        <div class="small muted">Runs fault campaigns through the real chain and judges them</div>
        <div class="small mono muted">${camp.runner && camp.runner.exists ? 'container campaign-runner' : 'started here, or on the host: cargo run -p campaign'}</div></td>
      <td>${live
        ? `<span class="badge info">campaign running</span><div class="small">${esc(live.id)}</div>
           <div class="small muted">${live.scenario ? 'scenario ' + esc(live.scenario) + ' · ' : ''}${live.done} of ${live.total} done</div>`
        : '<span class="badge idle">idle</span><div class="small muted">no campaign running</div>'}</td>
      <td><span class="muted small">—</span></td>
      <td><div class="small">evidence: <span class="mono">${esc(camp.runs_dir)}</span></div>
        ${camp.projects.length ? `<div class="small muted">projects: ${esc(camp.projects.join(', '))}</div>` : ''}</td>
      <td>${camp.runner && camp.runner.running
        ? '<button class="btn small danger" data-campaign="stop">Stop</button>'
        : `<button class="btn small primary" data-campaign="open" ${camp.runner && camp.runner.available ? '' : `disabled title="${esc((camp.runner && camp.runner.unavailable) || '')}"`}>Start…</button>`}</td>
    </tr>`);
  body.innerHTML = rows.join('');
  const running = components().filter((c) => c.state === 'running').length;
  $('ov-summary').textContent = `${running} / ${components().length} running`;
}

function onOverviewClick(e) {
  const campaignButton = e.target.closest('button[data-campaign]');
  if (campaignButton) {
    e.stopPropagation();
    if (campaignButton.dataset.campaign === 'stop') stopCampaign();
    else setTab('campaign');
    return;
  }
  const button = e.target.closest('button[data-act]');
  if (button) {
    e.stopPropagation();
    componentAction(button.dataset.svc, button.dataset.act);
    return;
  }
  const row = e.target.closest('tr[data-tab]');
  if (row) setTab(row.dataset.tab);
}

async function componentAction(service, action) {
  if (state.busy) return;
  if (action === 'stop' && service === 'opensovd-dfm' &&
      !confirm('Stopping DFM also cuts off the services that share its IPC namespace (Guardian, watchdog, gateway). Continue?')) return;
  state.busy = true;
  document.body.style.cursor = 'progress';
  try {
    const result = await change(`/api/components/${encodeURIComponent(service)}/${action}`);
    toast(result.done.join('\n'));
  } catch (error) {
    toast(`${action} ${service} failed: ${error.message}`, true);
  } finally {
    state.busy = false;
    document.body.style.cursor = '';
    tick();
  }
}

async function allAction(action) {
  if (state.busy) return;
  state.busy = true;
  document.body.style.cursor = 'progress';
  toast(`${action} all components …`);
  try {
    const result = await change(`/api/all/${action}`);
    const failed = result.done.filter((line) => !line.endsWith(': ok'));
    toast(result.done.join('\n'), failed.length > 0);
  } catch (error) {
    toast(`${action} all failed: ${error.message}`, true);
  } finally {
    state.busy = false;
    document.body.style.cursor = '';
    tick();
  }
}

// --- component tab ---------------------------------------------------------------

function isSovd(service) {
  return service.startsWith('opensovd');
}

function renderComponentSkeleton(service) {
  $('view').innerHTML = `
    <div class="panel" id="c-head"></div>
    ${isSovd(service) ? dtcSkeleton() : ''}
    <div class="panel">
      <div class="row spread">
        <div class="subtabs" id="log-tabs">
          <button data-kind="input">Input log</button>
          <button data-kind="output">Output log</button>
          <button data-kind="container">Container log</button>
        </div>
        <div class="row">
          <input class="filter" id="log-filter" placeholder="filter lines" value="${esc(state.logFilter)}">
          <label class="small"><input type="checkbox" id="log-pause" ${state.logPaused ? 'checked' : ''}> pause</label>
        </div>
      </div>
      <ul class="sources" id="log-sources"></ul>
      <div class="log" id="log-box"><div class="empty">loading…</div></div>
    </div>
    <div class="panel"><h2>Settings</h2><div id="c-settings" class="muted">loading…</div></div>`;
  $('log-tabs').onclick = (e) => {
    const b = e.target.closest('button[data-kind]');
    if (!b) return;
    state.logKind = b.dataset.kind;
    state.logKey = '';
    markLogTab();
    refreshLogs(service, true);
  };
  $('log-filter').oninput = (e) => { state.logFilter = e.target.value; state.logKey = ''; refreshLogs(service, true); };
  $('log-pause').onchange = (e) => { state.logPaused = e.target.checked; };
  $('c-head').addEventListener('click', (e) => {
    const b = e.target.closest('button[data-act]');
    if (b) componentAction(b.dataset.svc, b.dataset.act);
  });
  markLogTab();
  if (isSovd(service)) wireDtc();
}

function markLogTab() {
  for (const b of document.querySelectorAll('#log-tabs button')) {
    b.classList.toggle('active', b.dataset.kind === state.logKind);
  }
}

async function refreshComponent(service) {
  const c = component(service);
  fillComponentHead(service, c);
  const work = [refreshLogs(service, false)];
  if (isSovd(service)) work.push(refreshSovd());
  if (!state.detail || Date.now() - state.detailFetched > 6000) work.push(refreshDetail(service));
  await Promise.all(work);
}

function fillComponentHead(service, c) {
  const head = $('c-head');
  if (!head) return;
  if (!c) {
    head.innerHTML = `<h2>${esc(service)}</h2><div class="muted">not part of the project</div>`;
    return;
  }
  head.innerHTML = `
    <div class="row spread">
      <div>
        <div class="row"><span class="dot ${esc(c.state)}"></span><h2 style="margin:0">${esc(c.title)}</h2>${stateBadge(c.state)}
          ${c.health ? `<span class="badge ${c.health === 'healthy' ? 'ok' : 'warn'}">${esc(c.health)}</span>` : ''}</div>
        <div class="muted">${esc(c.role)}</div>
      </div>
      ${powerButtons(c)}
    </div>
    <div class="tiles" style="margin:14px 0 0">
      <div class="tile"><div class="l">Status</div><div>${esc(c.status)}</div></div>
      <div class="tile"><div class="l">Memory</div><div class="n" style="font-size:18px">${c.memory ? mib(c.memory.usage_bytes) : '—'}</div></div>
      <div class="tile"><div class="l">Started</div><div>${c.started_at && !c.started_at.startsWith('0001') ? esc(new Date(c.started_at).toLocaleString()) : '—'}</div></div>
      <div class="tile"><div class="l">Restarts</div><div class="n" style="font-size:18px">${c.restart_count}</div></div>
      <div class="tile"><div class="l">Container</div><div class="mono small">${esc(c.container || '—')}</div></div>
    </div>`;
}

async function refreshDetail(service) {
  try {
    state.detail = await api(`/api/components/${encodeURIComponent(service)}`);
    state.detailFetched = Date.now();
  } catch (error) {
    const el = $('c-settings');
    if (el) el.innerHTML = `<span class="muted">${esc(error.message)}</span>`;
    return;
  }
  const el = $('c-settings');
  if (!el) return;
  const s = state.detail.component.settings;
  if (!s) { el.innerHTML = '<span class="muted">no container</span>'; return; }
  const list = (items) => items && items.length
    ? items.map((i) => `<div class="mono small">${esc(i)}</div>`).join('') : '<span class="muted">—</span>';
  el.classList.remove('muted');
  el.innerHTML = `
    <div class="kv">
      <div>Image</div><div class="mono">${esc(s.image)}</div>
      <div>Entrypoint</div><div class="mono small">${esc(s.entrypoint || '—')}</div>
      <div>Command</div><div class="mono small">${esc(s.command || '—')}</div>
      <div>Restart policy</div><div>${esc(s.restart_policy || 'no')}</div>
      <div>Ports</div><div>${list(s.ports)}</div>
      <div>Mounts</div><div>${list(s.mounts)}</div>
      <div>Networks</div><div>${list(s.networks)}</div>
      <div>IPC / PID namespace</div><div class="mono small">${esc(s.ipc_mode || '—')} / ${esc(s.pid_mode || 'own')}</div>
    </div>
    <h3>Environment</h3>
    ${s.env.length ? `<table><tbody>${s.env.map((e) => {
      const i = e.indexOf('=');
      return `<tr><td class="mono small">${esc(e.slice(0, i))}</td><td class="mono small">${esc(e.slice(i + 1))}</td></tr>`;
    }).join('')}</tbody></table>` : '<div class="muted">none</div>'}
    ${state.detail.files.map((f) => `<h3>${esc(f.path)} <span class="small muted">(read from the running container)</span></h3>` +
      (f.error ? `<div class="muted">${esc(f.error)}</div>` : `<pre class="file">${esc(f.content)}</pre>`)).join('')}`;
}

async function refreshLogs(service, force) {
  if (state.logPaused && !force) return;
  let data;
  try {
    data = await api(`/api/components/${encodeURIComponent(service)}/logs/${state.logKind}`);
  } catch (error) {
    const box = $('log-box');
    if (box) box.innerHTML = `<div class="err">${esc(error.message)}</div>`;
    return;
  }
  const sources = $('log-sources');
  if (sources) {
    sources.innerHTML = data.sources.map((s) =>
      `<li>${esc(s.label)} — <span class="${s.status === 'subscribed' || s.status === 'polling' || s.status === 'read on request' ? '' : 'err'}">${esc(s.status)}</span></li>`).join('') +
      data.errors.map((e) => `<li class="err">${esc(e)}</li>`).join('');
  }
  const filter = state.logFilter.toLowerCase();
  const entries = filter ? data.entries.filter((e) => e.text.toLowerCase().includes(filter)) : data.entries;
  const last = entries[entries.length - 1];
  const key = state.logKind + entries.length + (last ? last.ts_ms + last.text : '');
  if (key === state.logKey) return;
  state.logKey = key;
  const box = $('log-box');
  if (!box) return;
  const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  box.innerHTML = entries.length
    ? entries.map((e) => `<div class="l"><span class="t">${clock(e.ts_ms)}</span> <span class="s">[${esc(e.source)}]</span> ${esc(e.text)}</div>`).join('')
    : '<div class="empty">nothing observed yet</div>';
  if (atBottom || force) box.scrollTop = box.scrollHeight;
}

// --- OpenSOVD DTCs --------------------------------------------------------------

function dtcSkeleton() {
  return `
    <div class="panel" id="dtc-panel">
      <div class="row spread">
        <h2>Diagnostic Trouble Codes <span class="muted small" id="dtc-source"></span></h2>
        <div class="row">
          <select class="sel" id="dtc-filter">
            <option value="all">all DTCs</option>
            <option value="active">active only</option>
            <option value="history">failed since last clear</option>
          </select>
          <button class="btn" id="dtc-print">Print report (PDF)</button>
          <button class="btn danger" id="dtc-clear-all">Clear all faults</button>
        </div>
      </div>
      <div class="tiles" id="dtc-tiles"></div>
      <table>
        <thead><tr><th>Severity</th><th>DTC</th><th>Description</th><th>Category</th><th>Status</th><th>Occurrences</th><th></th></tr></thead>
        <tbody id="dtc-body"><tr><td colspan="7" class="muted">loading…</td></tr></tbody>
      </table>
      <div class="note">Severity colors: <span class="badge sev Fatal">Fatal</span> <span class="badge sev Error">Error</span>
        <span class="badge sev Warn">Warn</span> <span class="badge sev Info">Info</span> <span class="badge sev Debug">Debug</span>
        — names, summaries, and categories from the DFM fault catalog. Clearing uses OpenSOVD's
        <span class="mono">DELETE …/faults</span>; DFM resets the status and counters of the cleared faults.</div>
    </div>`;
}

function wireDtc() {
  $('dtc-filter').value = state.sovdFilter;
  $('dtc-filter').onchange = (e) => { state.sovdFilter = e.target.value; fillDtc(); };
  $('dtc-print').onclick = printDtcReport;
  $('dtc-clear-all').onclick = async () => {
    if (!confirm('Clear ALL faults of ' + ((state.sovd && state.sovd.entity) || 'the entity') + ' in OpenSOVD? Status and counters are reset.')) return;
    try {
      await change('/api/sovd/faults', 'DELETE');
      toast('All faults cleared');
      state.faultDetails = {};
      await refreshSovd();
    } catch (error) {
      toast('Clearing failed: ' + error.message, true);
    }
  };
  $('dtc-body').addEventListener('click', onDtcClick);
}

async function refreshSovd() {
  try {
    state.sovd = await api('/api/sovd/faults');
    state.sovdError = null;
  } catch (error) {
    state.sovdError = error.message;
  }
  await Promise.all([...state.expanded].map(loadFaultDetail));
  fillDtc();
}

async function loadFaultDetail(code) {
  try {
    state.faultDetails[code] = await api('/api/sovd/faults/' + encodeURIComponent(code));
  } catch (error) {
    state.faultDetails[code] = { error: error.message };
  }
}

function faultFlags(f) {
  const s = f.status || {};
  return {
    active: !!s.testFailed,
    confirmed: !!s.confirmedDtc,
    pending: !!s.pendingDtc,
    history: !!s.testFailedSinceLastClear,
    lamp: !!s.warningIndicatorRequested,
    notTested: !!s.testNotCompletedSinceLastClear && !s.testFailedSinceLastClear && !s.testFailed,
  };
}

function statusBadges(f) {
  const x = faultFlags(f);
  const out = [];
  if (x.active) out.push('<span class="badge bad">ACTIVE</span>');
  if (x.confirmed) out.push('<span class="badge bad">CONFIRMED</span>');
  if (x.pending) out.push('<span class="badge warn">PENDING</span>');
  if (!x.active && x.history) out.push('<span class="badge warn">HISTORY</span>');
  if (x.lamp) out.push('<span class="badge warn">⚠ warning lamp</span>');
  if (x.notTested) out.push('<span class="badge idle">NOT TESTED</span>');
  else if (!x.active && !x.history) out.push('<span class="badge ok">PASSED</span>');
  return out.join(' ');
}

function sortedFaults() {
  const items = (state.sovd && state.sovd.items) || [];
  return [...items].sort((a, b) => {
    const fa = faultFlags(a); const fb = faultFlags(b);
    return (fb.active - fa.active)
      || ((SEVERITY_RANK[b.severity_name] ?? -1) - (SEVERITY_RANK[a.severity_name] ?? -1))
      || (fb.history - fa.history)
      || a.code.localeCompare(b.code);
  });
}

function fillDtc() {
  const body = $('dtc-body');
  if (!body) return;
  if (state.sovdError && !state.sovd) {
    body.innerHTML = `<tr><td colspan="7" class="muted">OpenSOVD unavailable: ${esc(state.sovdError)}</td></tr>`;
    return;
  }
  if (!state.sovd) return;
  const all = sortedFaults();
  const flags = all.map(faultFlags);
  const count = (pred) => flags.filter(pred).length;
  $('dtc-source').textContent = `· ${state.sovd.entity} · catalog v${state.sovd.catalog_version ?? '?'}` +
    (state.sovdError ? ' · last refresh failed: ' + state.sovdError : '');
  $('dtc-tiles').innerHTML = `
    <div class="tile"><div class="n">${all.length}</div><div class="l">DTCs in catalog</div></div>
    <div class="tile bad"><div class="n">${count((x) => x.active)}</div><div class="l">active</div></div>
    <div class="tile bad"><div class="n">${count((x) => x.confirmed)}</div><div class="l">confirmed</div></div>
    <div class="tile warn"><div class="n">${count((x) => x.history)}</div><div class="l">failed since clear</div></div>
    <div class="tile"><div class="n">${count((x) => x.notTested)}</div><div class="l">not tested</div></div>`;
  const shown = all.filter((f) => {
    const x = faultFlags(f);
    return state.sovdFilter === 'all' || (state.sovdFilter === 'active' ? x.active : x.history || x.active);
  });
  body.innerHTML = shown.length ? shown.map((f) => {
    const cat = f.catalog || {};
    const open = state.expanded.has(f.code);
    return `
      <tr class="clickable sev-row-${esc(f.severity_name)}" data-code="${esc(f.code)}">
        <td><span class="badge sev ${esc(f.severity_name)}">${esc(f.severity_name)}</span></td>
        <td class="mono">${esc(f.code)}</td>
        <td>${esc(cat.summary || f.symptom || '')}</td>
        <td>${esc(cat.category || '—')}</td>
        <td>${statusBadges(f)}<div class="small muted mono">mask ${esc(f.status && f.status.mask)}</div></td>
        <td>${esc(f.occurrence_counter ?? '—')}<div class="small muted">aging ${esc(f.aging_counter ?? '—')} · healing ${esc(f.healing_counter ?? '—')}</div></td>
        <td><div class="row">
          <button class="btn small" data-detail="${esc(f.code)}">${open ? 'Hide' : 'Details'}</button>
          <button class="btn small danger" data-clear="${esc(f.code)}">Clear</button></div></td>
      </tr>
      ${open ? `<tr><td colspan="7" class="detail">${faultDetailHtml(f.code)}</td></tr>` : ''}`;
  }).join('') : '<tr><td colspan="7" class="muted">no DTC matches the filter</td></tr>';
}

function faultDetailHtml(code) {
  const d = state.faultDetails[code];
  if (!d) return '<span class="muted">loading…</span>';
  if (d.error) return `<span class="muted">${esc(d.error)}</span>`;
  const env = d.environment_data;
  const status = d.status || {};
  return `
    <div class="kv small">
      <div>Symptom</div><div>${esc(d.symptom || '—')}</div>
      <div>Status bits</div><div class="mono">${Object.entries(status).map(([k, v]) => `${esc(k)}=${esc(v)}`).join(' · ')}</div>
    </div>
    <h3>Environment data</h3>
    ${env && Object.keys(env).length
      ? `<div class="kv small envdata">${Object.entries(env).map(([k, v]) =>
        `<div>${esc(k)}</div><div class="mono">${esc(typeof v === 'object' ? JSON.stringify(v) : v)}</div>`).join('')}</div>`
      : '<div class="muted small">none stored for this fault</div>'}`;
}

async function onDtcClick(e) {
  const clear = e.target.closest('button[data-clear]');
  if (clear) {
    e.stopPropagation();
    const code = clear.dataset.clear;
    if (!confirm(`Clear ${code} in OpenSOVD?`)) return;
    try {
      await change('/api/sovd/faults/' + encodeURIComponent(code), 'DELETE');
      toast(code + ' cleared');
      delete state.faultDetails[code];
      await refreshSovd();
    } catch (error) {
      toast(`Clearing ${code} failed: ${error.message}`, true);
    }
    return;
  }
  const target = e.target.closest('[data-detail]') || e.target.closest('tr[data-code]');
  if (!target) return;
  const code = target.dataset.detail || target.dataset.code;
  if (state.expanded.has(code)) state.expanded.delete(code);
  else { state.expanded.add(code); await loadFaultDetail(code); }
  fillDtc();
}

async function printDtcReport() {
  if (!state.sovd) { toast('No fault list loaded', true); return; }
  const faults = sortedFaults();
  const withHistory = faults.filter((f) => faultFlags(f).history || faultFlags(f).active);
  await Promise.all(withHistory.map((f) => loadFaultDetail(f.code)));
  const flags = faults.map(faultFlags);
  const plain = (f) => {
    const x = faultFlags(f);
    return [x.active && 'ACTIVE', x.confirmed && 'CONFIRMED', x.pending && 'PENDING',
      !x.active && x.history && 'HISTORY', x.lamp && 'WARNING LAMP',
      x.notTested ? 'NOT TESTED' : (!x.active && !x.history && 'PASSED')].filter(Boolean).join(', ');
  };
  printReport(`
    <h1>Diagnostic Trouble Code Report</h1>
    <div class="meta">Entity <b>${esc(state.sovd.entity)}</b> · source ${esc(state.sovd.url)} ·
      catalog version ${esc(state.sovd.catalog_version ?? '?')} · generated ${esc(new Date().toLocaleString())}<br>
      ${faults.length} DTCs · ${flags.filter((x) => x.active).length} active ·
      ${flags.filter((x) => x.confirmed).length} confirmed · ${flags.filter((x) => x.history).length} failed since last clear</div>
    <table>
      <thead><tr><th>Severity</th><th>DTC</th><th>Summary</th><th>Category</th><th>Status</th><th>Occ.</th><th>Mask</th></tr></thead>
      <tbody>${faults.map((f) => `<tr>
        <td><span class="badge sev ${esc(f.severity_name)}">${esc(f.severity_name)}</span></td>
        <td>${esc(f.code)}</td><td>${esc((f.catalog && f.catalog.summary) || f.symptom || '')}</td>
        <td>${esc((f.catalog && f.catalog.category) || '')}</td><td>${esc(plain(f))}</td>
        <td>${esc(f.occurrence_counter ?? '')}</td><td>${esc(f.status && f.status.mask)}</td></tr>`).join('')}</tbody>
    </table>
    ${withHistory.length ? '<h2>Environment data of failed DTCs</h2>' : ''}
    ${withHistory.map((f) => {
      const env = (state.faultDetails[f.code] || {}).environment_data || {};
      const rows = Object.entries(env);
      return `<div class="scenario-print"><b>${esc(f.code)}</b> — ${esc(plain(f))}
        ${rows.length ? `<table><tbody>${rows.map(([k, v]) => `<tr><td>${esc(k)}</td><td>${esc(typeof v === 'object' ? JSON.stringify(v) : v)}</td></tr>`).join('')}</tbody></table>`
          : '<div class="meta">no environment data stored</div>'}</div>`;
    }).join('')}`);
}

// --- campaign ----------------------------------------------------------------------

function renderCampaignSkeleton() {
  $('view').innerHTML = `
    <div id="cp-banner" class="banner"><span class="dot idle"></span><span class="muted">loading…</span></div>
    <div class="panel" id="cp-control">
      <div class="row spread">
        <h2 style="margin:0">Run a campaign</h2>
        <div class="row">
          <label class="small"><input type="checkbox" id="cp-build" ${state.rebuild ? 'checked' : ''}> rebuild the Guardian and VSS Publisher images first</label>
          <button class="btn primary" id="cp-start">Start campaign</button>
          <button class="btn danger" id="cp-stop">Stop campaign</button>
        </div>
      </div>
      <div class="row small" style="margin:10px 0 6px">
        <label><input type="checkbox" id="cp-all" ${state.pickAll ? 'checked' : ''}> all scenarios</label>
        <span class="muted" id="cp-count"></span>
      </div>
      <div class="pick" id="cp-list"><span class="muted small">loading scenarios…</span></div>
      <div class="note" id="cp-runner-info"></div>
      <div class="log" id="cp-runner-log" style="height:200px;margin-top:8px"><div class="empty">no campaign started from the dashboard yet</div></div>
    </div>
    <div class="panel">
      <div class="row spread">
        <div class="row">
          <h2 style="margin:0">Campaign report</h2>
          <select class="sel" id="cp-select"></select>
          <label class="small"><input type="checkbox" id="cp-follow" ${state.followLive ? 'checked' : ''}> follow the running campaign</label>
        </div>
        <button class="btn" id="cp-print">Print report (PDF)</button>
      </div>
      <div style="margin-top:14px" id="cp-summary"></div>
    </div>
    <div id="cp-scenarios"></div>`;
  $('cp-select').onchange = (e) => { state.campaignId = e.target.value; state.followLive = false; $('cp-follow').checked = false; refreshCampaign(); };
  $('cp-follow').onchange = (e) => { state.followLive = e.target.checked; refreshCampaign(); };
  $('cp-print').onclick = printCampaignReport;
  $('cp-build').onchange = (e) => { state.rebuild = e.target.checked; };
  $('cp-all').onchange = (e) => { state.pickAll = e.target.checked; fillScenarioPicker(); };
  $('cp-list').onchange = (e) => {
    const box = e.target.closest('input[data-scenario]');
    if (!box) return;
    if (box.checked) state.picked.add(box.dataset.scenario); else state.picked.delete(box.dataset.scenario);
    fillScenarioPicker();
  };
  $('cp-start').onclick = startCampaign;
  $('cp-stop').onclick = stopCampaign;
  state.runnerKey = '';
  loadScenarios();
  $('cp-scenarios').addEventListener('click', (e) => {
    const head = e.target.closest('.scenario > .head');
    if (!head) return;
    const id = head.parentElement.dataset.id;
    if (state.collapsed.has(id)) state.collapsed.delete(id); else state.collapsed.add(id);
    head.parentElement.classList.toggle('collapsed');
    if (!head.parentElement.classList.contains('collapsed')) fillCampaign();
  });
  $('cp-scenarios').addEventListener('mousemove', signalHover);
  $('cp-scenarios').addEventListener('mouseleave', () => { state.hover = null; });
}

async function refreshCampaign() {
  try {
    const list = await api('/api/campaigns');
    state.campaigns = list.campaigns;
    state.runsDir = list.runs_dir;
  } catch (error) {
    state.campaignError = error.message;
  }
  const running = state.overview && state.overview.campaign.running;
  if (state.followLive && running) state.campaignId = running.id;
  if (!state.campaignId && state.campaigns.length) state.campaignId = state.campaigns[0].id;
  fillCampaignBanner(running);
  fillRunner();
  fillCampaignSelect();
  if (!state.campaignId) {
    $('cp-summary').innerHTML = `<div class="muted">No campaign in <span class="mono">${esc(state.runsDir || 'runs/')}</span> yet.
      Start one above.</div>`;
    $('cp-scenarios').innerHTML = '';
    return;
  }
  try {
    state.campaign = await api('/api/campaigns/' + encodeURIComponent(state.campaignId));
    state.campaignError = null;
  } catch (error) {
    state.campaignError = error.message;
  }
  fillCampaign();
}

function fillCampaignBanner(running) {
  const el = $('cp-banner');
  if (!el) return;
  if (running) {
    el.className = 'banner live';
    el.innerHTML = `<span class="dot live"></span><div><strong>Campaign ${esc(running.id)} is running</strong>
      <div class="muted small">${running.scenario ? 'current scenario <span class="mono">' + esc(running.scenario) + '</span> · ' : 'between scenarios · '}${running.done} of ${running.total} scenarios done</div></div>`;
  } else {
    el.className = 'banner';
    el.innerHTML = `<span class="dot idle"></span><div><strong>No campaign running</strong>
      <div class="muted small">Start one below, or on the host with <span class="mono">cargo run -p campaign -- run --all</span>.
      The report appears here while it runs.</div></div>`;
  }
}

function fillCampaignSelect() {
  const select = $('cp-select');
  if (!select) return;
  const options = state.campaigns.map((c) => {
    const label = `${c.id} · ${c.mode} · ${c.state}` + (c.scenarios ? ` · ${c.pass} pass / ${c.fail} fail / ${c.inconclusive} inconcl. of ${c.scenarios}` : '');
    return `<option value="${esc(c.id)}" ${c.id === state.campaignId ? 'selected' : ''}>${esc(label)}</option>`;
  }).join('');
  if (select.dataset.options !== options) {
    select.innerHTML = options;
    select.dataset.options = options;
  }
  select.value = state.campaignId || '';
}

function scenarioVerdict(s) {
  if (s.report) return s.report.verdict;
  return { running: 'RUNNING', pending: 'PENDING', incomplete: 'INCOMPLETE' }[s.state] || 'PENDING';
}

function expectationText(e) {
  if (!e) return '';
  const parts = [e.kind];
  for (const key of ['dtc', 'state', 'mitigation']) if (e[key]) parts.push(e[key]);
  if (e.after) parts.push('after ' + (typeof e.after === 'string' ? e.after : JSON.stringify(e.after)));
  return parts.join(' ');
}

function campaignCounts(c) {
  const verdicts = c.scenarios.map(scenarioVerdict);
  const n = (v) => verdicts.filter((x) => x === v).length;
  return { total: verdicts.length, pass: n('PASS'), fail: n('FAIL'), inconclusive: n('INCONCLUSIVE'),
    running: n('RUNNING'), pending: n('PENDING') + n('INCOMPLETE') };
}

function fillCampaign() {
  const c = state.campaign;
  const summary = $('cp-summary');
  if (!summary) return;
  if (!c) {
    summary.innerHTML = `<div class="muted">${esc(state.campaignError || 'loading…')}</div>`;
    return;
  }
  const k = campaignCounts(c);
  const pct = (x) => (k.total ? 100 * x / k.total : 0).toFixed(1) + '%';
  summary.innerHTML = `
    <div class="kv small" style="margin-bottom:12px">
      <div>Campaign</div><div class="mono">${esc(c.id)} (${esc(c.mode)})</div>
      <div>State</div><div>${c.state === 'running' ? '<span class="badge info">running</span>' : c.state === 'done' ? '<span class="badge ok">finished</span>' : '<span class="badge warn">' + esc(c.state) + ' — the tool stopped before the end</span>'}</div>
      <div>Started</div><div>${esc(c.started_at || '—')}</div>
    </div>
    <div class="tiles">
      <div class="tile"><div class="n">${k.total}</div><div class="l">scenarios</div></div>
      <div class="tile ok"><div class="n">${k.pass}</div><div class="l">pass</div></div>
      <div class="tile bad"><div class="n">${k.fail}</div><div class="l">fail</div></div>
      <div class="tile warn"><div class="n">${k.inconclusive}</div><div class="l">inconclusive</div></div>
      <div class="tile"><div class="n">${k.running + k.pending}</div><div class="l">${c.state === 'running' ? 'running / to come' : 'not judged'}</div></div>
    </div>
    <div class="progress"><div class="p" style="width:${pct(k.pass)}"></div><div class="f" style="width:${pct(k.fail)}"></div>
      <div class="i" style="width:${pct(k.inconclusive)}"></div><div class="r" style="width:${pct(k.running)}"></div></div>`;
  $('cp-scenarios').innerHTML = c.scenarios.map(scenarioCard).join('');
}

function scenarioCard(s) {
  const verdict = scenarioVerdict(s);
  const r = s.report;
  const collapsed = state.collapsed.has(s.id) || (!r && s.state !== 'running');
  let body;
  if (r) {
    body = scenarioReportHtml(r, s, !collapsed);
  } else if (s.state === 'running') {
    body = `<div class="row"><span class="dot live"></span> running — ${s.observations} observations recorded so far
      <span class="muted small">(samples, Guardian events, OpenSOVD changes)</span></div>
      ${s.manifest ? signalHtml(s.manifest.run_id, null, true) : ''}`;
  } else {
    body = `<div class="muted">${s.state === 'pending' ? 'not started yet' : 'not judged: the campaign tool stopped during this scenario'}</div>`;
  }
  if (s.error) body += `<h3>Run error</h3><pre class="file">${esc(s.error)}</pre>`;
  return `
    <div class="scenario ${collapsed ? 'collapsed' : ''}" data-id="${esc(s.id)}">
      <div class="head">
        <span class="verdict ${esc(verdict)}">${esc(verdict)}</span>
        <span class="id">${esc(s.id)}</span>
        ${r && r.status === 'planned' ? '<span class="badge idle">checks a planned requirement</span>' : ''}
        ${r && r.chain ? `<span class="badge ${r.chain.complete ? 'ok' : 'bad'}">chain ${r.chain.complete ? 'complete' : 'incomplete'}</span>` : ''}
        <span class="muted small grow">${esc(r ? r.description : '')}</span>
        ${r ? `<span class="small muted">${esc(r.samples)} samples · ${esc(r.guardian_events)} events</span>` : ''}
      </div>
      <div class="body">${body}</div>
    </div>`;
}

const LINK_STATE = {
  present: ['ok', '✓ present'],
  missing: ['bad', '✗ missing'],
  not_expected: ['idle', '— not expected'],
  unexpected: ['warn', '! unexpected'],
};

function sampleText(s) {
  return s ? `#${s.sequence} (counter ${s.alive_counter})` : '—';
}

function dtcStatusBadges(status) {
  if (!status) return '<span class="badge idle">never shown</span>';
  const flags = [['testFailed', 'bad', 'testFailed'], ['confirmedDtc', 'bad', 'confirmed'], ['pendingDtc', 'warn', 'pending'],
    ['testFailedSinceLastClear', 'warn', 'failed since clear'], ['warningIndicatorRequested', 'warn', '⚠ warning lamp']];
  const set = flags.filter(([key]) => status[key] === true)
    .map(([, cls, label]) => `<span class="badge ${cls}">${label}</span>`);
  return (set.length ? set.join(' ') : '<span class="badge ok">no flag set</span>')
    + ` <span class="small muted mono">mask ${esc(status.mask)}</span>`;
}

// Hazard → safety goal → fault → detection → mitigation → DTC → verdict,
// as judged by the campaign tool (evaluate.rs).
function chainHtml(chain) {
  if (!chain) {
    return `<h3>Evidence chain</h3><div class="muted small">This report was written by an older campaign tool and has no
      evidence chain. Judge it again with <span class="mono">cargo run -p campaign -- evaluate &lt;run-dir&gt;</span>.</div>`;
  }
  const flow = chain.links.map((l) => `<span class="chain-step ${esc(l.state)}" title="${esc(l.evidence)}">${esc(l.link)}</span>`)
    .join('<span class="chain-arrow">→</span>');
  const links = chain.links.map((l) => {
    const [cls, text] = LINK_STATE[l.state] || ['idle', l.state];
    return `<tr><td><b>${esc(l.link)}</b></td><td>${esc(l.evidence)}</td><td><span class="badge ${cls}">${text}</span></td></tr>`;
  }).join('');
  const detections = chain.detections.length ? `
    <h4>Detection</h4>
    <table><thead><tr><th>Event</th><th>Detection</th><th>After t0</th><th>Guardian time</th><th>Sample</th><th>Recovered</th></tr></thead><tbody>
    ${chain.detections.map((d) => `<tr><td class="mono">#${esc(d.event_id)}</td><td>${esc(d.event)}</td>
      <td>${esc(seconds(d.latency_ms))}</td><td class="mono small">${esc(d.guardian_time_ms)} ms</td><td class="mono small">${esc(sampleText(d.sample))}</td>
      <td>${d.recovered_event_id ? `#${esc(d.recovered_event_id)} at ${esc(seconds(d.recovered_ms))}` : '—'}</td></tr>`).join('')}
    </tbody></table>` : '';
  const mitigations = chain.mitigations.length ? `
    <h4>Mitigation</h4>
    <table><thead><tr><th>Event</th><th>Mitigation</th><th>After t0</th><th>Cause chain (correlation IDs)</th></tr></thead><tbody>
    ${chain.mitigations.map((m) => `<tr><td class="mono">#${esc(m.event_id)}</td><td><b>${esc(m.mitigation)}</b></td>
      <td>${esc(seconds(m.latency_ms))}</td>
      <td class="small">${m.cause_chain.map(esc).join(' <span class="chain-arrow">→</span> ')}
        ${m.detection_event_id ? '' : ' <span class="badge bad">not caused by a detection</span>'}</td></tr>`).join('')}
    </tbody></table>` : '';
  const diagnostics = chain.diagnostics.length ? `
    <h4>DTCs in OpenSOVD</h4>
    <table><thead><tr><th>DTC</th><th>Detection</th><th>Failed in OpenSOVD</th><th>Severity</th><th>Fault type</th><th>Status</th><th>Occ.</th><th>Passed later</th></tr></thead><tbody>
    ${chain.diagnostics.map((d) => `<tr><td class="mono">${esc(d.dtc)}${d.symptom ? `<div class="small muted">${esc(d.symptom)}</div>` : ''}</td>
      <td class="mono">#${esc(d.detection_event_id)}</td>
      <td>${d.latency_ms == null ? '<span class="badge bad">never</span>' : esc(seconds(d.latency_ms)) + ' after the event'}</td>
      <td>${d.severity ? `<span class="badge sev ${esc(d.severity)}">${esc(d.severity)}</span>` : '—'}</td>
      <td>${esc(d.fault_type || '—')}</td><td>${dtcStatusBadges(d.status)}</td>
      <td>${esc(d.occurrence_counter ?? '—')}</td><td>${d.passed_later ? 'yes' : 'no'}</td></tr>
      ${d.environment_data && Object.keys(d.environment_data).length ? `<tr><td colspan="8" class="detail"><div class="kv small">
        ${Object.entries(d.environment_data).map(([k, v]) => `<div>${esc(k)}</div><div class="mono">${esc(typeof v === 'object' ? JSON.stringify(v) : v)}</div>`).join('')}
      </div></td></tr>` : ''}`).join('')}
    </tbody></table>` : '';
  return `
    <h3>Evidence chain <span class="badge ${chain.complete ? 'ok' : 'bad'}">${chain.complete ? 'complete' : 'incomplete'}</span></h3>
    <div class="chain-flow">${flow}</div>
    <table><thead><tr><th>Link</th><th>Evidence (linked by session and event IDs)</th><th></th></tr></thead><tbody>${links}</tbody></table>
    ${detections}${mitigations}${diagnostics}`;
}

function timelineHtml(timeline, open) {
  if (!timeline || !timeline.length) return '';
  const first = timeline[0].session_id;
  return `<details class="timeline" ${open ? 'open' : ''}><summary>Guardian events (${timeline.length})</summary>
    <table><thead><tr><th>#</th><th>Cause</th><th>At the tap</th><th>Guardian time</th><th>Event</th><th>Sample</th></tr></thead><tbody>
    ${timeline.map((e) => `<tr><td class="mono">#${esc(e.event_id)}</td><td class="mono">${e.cause_event_id ? '#' + esc(e.cause_event_id) : '—'}</td>
      <td>${esc(seconds(e.delivered_ms))}</td><td class="mono small">${esc(e.guardian_time_ms)} ms</td>
      <td>${esc(e.event)}${e.session_id !== first ? ` <span class="badge bad">session ${esc(e.session_id)}</span>` : ''}</td>
      <td class="mono small">${esc(sampleText(e.sample))}</td></tr>`).join('')}
    </tbody></table></details>`;
}

function scenarioReportHtml(r, forPrint = false, showSignal = true) {
  const checks = r.checks || [];
  const violations = r.violations || [];
  const requirements = Object.entries(r.requirements || {});
  return `
    <p style="margin-top:0"><b>${esc(r.reason)}</b></p>
    <div class="kv small">
      <div>Hazard → safety goal</div><div>${esc(r.hazard || '—')} → ${esc(r.safety_goal || '—')}</div>
      <div>Injected fault</div><div>${esc(r.description)}</div>
      ${r.hara_tests && r.hara_tests.length ? `<div>HARA test</div><div>${esc(r.hara_tests.join(', '))}</div>` : ''}
      <div>Onset t0</div><div>${r.onset ? esc(seconds(r.onset.t_ms) + ' — ' + r.onset.description) : 'not observed'}</div>
      <div>Guardian session</div><div class="mono">${esc(r.session_id || '—')}</div>
      <div>Run</div><div class="mono">${esc(r.manifest && r.manifest.run_id)} · started ${esc(r.manifest && r.manifest.started_at)} · git ${esc((r.manifest && r.manifest.git_revision) || '—')}</div>
      ${r.note ? `<div>Note</div><div>${esc(r.note)}</div>` : ''}
    </div>
    ${showSignal && r.manifest ? signalHtml(r.manifest.run_id, r, false) : ''}
    ${chainHtml(r.chain)}
    <h3>Checks</h3>
    ${checks.length ? `<table><thead><tr><th>Requirement</th><th>Expectation</th><th>Observed</th><th>Latency</th><th>Budget</th><th>Result</th></tr></thead><tbody>
      ${checks.map((c) => `<tr><td>${esc((c.expectation && c.expectation.requirement) || '—')}</td>
        <td class="mono small">${esc(expectationText(c.expectation))}</td>
        <td>${esc(c.outcome && c.outcome.detail)}</td><td>${esc(seconds(c.latency_ms))}</td><td>${esc(seconds(c.budget_ms))}</td>
        <td class="res-${esc(c.outcome && c.outcome.result)}">${esc(c.outcome && c.outcome.result)}</td></tr>`).join('')}
    </tbody></table>` : '<div class="muted">no expectations</div>'}
    ${violations.length ? `<h3>Forbidden reactions</h3><table><thead><tr><th>Rule</th><th>Requirement</th><th>Detail</th></tr></thead><tbody>
      ${violations.map((v) => `<tr><td>${esc(v.rule)}</td><td>${esc(v.requirement)}</td><td>${esc(v.detail)}</td></tr>`).join('')}</tbody></table>` : ''}
    ${requirements.length ? `<h3>Result per requirement</h3><div class="row">${requirements.map(([req, v]) =>
      `<span class="badge ${v === 'PASS' ? 'ok' : v === 'FAIL' ? 'bad' : 'warn'}">${esc(req)}: ${esc(v)}</span>`).join(' ')}</div>` : ''}
    ${timelineHtml(r.timeline, forPrint)}`;
}

// --- signal plot -------------------------------------------------------------------
// The temperatures the Guardian received, with its thermal state, monitoring
// status, detections, mitigations, the tool's injections, and the DTCs failed
// in OpenSOVD on one time axis. Drawn from the recording (signal.rs).

const SIG = { W: 1000, L: 74, R: 12, T: 14, PH: 180, LANE: 18 };
const SIG_LANES = ['thermal', 'monitoring', 'events', 'DTC'];
// Tool actions that only set up the run; the others are the injected fault.
const SETUP_ACTIONS = /^(start_|guardian_ready)/;

function loadSignal(runId) {
  if (!state.signalLoads.has(runId)) {
    const path = '/api/signal/' + runId.split('/').map(encodeURIComponent).join('/');
    state.signalLoads.set(runId, api(path)
      .then((data) => state.signals.set(runId, { data, at: Date.now() }))
      .catch((error) => state.signals.set(runId, { error: error.message, at: Date.now() }))
      .finally(() => state.signalLoads.delete(runId)));
  }
  return state.signalLoads.get(runId);
}

// A judged run's plot is loaded once; a running one again every poll.
function signalHtml(runId, report, live) {
  if (!runId) return '';
  const entry = state.signals.get(runId);
  // Only the call that starts a load redraws when it lands: every redraw
  // comes here again, and chaining on a pending load would multiply them.
  const stale = !entry || (live && Date.now() - entry.at > POLL_MS);
  if (stale && !state.signalLoads.has(runId)) loadSignal(runId).then(fillCampaign);
  let inner;
  if (!entry) inner = '<div class="muted small">loading signal…</div>';
  else if (entry.error) inner = `<div class="muted small">no signal: ${esc(entry.error)}</div>`;
  else inner = signalSvg(runId, entry.data, report) + signalLegend();
  return `<h3>Signal</h3><div class="signal" data-run="${esc(runId)}">${inner}
    <div class="signal-readout small mono">${esc(signalReadout(runId))}</div></div>`;
}

function signalScale(signal, report) {
  const end = Math.max(signal.end_ms, (report && report.window_end_ms) || 0, 1000);
  const x = (t) => SIG.L + (t / end) * (SIG.W - SIG.L - SIG.R);
  const values = signal.samples.flatMap((p) => [p.max_c, p.avg_c, p.min_c]);
  const lo = values.length ? Math.floor(Math.min(...values)) - 2 : 0;
  const hi = values.length ? Math.ceil(Math.max(...values)) + 2 : 50;
  const y = (v) => SIG.T + SIG.PH - ((v - lo) / (hi - lo)) * SIG.PH;
  return { end, x, y, lo, hi };
}

function niceStep(span, count, steps) {
  return steps.find((s) => span / s <= count) || steps[steps.length - 1];
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted.length ? sorted[Math.floor(sorted.length / 2)] : 0;
}

function signalSvg(runId, signal, report) {
  const { end, x, y, lo, hi } = signalScale(signal, report);
  const top = SIG.T, bottom = SIG.T + SIG.PH;
  const lane0 = bottom + 24;
  const height = lane0 + SIG_LANES.length * SIG.LANE + 4;
  const right = SIG.W - SIG.R;
  const samples = signal.samples;
  const dt = median(samples.slice(1).map((p, i) => p.t_ms - samples[i].t_ms)) || 100;
  const gap = Math.max(400, 4 * dt);
  const out = [];

  // Grid and axes.
  const yStep = niceStep(hi - lo, 5, [1, 2, 5, 10, 20, 50, 100]);
  for (let v = Math.ceil(lo / yStep) * yStep; v <= hi; v += yStep) {
    out.push(`<line class="grid" x1="${SIG.L}" x2="${right}" y1="${y(v)}" y2="${y(v)}"/>
      <text x="${SIG.L - 6}" y="${y(v) + 4}" text-anchor="end">${v}</text>`);
  }
  out.push(`<text x="${SIG.L - 6}" y="${top - 4}" text-anchor="end">°C</text>`);
  const xStep = niceStep(end, 14, [500, 1000, 2000, 5000, 10000, 20000, 30000, 60000]);
  for (let t = 0; t <= end; t += xStep) {
    out.push(`<line class="grid" x1="${x(t)}" x2="${x(t)}" y1="${top}" y2="${bottom}"/>
      <text x="${x(t)}" y="${bottom + 14}" text-anchor="middle">${t / 1000} s</text>`);
  }

  // Missing data: samples marked invalid, and gaps in the stream.
  samples.forEach((p, i) => {
    if (p.quality !== 'VALID') {
      out.push(`<rect class="sig-invalid" x="${x(p.t_ms)}" y="${top}" width="${Math.max(1.5, x(p.t_ms + dt) - x(p.t_ms))}"
        height="${SIG.PH}"><title>${esc(seconds(p.t_ms))}: quality ${esc(p.quality)}</title></rect>`);
    }
    const next = samples[i + 1];
    if (next && next.t_ms - p.t_ms > gap) {
      out.push(`<rect class="sig-gap" x="${x(p.t_ms)}" y="${top}" width="${x(next.t_ms) - x(p.t_ms)}" height="${SIG.PH}">
        <title>no samples for ${esc(seconds(next.t_ms - p.t_ms))}</title></rect>`);
    }
  });

  // After the evaluation window: recorded, but not judged (teardown).
  if (report && report.window_end_ms != null && report.window_end_ms < end) {
    const w = x(report.window_end_ms);
    out.push(`<rect class="sig-after" x="${w}" y="${top}" width="${right - w}" height="${height - 4 - top}">
      <title>after the evaluation window (${esc(seconds(report.window_end_ms))}): recorded, not judged</title></rect>`);
  }

  // Temperatures, broken at gaps and at invalid samples.
  for (const key of ['min_c', 'avg_c', 'max_c']) {
    let d = '', open = false, prev = null;
    for (const p of samples) {
      const ok = p.quality === 'VALID';
      if (!ok || (prev && p.t_ms - prev.t_ms > gap)) open = false;
      if (ok) {
        d += `${open ? 'L' : 'M'}${x(p.t_ms).toFixed(1)} ${y(p[key]).toFixed(1)}`;
        open = true;
      }
      prev = p;
    }
    out.push(`<path class="sig-line sig-${key}" d="${d}"/>`);
  }

  // Injections and the onset t0, across plot and lanes.
  const laneEnd = height - 4;
  for (const inj of signal.injections) {
    const setup = SETUP_ACTIONS.test(inj.action);
    out.push(`<line class="sig-inject ${setup ? 'setup' : ''}" x1="${x(inj.t_ms)}" x2="${x(inj.t_ms)}" y1="${top}" y2="${laneEnd}">
      <title>${esc(seconds(inj.t_ms))}: ${esc(inj.action)} — ${esc(inj.detail)}</title></line>`);
    if (!setup) {
      out.push(`<text class="sig-label" x="${x(inj.t_ms) + 3}" y="${top + 10}">${esc(inj.action)} ${esc(inj.detail)}</text>`);
    }
  }
  if (report && report.onset) {
    const t0 = x(report.onset.t_ms);
    out.push(`<line class="sig-onset" x1="${t0}" x2="${t0}" y1="${top - 6}" y2="${laneEnd}">
      <title>onset t0 ${esc(seconds(report.onset.t_ms))}: ${esc(report.onset.description)}</title></line>
      <text class="sig-label onset" x="${t0 + 3}" y="${top - 2}">t0</text>`);
  }

  // Lanes.
  const laneY = (i) => lane0 + i * SIG.LANE;
  SIG_LANES.forEach((name, i) => {
    out.push(`<text x="${SIG.L - 6}" y="${laneY(i) + 12}" text-anchor="end">${name}</text>`);
  });
  const segments = (changes, i, cls) => changes.forEach((c, k) => {
    const x1 = x(c.t_ms), x2 = x(k + 1 < changes.length ? changes[k + 1].t_ms : signal.end_ms);
    out.push(`<rect class="${cls(c.value)}" x="${x1}" y="${laneY(i) + 2}" width="${Math.max(1, x2 - x1)}" height="${SIG.LANE - 4}">
      <title>${esc(seconds(c.t_ms))}: ${esc(c.value)}</title></rect>`);
    if (x2 - x1 > 8 * c.value.length) {
      out.push(`<text class="lane-text" x="${x1 + 4}" y="${laneY(i) + 12}">${esc(c.value)}</text>`);
    }
  });
  segments(signal.thermal, 0, (v) => 'st st-' + v.toLowerCase());
  segments(signal.monitoring, 1, (v) => 'mon mon-' + (v === 'OK' ? 'ok' : v === 'DEGRADED' ? 'degraded' : 'lost'));

  const shape = { detected: 'M0 -6L6 5L-6 5Z', recovered: 'M0 6L6 -5L-6 -5Z', mitigation: 'M0 -6L6 0L0 6L-6 0Z',
    supervisor: 'M-5 -5H5V5H-5Z' };
  for (const m of signal.markers) {
    out.push(`<path class="mk mk-${esc(m.kind)}" transform="translate(${x(m.t_ms).toFixed(1)} ${laneY(2) + SIG.LANE / 2})"
      d="${shape[m.kind]}"><title>${esc(seconds(m.t_ms))}: ${esc(m.text)}</title></path>`);
  }

  const failedSince = {};
  const dtcBar = (code, from, to) => {
    out.push(`<rect class="sig-dtc" x="${x(from)}" y="${laneY(3) + 2}" width="${Math.max(2, x(to) - x(from))}" height="${SIG.LANE - 4}">
      <title>${esc(code)} failed in OpenSOVD ${esc(seconds(from))} – ${esc(seconds(to))}</title></rect>`);
    if (x(to) - x(from) > 7 * code.length) {
      out.push(`<text class="lane-text on-bad" x="${x(from) + 4}" y="${laneY(3) + 12}">${esc(code)}</text>`);
    }
  };
  for (const d of signal.dtcs) {
    if (d.failed) failedSince[d.code] = d.t_ms;
    else if (failedSince[d.code] != null) { dtcBar(d.code, failedSince[d.code], d.t_ms); delete failedSince[d.code]; }
  }
  for (const [code, from] of Object.entries(failedSince)) dtcBar(code, from, signal.end_ms);

  if (!samples.length) {
    out.push(`<text x="${(SIG.L + right) / 2}" y="${(top + bottom) / 2}" text-anchor="middle">no temperature samples recorded</text>`);
  }
  // Crosshair at the hovered time; kept across the two-second redraws.
  if (state.hover && state.hover.runId === runId) {
    out.push(`<line class="sig-cross" x1="${x(state.hover.t)}" x2="${x(state.hover.t)}" y1="${top}" y2="${laneEnd}"/>`);
  }
  return `<svg viewBox="0 0 ${SIG.W} ${height}" role="img" aria-label="Battery temperature and Guardian reaction over time">
    ${out.join('')}</svg>`;
}

function signalLegend() {
  const item = (cls, text) => `<span><i class="${cls}"></i>${text}</span>`;
  return `<div class="signal-legend small muted">
    ${item('lg-line sig-max_c', 'max')}${item('lg-line sig-avg_c', 'avg')}${item('lg-line sig-min_c', 'min')}
    ${item('lg-box sig-invalid', 'quality not VALID')}${item('lg-box sig-gap', 'no samples')}
    ${item('lg-vline onset', 't0 onset')}${item('lg-vline inject', 'injection')}
    ${item('lg-mk mk-detected', 'detected')}${item('lg-mk mk-recovered', 'recovered')}
    ${item('lg-mk mk-mitigation', 'mitigation')}${item('lg-mk mk-supervisor', 'watchdog')}
    ${item('lg-box sig-dtc', 'DTC failed')}${item('lg-box sig-after', 'after the evaluation window')}</div>`;
}

function signalReadout(runId) {
  const entry = state.signals.get(runId);
  if (!state.hover || state.hover.runId !== runId || !entry || !entry.data) return 'hover the plot for values';
  const { t } = state.hover;
  const signal = entry.data;
  let best = null;
  for (const p of signal.samples) if (!best || Math.abs(p.t_ms - t) < Math.abs(best.t_ms - t)) best = p;
  const at = (changes) => (changes.filter((c) => c.t_ms <= t).pop() || { value: '—' }).value;
  const sample = best && Math.abs(best.t_ms - t) < 1000
    ? `sample at ${seconds(best.t_ms)}: max ${best.max_c.toFixed(1)} · avg ${best.avg_c.toFixed(1)} · min ${best.min_c.toFixed(1)} °C · ${best.quality}`
    : 'no sample nearby';
  return `${seconds(t)} — ${sample} — thermal ${at(signal.thermal)} · monitoring ${at(signal.monitoring)}`;
}

function signalHover(e) {
  const box = e.target.closest('.signal');
  const svg = box && box.querySelector('svg');
  if (!svg) return;
  const runId = box.dataset.run;
  const entry = state.signals.get(runId);
  if (!entry || !entry.data) return;
  const rect = svg.getBoundingClientRect();
  const px = ((e.clientX - rect.left) / rect.width) * SIG.W;
  if (px < SIG.L || px > SIG.W - SIG.R) return;
  const report = state.campaign && (state.campaign.scenarios.find((s) => s.report && s.report.manifest
    && s.report.manifest.run_id === runId) || {}).report;
  const { end } = signalScale(entry.data, report);
  state.hover = { runId, t: Math.round(((px - SIG.L) / (SIG.W - SIG.L - SIG.R)) * end) };
  box.querySelector('.signal-readout').textContent = signalReadout(runId);
  let cross = svg.querySelector('.sig-cross');
  if (!cross) {
    cross = document.createElementNS('http://www.w3.org/2000/svg', 'line');
    cross.setAttribute('class', 'sig-cross');
    svg.appendChild(cross);
  }
  const vb = svg.viewBox.baseVal;
  cross.setAttribute('x1', px); cross.setAttribute('x2', px);
  cross.setAttribute('y1', SIG.T); cross.setAttribute('y2', vb.height - 4);
}

async function loadScenarios() {
  try {
    state.scenarioList = (await api('/api/campaign/scenarios')).scenarios;
  } catch (error) {
    $('cp-list').innerHTML = `<span class="err small">${esc(error.message)}</span>`;
    return;
  }
  fillScenarioPicker();
}

function fillScenarioPicker() {
  const list = $('cp-list');
  if (!list || !state.scenarioList) return;
  list.innerHTML = state.scenarioList.map((s) => `
    <label class="${state.pickAll ? 'off' : ''}" title="${esc(s.description)}">
      <input type="checkbox" data-scenario="${esc(s.id)}" ${state.pickAll || state.picked.has(s.id) ? 'checked' : ''} ${state.pickAll ? 'disabled' : ''}>
      <span class="mono">${esc(s.id)}</span>${s.status === 'planned' ? ' <span class="badge idle">planned</span>' : ''}
      <div class="small muted">${esc(s.description)}</div>
    </label>`).join('');
  const n = state.pickAll ? state.scenarioList.length : state.picked.size;
  $('cp-count').textContent = `${n} of ${state.scenarioList.length} scenarios selected`;
}

const EXIT_MEANING = {
  0: 'every implemented scenario passed',
  1: 'at least one implemented scenario did not pass',
  2: 'the campaign tool itself failed',
};

function fillRunner() {
  const runner = state.overview && state.overview.campaign.runner;
  const info = $('cp-runner-info');
  if (!info || !runner) return;
  const leftovers = (state.overview.campaign.projects || []).length > 0;
  $('cp-start').disabled = !runner.available || runner.running || state.busy;
  $('cp-stop').disabled = state.busy || !(runner.running || leftovers);
  let text;
  if (!runner.available) {
    text = `Campaigns cannot be started here: ${esc(runner.unavailable)}`;
  } else if (runner.running) {
    text = `<span class="dot live"></span> running <span class="mono">campaign ${esc((runner.args || []).join(' '))}</span>
      since ${esc(new Date(runner.started_at).toLocaleTimeString())}`;
  } else if (runner.exists) {
    text = `Last run <span class="mono">campaign ${esc((runner.args || []).join(' '))}</span> ended with exit code
      ${esc(runner.exit_code)}: ${EXIT_MEANING[runner.exit_code] || 'stopped'}.`;
  } else {
    text = 'The campaign runs in its own container (campaign-runner) with the repository mounted from the host; its evidence goes to runs/ as usual.';
  }
  info.innerHTML = text;
  const log = runner.log || [];
  const last = log[log.length - 1];
  const key = log.length + (last ? last.ts_ms + last.text : '');
  if (key === state.runnerKey || !log.length) return;
  state.runnerKey = key;
  const box = $('cp-runner-log');
  const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  box.innerHTML = log.map((e) => `<div class="l"><span class="t">${clock(e.ts_ms)}</span> ${esc(e.text)}</div>`).join('');
  if (atBottom) box.scrollTop = box.scrollHeight;
}

async function startCampaign() {
  const scenarios = state.pickAll ? [] : [...state.picked];
  if (!state.pickAll && scenarios.length === 0) { toast('Select at least one scenario', true); return; }
  if (state.busy) return;
  state.busy = true;
  try {
    const result = await change('/api/campaign/start', 'POST', { scenarios, build: state.rebuild });
    toast('Started: ' + result.done.join('\n'));
    state.followLive = true;
    const follow = $('cp-follow');
    if (follow) follow.checked = true;
  } catch (error) {
    toast('Cannot start the campaign: ' + error.message, true);
  } finally {
    state.busy = false;
    tick();
  }
}

async function stopCampaign() {
  if (!confirm('Stop the running campaign? The scenario in progress stays unjudged; its Compose project is removed.')) return;
  if (state.busy) return;
  state.busy = true;
  try {
    const result = await change('/api/campaign/stop');
    toast(result.done.length ? result.done.join('\n') : 'nothing to stop');
  } catch (error) {
    toast('Cannot stop the campaign: ' + error.message, true);
  } finally {
    state.busy = false;
    tick();
  }
}

async function printCampaignReport() {
  const c = state.campaign;
  if (!c) { toast('No campaign loaded', true); return; }
  await Promise.all(c.scenarios.filter((s) => s.report && s.report.manifest)
    .map((s) => loadSignal(s.report.manifest.run_id)));
  const k = campaignCounts(c);
  printReport(`
    <h1>Fault Campaign Report</h1>
    <div class="meta">Campaign <b>${esc(c.id)}</b> (${esc(c.mode)}) · ${esc(c.state)} · started ${esc(c.started_at || '—')} ·
      generated ${esc(new Date().toLocaleString())}<br>
      ${k.total} scenarios · ${k.pass} PASS · ${k.fail} FAIL · ${k.inconclusive} INCONCLUSIVE · ${k.running + k.pending} not judged</div>
    <table><thead><tr><th>Scenario</th><th>Verdict</th><th>Reason</th><th>Evidence chain</th></tr></thead><tbody>
      ${c.scenarios.map((s) => `<tr><td>${esc(s.id)}</td><td><span class="verdict ${esc(scenarioVerdict(s))}">${esc(scenarioVerdict(s))}</span></td>
        <td>${esc(s.report ? s.report.reason : s.state)}</td>
        <td>${s.report && s.report.chain ? (s.report.chain.complete ? 'complete'
          : 'missing: ' + esc(s.report.chain.links.filter((l) => l.state === 'missing').map((l) => l.link).join(', '))) : '—'}</td></tr>`).join('')}</tbody></table>
    ${c.scenarios.filter((s) => s.report).map((s) => `<div class="scenario-print">
      <h2>${esc(s.id)} — ${esc(s.report.verdict)}</h2>${scenarioReportHtml(s.report, true)}</div>`).join('')}`);
}

// Fills the print area and opens the browser's print dialog, where
// "Save as PDF" writes the PDF.
function printReport(html) {
  $('print-root').innerHTML = html;
  document.body.classList.add('print-mode');
  const done = () => {
    document.body.classList.remove('print-mode');
    window.removeEventListener('afterprint', done);
  };
  window.addEventListener('afterprint', done);
  setTimeout(() => window.print(), 50);
}

// --- start -----------------------------------------------------------------------

window.addEventListener('hashchange', () => setTab(location.hash.slice(1) || 'overview'));
renderTabs();
renderView();
tick();
setInterval(tick, POLL_MS);
