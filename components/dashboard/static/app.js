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

/* AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5); Claude Code / Claude Sonnet 5.5 (claude-sonnet-5-5) */

// Dashboard front end: plain JavaScript, no build step. It follows
// DASHBOARDS.md: a headline sentence, at most four key metrics with their
// reference, one chart, and the details on request.

'use strict';

// --- config: definitions and copy live here, the views only render ------------------

const POLL_MS = 2000;
const TABS = [['campaign', 'Campaign'], ['chain', 'Live chain'], ['diagnostics', 'Diagnostics']];
const VERDICT_RANK = { FAIL: 7, INCONCLUSIVE: 6, RUNNING: 5, PENDING: 4, INCOMPLETE: 4, PLANNED: 3, PASS: 1, NOTRUN: 0 };
const VERDICT_TEXT = { PASS: 'Pass', FAIL: 'Fail', INCONCLUSIVE: 'Inconclusive', PLANNED: 'Planned', RUNNING: 'Running',
  PENDING: 'Pending', INCOMPLETE: 'Not judged', NOTRUN: 'Not run' };
const SEVERITY_RANK = { Fatal: 5, Error: 4, Warn: 3, Info: 2, Debug: 1, Trace: 0, Unknown: -1 };
const SHORT_NAMES = {
  'kuksa-can-provider': 'CAN Provider', 'kuksa-databroker': 'Data Broker', 'vss-publisher': 'VSS Publisher',
  zenoh: 'Zenoh Router', guardian: 'Guardian', watchdog: 'Watchdog', 'opensovd-dfm': 'DFM', 'opensovd-gateway': 'SOVD Gateway',
};
const BARS_SHOWN = 8;

// Key metrics of the campaign view. `value` and `ref` get the summary (see summarise()).
const METRICS = [
  { id: 'passing', label: 'Tests passing',
    tone: (s) => (!s.tests ? '' : s.fail ? 'fail' : s.pass === s.tests ? 'pass' : ''),
    value: (s) => (s.tests ? `${s.pass}<small>of ${s.tests}</small>` : '—'),
    ref: (s) => `target: all${s.previous ? ` · previous campaign: ${s.previous.pass} of ${s.previous.scenarios}` : ''}` },
  { id: 'chains', label: 'Evidence chains complete',
    tone: (s) => (!s.judged ? '' : s.chainsComplete === s.judged ? 'pass' : 'warn'),
    value: (s) => (s.judged ? `${s.chainsComplete}<small>of ${s.judged}</small>` : '—'),
    ref: () => 'target: all, from hazard to verdict' },
  { id: 'tightest', label: 'Tightest reaction',
    tone: (s) => (!s.tightest ? '' : s.tightest.ratio > 1 ? 'fail' : 'pass'),
    value: (s) => (s.tightest ? `${Math.round(100 * s.tightest.ratio)}<small>% of budget</small>` : '—'),
    ref: (s) => (s.tightest ? `${s.tightest.ts}: ${formatMs(s.tightest.latency)} of ${formatMs(s.tightest.budget)}` : 'no timed check yet') },
  { id: 'open', label: 'Open requirements',
    tone: (s) => (s.planned ? 'warn' : 'pass'),
    value: (s) => `${s.planned}`,
    ref: (s) => (s.planned ? 'planned checks that do not pass yet' : 'no planned check is open') },
];

// Headline sentences. The first template whose `when` holds is used; the data fills it.
const HEADLINES = [
  { when: (s) => !s.campaign, text: () => 'No campaign yet. <em>Run all scenarios</em> to see how the Guardian does.' },
  { when: (s) => s.running, text: (s) => `Running <em>${esc(s.running.scenario || 'the next scenario')}</em>, ${s.running.done} of ${s.running.total} done.` },
  { when: (s) => s.fail > 0, text: (s) => `<em>${plural(s.fail, 'HARA test')} failed:</em> ${esc(s.failed.slice(0, 4).join(', '))}${s.failed.length > 4 ? ' and more' : ''}.` },
  { when: (s) => s.inconclusive > 0, text: (s) => `Nothing failed, but <em>${plural(s.inconclusive, 'test')} could not be judged:</em> ${esc(s.inconclusiveIds.join(', '))}.` },
  { when: (s) => s.judged > s.chainsComplete, text: (s) => `All ${s.tests} HARA tests passed, but <em>${plural(s.judged - s.chainsComplete, 'evidence chain')} ${s.judged - s.chainsComplete === 1 ? 'is' : 'are'} incomplete.</em>` },
  { when: (s) => s.planned > 0, text: (s) => `All ${s.tests} HARA tests passed. <em>${plural(s.planned, 'planned check')}</em> ${s.planned === 1 ? 'is' : 'are'} still open.` },
  { when: (s) => s.state !== 'done', text: (s) => `${s.pass} of ${s.tests} tests passed so far. The campaign stopped before the end.` },
  { when: () => true, text: (s) => `A normal run. All ${s.tests} HARA tests passed and <em>nothing needs your attention.</em>` },
];

// --- state -----------------------------------------------------------------------------

const state = {
  tab: 'campaign',
  overview: null,
  connected: false,
  updated: 0,
  busy: false,
  switching: false,
  catalog: null,
  titles: {},
  campaigns: [],
  campaignId: null,
  followLive: true,
  campaign: null,
  openRows: new Set(),
  openDetails: new Set(),
  showAllBars: false,
  showTests: false,
  showPicker: false,
  picked: new Set(),
  rebuild: false,
  changes: null,
  copied: false,
  headKey: '',
  bodyKey: '',
  selected: null,
  chainProject: null,
  sovd: null,
  sovdError: null,
  diagCampaign: null,
  showAllFaults: false,
  openFaults: new Set(),
  faultDetails: {},
  signals: new Map(),
  signalLoads: new Map(),
  hover: null,
};

const $ = (id) => document.getElementById(id);

// --- helpers ---------------------------------------------------------------------------

function esc(value) {
  return String(value ?? '').replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}

async function api(path, options = {}) {
  const response = await fetch(path, options);
  let body = null;
  try { body = await response.json(); } catch (_) { /* empty body */ }
  if (!response.ok) throw new Error((body && body.error) || `${response.status} ${response.statusText}`);
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
  setTimeout(() => el.remove(), bad ? 9000 : 4500);
}

function clock(ms) {
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour12: false }) + '.' + String(d.getMilliseconds()).padStart(3, '0');
}

// One place for number formats.
function formatMs(ms) {
  if (ms == null) return '—';
  return ms >= 10000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms).toLocaleString('en')} ms`;
}
function secs(ms) { return ms == null ? '—' : (ms / 1000).toFixed(2) + ' s'; }
function plural(n, word) { return `${n} ${word}${n === 1 ? '' : 's'}`; }

function ago(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 10) return 'just now';
  if (s < 90) return `${s} s ago`;
  return `${Math.round(s / 60)} min ago`;
}

function campaignLabel(c) {
  const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})/.exec(c.id);
  const when = m ? `${m[1]}-${m[2]}-${m[3]} ${m[4]}:${m[5]}` : c.id;
  return `${when} · ${c.scenarios ? `${c.pass} of ${c.scenarios} pass` : c.mode}${c.state === 'running' ? ' · running' : ''}`;
}

const VERDICT_MARK = { PASS: '✓', FAIL: '✕', INCONCLUSIVE: '▲', RUNNING: '●' };
function verdictHtml(v) { return `<span class="v ${esc(v)}">${VERDICT_MARK[v] ? VERDICT_MARK[v] + ' ' : ''}${esc(VERDICT_TEXT[v] || v)}</span>`; }

function components() { return (state.overview && state.overview.snapshot.components) || []; }

// --- routing: the view lives in the URL ----------------------------------------------------

function parseHash() {
  const [tab, query] = location.hash.slice(1).split('?');
  return { tab, params: new URLSearchParams(query || '') };
}

function writeHash() {
  if (state.tab !== 'campaign') return;
  const params = new URLSearchParams();
  if (state.campaignId && !state.followLive) params.set('c', state.campaignId);
  if (state.openRows.size) params.set('t', [...state.openRows].join(','));
  const hash = '#campaign' + (params.toString() ? '?' + params : '');
  if (location.hash !== hash) history.replaceState(null, '', hash);
}

function setTab(tab, params) {
  if (!TABS.some(([id]) => id === tab)) tab = 'campaign';
  state.tab = tab;
  if (params && tab === 'campaign') {
    if (params.get('c')) { state.campaignId = params.get('c'); state.followLive = false; }
    if (params.get('t')) { state.openRows = new Set(params.get('t').split(',')); state.showTests = true; }
  }
  $('tabs').innerHTML = TABS.map(([id, label]) =>
    `<a href="#${id}" class="${id === tab ? 'active' : ''}">${label}</a>`).join('');
  state.headKey = state.bodyKey = '';
  $('view').onclick = null;
  $('view').onchange = null;
  if (tab === 'campaign') renderCampaignSkeleton();
  else if (tab === 'chain') renderChainSkeleton();
  else renderDiagnosticsSkeleton();
  tick();
}

function setupTheme() {
  const button = $('theme');
  const paint = () => { button.textContent = document.documentElement.dataset.theme === 'dark' ? 'Light' : 'Dark'; };
  button.onclick = () => {
    const next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = next;
    try { localStorage.setItem('theme', next); } catch (_) { /* private mode */ }
    paint();
  };
  paint();
}

let tickTask = null;
function tick() {
  if (state.switching) return Promise.resolve();
  if (tickTask) return tickTask;
  tickTask = (async () => {
    await refreshOverview();
    if (state.tab === 'campaign') await refreshCampaign();
    else if (state.tab === 'chain') await refreshChain();
    else await refreshDiagnostics();
  })().finally(() => { tickTask = null; });
  return tickTask;
}

async function refreshOverview() {
  try {
    state.overview = await api('/api/overview');
    state.connected = true;
    state.updated = Date.now();
  } catch (_) {
    state.connected = false;
  }
  const select = $('backend');
  const camp = state.overview && state.overview.campaign;
  select.value = state.overview ? state.overview.backend : 'compose';
  select.querySelector('option[value="opendut"]').disabled = !(state.overview && state.overview.opendut_configured);
  select.disabled = !state.overview || state.switching || state.busy
    || Boolean(camp && camp.runner && (camp.runner.running || camp.runner.recovery));
}

// --- runtime: Docker Compose on this host, or AutoSD peers through OpenDUT ------------------

function isRemote() { return Boolean(state.overview && state.overview.backend === 'opendut'); }

$('backend').onchange = async (e) => {
  const backend = e.target.value;
  state.switching = true;
  e.target.disabled = true;
  try {
    // Finish polling the previous runtime before replacing its evidence state.
    await tickTask;
    await change('/api/backend', 'POST', { backend });
    state.campaignId = null; state.campaign = null; state.campaigns = []; state.followLive = true;
    state.catalog = null; state.openRows.clear(); state.picked.clear(); state.changes = null;
    state.signals.clear(); state.signalLoads.clear(); state.hover = null;
    state.selected = null; state.chainProject = null;
    state.sovd = null; state.faultDetails = {}; state.openFaults.clear();
    state.switching = false;
    setTab(state.tab);
  } catch (error) {
    toast(error.message, true);
    e.target.value = state.overview ? state.overview.backend : 'compose';
  } finally {
    state.switching = false;
    tick();
  }
};

// What the user sees when something is wrong with the dashboard itself.
function problem() {
  const snap = state.overview && state.overview.snapshot;
  if (!state.connected) return 'The dashboard cannot reach its own API. The last data is shown; it retries every two seconds.';
  if (snap && snap.error && isRemote()) {
    return snap.error_source === 'controller' ? `The OpenDUT controller is not available (${snap.error}).`
      : `AutoSD diagnostics: ${String(snap.error).includes('Connection refused') ? 'the endpoint is not listening yet' : snap.error}.`;
  }
  if (snap && snap.error) return `Docker is not available (${snap.error}). Campaign results still show; the live chain does not.`;
  return '';
}

// --- campaign: model ------------------------------------------------------------------------

function scenarioVerdict(cat, byId) {
  const cs = byId.get(cat.id);
  if (!cs) return 'NOTRUN';
  if (cs.report) {
    const v = cs.report.verdict;
    return cs.report.status === 'planned' && v !== 'PASS' ? 'PLANNED' : v;
  }
  return { running: 'RUNNING', pending: 'PENDING', incomplete: 'INCOMPLETE' }[cs.state] || 'PENDING';
}

function worst(verdicts) {
  return verdicts.reduce((a, b) => (VERDICT_RANK[b] > VERDICT_RANK[a] ? b : a), 'NOTRUN');
}

function mostBudgetUsed(reports) {
  let best = null;
  for (const r of reports) {
    for (const c of (r.checks || [])) {
      if (c.latency_ms == null || !c.budget_ms) continue;
      const ratio = c.latency_ms / c.budget_ms;
      if (!best || ratio > best.ratio) best = { ratio, latency: c.latency_ms, budget: c.budget_ms };
    }
  }
  return best;
}

function buildModel() {
  const campaign = state.campaign;
  const byId = new Map((campaign ? campaign.scenarios : []).map((s) => [s.id, s]));
  const tests = new Map();
  const extra = [];
  for (const s of (state.catalog || [])) {
    if (!s.hara_tests.length) extra.push(s);
    for (const ts of s.hara_tests) {
      if (!tests.has(ts)) tests.set(ts, []);
      tests.get(ts).push(s);
    }
  }
  const row = (ts, scenarios) => {
    const reports = scenarios.map((s) => (byId.get(s.id) || {}).report).filter(Boolean);
    return { ts, scenarios, verdict: worst(scenarios.map((s) => scenarioVerdict(s, byId))), reports, budget: mostBudgetUsed(reports) };
  };
  const rows = [...tests.entries()].sort((a, b) => a[0].localeCompare(b[0])).map(([ts, list]) => row(ts, list));
  const extraRows = extra.map((s) => row(s.id, [s]));
  return { campaign, byId, rows, extraRows };
}

function summarise(model) {
  const c = model.campaign;
  const running = state.overview && state.overview.campaign.running;
  const rows = model.rows;
  const of = (v) => rows.filter((r) => r.verdict === v);
  const judgedRows = rows.filter((r) => r.reports.length);
  const timed = rows.filter((r) => r.budget).map((r) => ({ ts: r.ts, verdict: r.verdict, ...r.budget })).sort((a, b) => b.ratio - a.ratio);
  const previous = state.campaigns.find((p) => p.id !== (c && c.id) && p.state === 'done' && p.scenarios >= (c ? c.scenarios.length : 0) / 2 && (!c || p.id < c.id));
  return {
    campaign: c, state: c && c.state, running: running && c && running.id === c.id ? running : null,
    tests: rows.length, pass: of('PASS').length, fail: of('FAIL').length, failed: of('FAIL').map((r) => r.ts),
    inconclusive: of('INCONCLUSIVE').length, inconclusiveIds: of('INCONCLUSIVE').map((r) => r.ts),
    planned: of('PLANNED').length, judged: judgedRows.length,
    chainsComplete: judgedRows.filter((r) => r.reports.every((x) => !x.chain || x.chain.complete)).length,
    tightest: timed[0] || null, timed, previous,
  };
}

// --- campaign: view -------------------------------------------------------------------------

function renderCampaignSkeleton() {
  $('view').innerHTML = `
    <div class="context" id="cp-context"></div>
    <div id="cp-picker"></div>
    <div id="cp-main"></div>`;
  $('view').onclick = onCampaignClick;
  $('view').onchange = onCampaignChange;
  $('view').addEventListener('toggle', onDetailsToggle, true);
  $('view').onmousemove = signalHover;
  $('view').onmouseleave = () => { state.hover = null; };
  loadCatalog();
}

async function loadCatalog() {
  try {
    const data = await api('/api/campaign/scenarios');
    state.catalog = data.scenarios;
    state.titles = data.hara_titles || {};
    state.catalogError = null;
  } catch (error) {
    state.catalogError = error.message;
  }
  state.bodyKey = '';
}

async function refreshCampaign() {
  try {
    const list = await api('/api/campaigns');
    state.campaigns = list.campaigns;
    state.runsDir = list.runs_dir;
  } catch (_) { /* keep the old list */ }
  const running = state.overview && state.overview.campaign.running;
  if (running && state.followLive) state.campaignId = running.id;
  if (!state.campaignId && state.campaigns.length) state.campaignId = state.campaigns[0].id;
  state.campaign = null;
  if (state.campaignId) {
    try { state.campaign = await api('/api/campaigns/' + encodeURIComponent(state.campaignId)); } catch (_) { /* unknown id */ }
  }
  if (!state.catalog) await loadCatalog();
  renderContext();
  renderCampaignMain();
}

function renderContext() {
  const camp = state.overview && state.overview.campaign;
  const box = $('cp-context');
  if (!camp || !box) return;
  const runner = camp.runner || {};
  const leftovers = (camp.projects || []).length > 0;
  const stale = state.connected && Date.now() - state.updated > 10000;
  const key = JSON.stringify([state.campaigns.map((c) => c.id + c.state + c.pass), state.campaignId, runner.available, runner.running,
    state.busy, state.showPicker, leftovers, state.copied, state.connected, Math.floor(Date.now() / 15000), stale]);
  if (key === state.headKey) return;
  state.headKey = key;
  const options = state.campaigns.map((c) =>
    `<option value="${esc(c.id)}" ${c.id === state.campaignId ? 'selected' : ''}>${esc(campaignLabel(c))}</option>`).join('');
  box.innerHTML = `
    <select id="cp-select" aria-label="Campaign" ${state.campaigns.length ? '' : 'disabled'}>${options || '<option>no campaign yet</option>'}</select>
    <span>${state.connected ? `Updated ${ago(state.updated)}` : '<span class="attn">▲ Not updating</span>'}</span>
    <span class="grow"></span>
    <button class="link" data-act="copy">${state.copied ? 'Copied' : 'Copy link to this view'}</button>
    ${runner.running || leftovers ? '<button class="link attn" data-act="stop">Stop the campaign</button>' : ''}
    <button class="link" data-act="picker" ${runner.available ? '' : 'disabled'}>${state.showPicker ? 'Close selection' : 'Select scenarios'}</button>
    <button class="btn primary" data-act="run-all" ${runner.available && !runner.running && !state.busy ? '' : 'disabled'}
      ${runner.available ? '' : `title="${esc(runner.unavailable || '')}"`}>Run all ↗</button>`;
}

function renderPicker() {
  const box = $('cp-picker');
  if (!box) return;
  if (!state.showPicker || !state.catalog) { box.innerHTML = ''; return; }
  box.innerHTML = `<div class="picker">
    <div class="actions"><span>${state.picked.size} of ${state.catalog.length} selected</span>
      <button class="link" data-act="pick-all">all</button><button class="link" data-act="pick-none">none</button></div>
    <div class="list">${state.catalog.map((s) => `
      <label title="${esc(s.description)}"><input type="checkbox" data-scenario="${esc(s.id)}" ${state.picked.has(s.id) ? 'checked' : ''}>
        <span class="mono">${esc(s.id)}</span> <span class="faint small">${esc(s.hara_tests.join(', '))}${s.status === 'planned' ? ' planned' : ''}</span></label>`).join('')}</div>
    <div class="actions"><button class="btn primary" data-act="run-selected" ${state.picked.size && !state.busy ? '' : 'disabled'}>Run ${state.picked.size} selected ↗</button>
      ${isRemote() ? '<span class="small muted">Fresh AutoSD peers are prepared through OpenDUT; the images are built from this checkout.</span>'
        : `<label class="small muted"><input type="checkbox" id="cp-rebuild" ${state.rebuild ? 'checked' : ''}> rebuild the Guardian and VSS Publisher images first</label>`}</div></div>`;
}

function headlineHtml(s) {
  const template = HEADLINES.find((h) => h.when(s));
  const tone = !s.campaign || s.running ? '' : s.fail ? 'fail' : s.inconclusive || s.judged > s.chainsComplete || s.planned ? 'warn' : 'pass';
  return `<h1 class="headline fade tone-${tone}">${template.text(s)}</h1>`;
}

// "Since you last looked": the verdicts of the last campaign this browser saw.
function sinceLine(model, s) {
  const c = model.campaign;
  if (!c || c.state !== 'done') return '';
  let seen = null;
  try { seen = JSON.parse(localStorage.getItem('lastSeen') || 'null'); } catch (_) { /* none */ }
  const now = Object.fromEntries(model.rows.map((r) => [r.ts, r.verdict]));
  if (!state.changes || state.changes.id !== c.id) {
    const diff = seen && seen.id !== c.id ? Object.keys(now).filter((ts) => seen.verdicts[ts] && seen.verdicts[ts] !== now[ts]) : [];
    state.changes = { id: c.id, diff, since: seen && seen.when };
    try { localStorage.setItem('lastSeen', JSON.stringify({ id: c.id, verdicts: now, when: new Date().toISOString() })); } catch (_) { /* private mode */ }
  }
  void s;
  const { diff, since } = state.changes;
  if (!diff.length) return '';
  const day = since ? new Date(since).toLocaleDateString('en', { weekday: 'long' }) : 'your last visit';
  return `<p class="since">${plural(diff.length, 'thing')} changed since ${esc(day)}: ${esc(diff.slice(0, 4).join(', '))}${diff.length > 4 ? ' …' : ''}.</p>`;
}

function metricsHtml(s) {
  if (!s.campaign) return '';
  return `<div class="metrics">${METRICS.map((m) => `
    <div class="metric"><div class="label">${esc(m.label)}</div><div class="value ${m.tone(s)}">${m.value(s)}</div><div class="ref">${esc(m.ref(s))}</div></div>`).join('')}</div>`;
}

// The one chart: reaction time against its budget. The band is the budget.
function barsHtml(s) {
  if (!s.timed.length) return '';
  const shown = state.showAllBars ? s.timed : s.timed.slice(0, BARS_SHOWN);
  const scale = Math.max(1.25, ...s.timed.map((t) => t.ratio * 1.05));
  const rows = shown.map((t) => {
    const over = t.ratio > 1;
    const title = state.titles[t.ts] || '';
    return `<div class="bar-row" title="${esc(`${t.ts} ${title}: ${t.latency} ms against a budget of ${t.budget} ms`)}">
      <div class="name"><b>${esc(t.ts)}</b><span class="muted">${esc(title)}</span></div>
      <div class="track" role="img" aria-label="${esc(`${t.ts} used ${Math.round(100 * t.ratio)} percent of its budget`)}">
        <div class="band" style="width:${(100 / scale).toFixed(2)}%"></div>
        <div class="fill ${over ? 'over' : esc(t.verdict)}" style="width:${(100 * Math.min(t.ratio, scale) / scale).toFixed(2)}%"></div></div>
      <div class="value">${over ? '<span class="t-fail">▲ </span>' : ''}${formatMs(t.latency)} of ${formatMs(t.budget)}</div></div>`;
  }).join('');
  return `<section>
    <div class="rowhead"><div><h2 class="title">Reaction time against budget</h2>
      <div class="sub">Each bar is how long the Guardian took, inside the budget the safety concept allows.</div></div></div>
    <div class="bars">${rows}</div>
    ${s.timed.length > BARS_SHOWN ? `<button class="link small" data-act="all-bars" style="margin-top:8px">${state.showAllBars ? 'Show fewer' : `Show all ${s.timed.length}`}</button>` : ''}</section>`;
}

function attentionHtml(model) {
  const rows = model.rows.filter((r) => ['FAIL', 'INCONCLUSIVE'].includes(r.verdict));
  if (!rows.length) return '';
  return `<section><div class="rowhead"><h2 class="title">Needs your attention</h2></div>${testsTable(rows, 'HARA test', true)}</section>`;
}

function renderCampaignMain() {
  const box = $('cp-main');
  if (!box) return;
  if (!state.catalog) {
    box.innerHTML = `<div class="state">${esc(state.catalogError ? 'The scenario catalog could not be read: ' + state.catalogError : 'Loading the scenarios…')}</div>`;
    return;
  }
  const model = buildModel();
  const c = model.campaign;
  const running = state.overview && state.overview.campaign.running;
  const key = JSON.stringify([state.campaignId, c && c.state, c && c.scenarios.map((s) => [s.id, s.state, s.observations, s.report && s.report.verdict]),
    [...state.openRows], [...state.openDetails], state.showPicker, [...state.picked], state.busy, state.showAllBars, state.showTests,
    running && [running.scenario, running.done], problem()]);
  if (key === state.bodyKey) return;
  state.bodyKey = key;
  renderPicker();
  writeHash();
  const s = summarise(model);
  const warn = problem();
  const extra = model.extraRows.length ? `<h3 class="title" style="margin:34px 0 10px;font-size:17px">Further checks <span class="faint small" style="font-family:var(--sans)">not HARA tests</span></h3>${testsTable(model.extraRows, 'Scenario', false)}` : '';
  box.innerHTML = `
    ${warn ? `<p class="since attn">▲ ${esc(warn)}</p>` : ''}
    ${headlineHtml(s)}
    ${sinceLine(model, s)}
    ${metricsHtml(s)}
    ${attentionHtml(model)}
    ${c ? barsHtml(s) : ''}
    ${c ? `<section>
      <details data-key="tests" ${state.showTests ? 'open' : ''}><summary>All ${model.rows.length} HARA tests</summary>
        ${testsTable(model.rows, 'HARA test', true)}${extra}
        <p style="margin-top:18px"><button class="link small" data-act="print">Print this report</button></p></details></section>` : ''}
    <div id="cp-runner"></div>`;
  renderRunner();
}

function testsTable(rows, first, withTitle) {
  const body = rows.map((r) => {
    const open = state.openRows.has(r.ts);
    const title = withTitle ? (state.titles[r.ts] || '') : (r.scenarios[0].description || '');
    return `<tr class="row" data-row="${esc(r.ts)}" aria-expanded="${open}" tabindex="0">
      <td class="id">${esc(r.ts)}</td><td>${esc(title)}${withTitle ? `<div class="faint small mono">${esc(r.scenarios.map((x) => x.id).join(' · '))}</div>` : ''}</td>
      <td>${verdictHtml(r.verdict)}</td>
      <td class="num muted">${r.budget ? `${formatMs(r.budget.latency)} of ${formatMs(r.budget.budget)}` : ''}</td></tr>
      ${open ? `<tr><td class="detail" colspan="4">${rowDetailHtml(r)}</td></tr>` : ''}`;
  }).join('');
  return `<div class="table-wrap"><table><thead><tr><th>${esc(first)}</th><th>${withTitle ? 'Title and scenarios' : 'Description'}</th><th>Verdict</th><th class="num">Reaction</th></tr></thead>
    <tbody>${body}</tbody></table></div>`;
}

function rowDetailHtml(r) {
  const byId = new Map((state.campaign ? state.campaign.scenarios : []).map((s) => [s.id, s]));
  const again = state.overview && state.overview.campaign.runner && state.overview.campaign.runner.available && !(state.overview.campaign.runner.running);
  return r.scenarios.map((cat) => {
    const cs = byId.get(cat.id);
    const verdict = scenarioVerdict(cat, byId);
    let body;
    if (cs && cs.report) body = scenarioReportHtml(cs.report, false);
    else if (cs && cs.state === 'running') {
      body = `<p class="muted">Running now, ${esc(cs.observations)} observations so far.</p>
        ${cs.manifest ? signalHtml(cs.manifest.run_id, null, true) : ''}`;
    }
    else if (cs) {
      body = `<p class="muted">${cs.state === 'pending' ? 'Not started yet.' : 'Not judged: the campaign tool stopped during this scenario.'}</p>`;
      if (cs.manifest && cs.observations) {
        body += `<p class="muted small">${esc(cs.observations)} retained observations (samples, Guardian events, OpenSOVD changes).</p>
          ${signalHtml(cs.manifest.run_id, null, false)}`;
      }
    }
    else body = '<p class="muted">This scenario is not part of the campaign.</p>';
    if (cs && cs.error) body += `<pre>${esc(cs.error)}</pre>`;
    return `<div class="scn"><div class="scn-head"><span class="mono">${esc(cat.id)}</span>${verdictHtml(verdict)}
      <span class="muted">${esc(cat.description)}</span><span style="flex:1"></span>
      ${again ? `<button class="link small" data-act="rerun" data-id="${esc(cat.id)}">Run again ↗</button>` : ''}</div>${body}</div>`;
  }).join('');
}

function sampleText(s) { return s ? `#${s.sequence} (counter ${s.alive_counter})` : '—'; }

function expectationText(e) {
  if (!e) return '';
  const parts = [e.kind];
  for (const key of ['dtc', 'state', 'mitigation', 'quality']) if (e[key]) parts.push(e[key]);
  if (e.after) parts.push('after ' + (typeof e.after === 'string' ? e.after : JSON.stringify(e.after)));
  return parts.join(' ');
}

function dtcFlags(status) {
  if (!status) return 'never shown';
  const flags = [['testFailed', 'failed'], ['confirmedDtc', 'confirmed'], ['pendingDtc', 'pending'],
    ['testFailedSinceLastClear', 'failed since clear'], ['warningIndicatorRequested', 'warning lamp']];
  const set = flags.filter(([key]) => status[key] === true).map(([, label]) => label);
  return set.length ? set.join(', ') : 'no flag set';
}

function detailsBlock(key, summary, content, forPrint) {
  return `<details data-key="${esc(key)}" ${forPrint || state.openDetails.has(key) ? 'open' : ''}><summary>${esc(summary)}</summary>${content}</details>`;
}

const LINK_STATE = { present: 'present', missing: 'missing', not_expected: 'not expected', unexpected: 'unexpected' };

function chainHtml(r, forPrint) {
  const chain = r.chain;
  if (!chain) return '<p class="muted small">No evidence chain: this report is from an older campaign tool.</p>';
  const id = r.manifest.run_id;
  const steps = chain.links.map((l) => `<span class="${l.state === 'missing' ? 'missing' : l.state === 'present' ? 'present' : ''}" title="${esc(l.evidence)}">${esc(l.link)}${l.state === 'missing' ? ' (missing)' : ''}</span>`).join('');
  const table = (head, rows) => `<table><thead><tr>${head.map((h) => `<th>${esc(h)}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table>`;
  return `
    <div class="chain" aria-label="Evidence chain">${steps}</div>
    ${detailsBlock(id + ':chain', 'Evidence, linked by session and event IDs',
    table(['Link', 'Evidence', ''], chain.links.map((l) => `<tr><td>${esc(l.link)}</td><td>${esc(l.evidence)}</td><td class="muted">${esc(LINK_STATE[l.state] || l.state)}</td></tr>`).join('')), forPrint)}
    ${chain.detections.length ? detailsBlock(id + ':det', `Detection (${chain.detections.length})`, table(['Event', 'Detection', 'After t0', 'Sample', 'Recovered'],
    chain.detections.map((d) => `<tr><td class="id">#${esc(d.event_id)}</td><td>${esc(d.event)}</td><td>${esc(secs(d.latency_ms))}</td><td class="id">${esc(sampleText(d.sample))}</td>
      <td>${d.recovered_event_id ? `#${esc(d.recovered_event_id)} at ${esc(secs(d.recovered_ms))}` : '—'}</td></tr>`).join('')), forPrint) : ''}
    ${chain.mitigations.length ? detailsBlock(id + ':mit', `Mitigation (${chain.mitigations.length})`, table(['Event', 'Mitigation', 'After t0', 'Cause chain'],
    chain.mitigations.map((m) => `<tr><td class="id">#${esc(m.event_id)}</td><td>${esc(m.mitigation)}</td><td>${esc(secs(m.latency_ms))}</td>
      <td>${m.cause_chain.map(esc).join(' → ')}${m.detection_event_id ? '' : ' (not caused by a detection)'}</td></tr>`).join('')), forPrint) : ''}
    ${chain.diagnostics.length ? detailsBlock(id + ':dtc', `DTCs in OpenSOVD (${chain.diagnostics.length})`, table(['DTC', 'Failed in OpenSOVD', 'Severity', 'Status', 'Occ.', 'Passed later'],
    chain.diagnostics.map((d) => `<tr><td class="id">${esc(d.dtc)}</td><td>${d.latency_ms == null ? 'never' : esc(secs(d.latency_ms)) + ' after the event'}</td>
      <td>${esc(d.severity || '—')}</td><td>${esc(dtcFlags(d.status))}</td><td>${esc(d.occurrence_counter ?? '—')}</td><td>${d.passed_later ? 'yes' : 'no'}</td></tr>`).join('')), forPrint) : ''}`;
}

function scenarioReportHtml(r, forPrint) {
  const checks = r.checks || [];
  const violations = r.violations || [];
  const manifest = r.manifest || {};
  const timeline = r.timeline && r.timeline.length ? detailsBlock(manifest.run_id + ':timeline', `Guardian events (${r.timeline.length})`, `
    <table><thead><tr><th>#</th><th>Cause</th><th>At the tap</th><th>Event</th><th>Sample</th></tr></thead><tbody>
    ${r.timeline.map((e) => `<tr><td class="id">#${esc(e.event_id)}</td><td class="id">${e.cause_event_id ? '#' + esc(e.cause_event_id) : '—'}</td>
      <td>${esc(secs(e.delivered_ms))}</td><td>${esc(e.event)}</td><td class="id">${esc(sampleText(e.sample))}</td></tr>`).join('')}</tbody></table>`, forPrint) : '';
  return `
    <p class="reason">${esc(r.reason)}</p>
    <p class="facts">Injected: <b>${esc(r.description)}</b>. Onset <b>${r.onset ? esc(secs(r.onset.t_ms) + ', ' + r.onset.description) : 'not observed'}</b>.
      ${r.note ? esc(r.note) : ''}</p>
    ${manifest.run_id ? signalHtml(manifest.run_id, r, false) : ''}
    ${chainHtml(r, forPrint)}
    ${checks.length ? detailsBlock(manifest.run_id + ':checks', `Checks (${checks.length})`, `<table><thead><tr><th>Requirement</th><th>Expectation</th><th>Observed</th><th class="num">Latency</th><th class="num">Budget</th><th>Result</th></tr></thead><tbody>
      ${checks.map((c) => `<tr><td>${esc((c.expectation && c.expectation.requirement) || '—')}</td><td class="id">${esc(expectationText(c.expectation))}</td>
        <td>${esc(c.outcome && c.outcome.detail)}</td><td class="num">${esc(secs(c.latency_ms))}</td><td class="num">${esc(secs(c.budget_ms))}</td>
        <td class="${c.outcome && c.outcome.result === 'Failed' ? 't-fail' : c.outcome && c.outcome.result === 'Met' ? 't-pass' : 't-warn'}">${esc(c.outcome && c.outcome.result)}</td></tr>`).join('')}</tbody></table>`, forPrint || (r.verdict !== 'PASS')) : ''}
    ${violations.length ? `<h4 class="attn">Forbidden reactions</h4><table><tbody>${violations.map((v) => `<tr><td>${esc(v.rule)}</td><td>${esc(v.requirement)}</td><td>${esc(v.detail)}</td></tr>`).join('')}</tbody></table>` : ''}
    ${timeline}
    ${detailsBlock(manifest.run_id + ':run', 'Run details', `<p class="facts">Hazard <b>${esc(r.hazard || '—')}</b>, safety goal <b>${esc(r.safety_goal || '—')}</b>.
      Guardian session <span class="mono">${esc(r.session_id || '—')}</span>.<br>Run <span class="mono">${esc(manifest.run_id)}</span>, started ${esc(manifest.started_at)}, git ${esc(manifest.git_revision || '—')}.</p>`, forPrint)}`;
}

// --- campaign: signal plot ------------------------------------------------------------
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
  if (stale && !state.signalLoads.has(runId)) loadSignal(runId).then(redrawSignals);
  let inner;
  if (!entry) inner = '<div class="muted small">loading signal…</div>';
  else if (entry.error) inner = `<div class="muted small">no signal: ${esc(entry.error)}</div>`;
  else inner = signalSvg(runId, entry.data, report) + signalLegend();
  return `<h4>Signal</h4><div class="signal" data-run="${esc(runId)}">${inner}
    <div class="signal-readout small mono">${esc(signalReadout(runId))}</div></div>`;
}

// The campaign view redraws only when its key changes; a loaded plot must show.
function redrawSignals() {
  state.bodyKey = '';
  renderCampaignMain();
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
        height="${SIG.PH}"><title>${esc(secs(p.t_ms))}: quality ${esc(p.quality)}</title></rect>`);
    }
    const next = samples[i + 1];
    if (next && next.t_ms - p.t_ms > gap) {
      out.push(`<rect class="sig-gap" x="${x(p.t_ms)}" y="${top}" width="${x(next.t_ms) - x(p.t_ms)}" height="${SIG.PH}">
        <title>no samples for ${esc(secs(next.t_ms - p.t_ms))}</title></rect>`);
    }
  });

  // After the evaluation window: recorded, but not judged (teardown).
  if (report && report.window_end_ms != null && report.window_end_ms < end) {
    const w = x(report.window_end_ms);
    out.push(`<rect class="sig-after" x="${w}" y="${top}" width="${right - w}" height="${height - 4 - top}">
      <title>after the evaluation window (${esc(secs(report.window_end_ms))}): recorded, not judged</title></rect>`);
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
      <title>${esc(secs(inj.t_ms))}: ${esc(inj.action)} — ${esc(inj.detail)}</title></line>`);
    if (!setup) {
      out.push(`<text class="sig-label" x="${x(inj.t_ms) + 3}" y="${top + 10}">${esc(inj.action)} ${esc(inj.detail)}</text>`);
    }
  }
  if (report && report.onset) {
    const t0 = x(report.onset.t_ms);
    out.push(`<line class="sig-onset" x1="${t0}" x2="${t0}" y1="${top - 6}" y2="${laneEnd}">
      <title>onset t0 ${esc(secs(report.onset.t_ms))}: ${esc(report.onset.description)}</title></line>
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
      <title>${esc(secs(c.t_ms))}: ${esc(c.value)}</title></rect>`);
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
      d="${shape[m.kind]}"><title>${esc(secs(m.t_ms))}: ${esc(m.text)}</title></path>`);
  }

  const failedSince = {};
  const dtcBar = (code, from, to) => {
    out.push(`<rect class="sig-dtc" x="${x(from)}" y="${laneY(3) + 2}" width="${Math.max(2, x(to) - x(from))}" height="${SIG.LANE - 4}">
      <title>${esc(code)} failed in OpenSOVD ${esc(secs(from))} – ${esc(secs(to))}</title></rect>`);
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
    ? `sample at ${secs(best.t_ms)}: max ${best.max_c.toFixed(1)} · avg ${best.avg_c.toFixed(1)} · min ${best.min_c.toFixed(1)} °C · ${best.quality}`
    : 'no sample nearby';
  return `${secs(t)} — ${sample} — thermal ${at(signal.thermal)} · monitoring ${at(signal.monitoring)}`;
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

// --- campaign: actions ---------------------------------------------------------------------

function onDetailsToggle(e) {
  const el = e.target;
  if (!el.matches || !el.matches('details[data-key]')) return;
  const key = el.dataset.key;
  if (key === 'tests') state.showTests = el.open;
  if (el.open) state.openDetails.add(key); else state.openDetails.delete(key);
}

function onCampaignChange(e) {
  if (e.target.id === 'cp-select') {
    state.campaignId = e.target.value;
    state.followLive = false;
    state.openRows.clear();
    state.bodyKey = state.headKey = '';
    refreshCampaign();
  } else if (e.target.id === 'cp-rebuild') {
    state.rebuild = e.target.checked;
  } else if (e.target.matches('input[data-scenario]')) {
    if (e.target.checked) state.picked.add(e.target.dataset.scenario); else state.picked.delete(e.target.dataset.scenario);
    state.bodyKey = '';
    renderCampaignMain();
  }
}

function refreshAll() { state.headKey = state.bodyKey = ''; renderContext(); renderCampaignMain(); }

async function copyLink() {
  writeHash();
  try { await navigator.clipboard.writeText(location.href); } catch (_) {
    const area = document.createElement('textarea');
    area.value = location.href;
    document.body.appendChild(area);
    area.select();
    document.execCommand('copy');
    area.remove();
  }
  state.copied = true;
  state.headKey = '';
  renderContext();
  setTimeout(() => { state.copied = false; state.headKey = ''; renderContext(); }, 2000);
}

function onCampaignClick(e) {
  const act = e.target.closest('[data-act]');
  if (act) {
    const a = act.dataset.act;
    if (a === 'run-all') startCampaign([]);
    else if (a === 'run-selected') startCampaign([...state.picked]);
    else if (a === 'rerun') startCampaign([act.dataset.id]);
    else if (a === 'stop') stopCampaign();
    else if (a === 'print') printCampaign();
    else if (a === 'copy') copyLink();
    else if (a === 'all-bars') { state.showAllBars = !state.showAllBars; state.bodyKey = ''; renderCampaignMain(); }
    else if (a === 'picker') { state.showPicker = !state.showPicker; refreshAll(); }
    else if (a === 'pick-all') { state.picked = new Set((state.catalog || []).map((s) => s.id)); refreshAll(); }
    else if (a === 'pick-none') { state.picked.clear(); refreshAll(); }
    return;
  }
  if (e.target.closest('details, summary, .detail')) return;
  const row = e.target.closest('tr[data-row]');
  if (row) toggleRow(row.dataset.row);
}

function toggleRow(id) {
  if (state.openRows.has(id)) state.openRows.delete(id); else state.openRows.add(id);
  state.bodyKey = '';
  renderCampaignMain();
}

document.addEventListener('keydown', (e) => {
  if ((e.key === 'Enter' || e.key === ' ') && e.target.matches && e.target.matches('tr[data-row]')) {
    e.preventDefault();
    toggleRow(e.target.dataset.row);
  }
});

const EXIT_MEANING = {
  0: 'every implemented scenario passed',
  1: 'at least one implemented scenario did not pass',
  2: 'the campaign tool itself failed',
};

function renderRunner() {
  const box = $('cp-runner');
  const runner = state.overview && state.overview.campaign.runner;
  if (!box || !runner) return;
  const log = runner.log || [];
  if (!log.length) { box.innerHTML = ''; return; }
  const meaning = runner.phase === 'cancelled' ? 'cancelled; evidence retained'
    : isRemote() && runner.exit_code !== 0 ? 'campaign or bench setup/cleanup failed; see the log and the retained evidence'
      : EXIT_MEANING[runner.exit_code] || 'stopped';
  const summary = runner.running ? `Runner output, running${runner.phase ? ' · ' + runner.phase : ''}`
    : `Runner output, exit code ${runner.exit_code}: ${meaning}`;
  const notes = [
    runner.sync_error ? `Live evidence retrieval: ${runner.sync_error.error}` : '',
    runner.cleanup && runner.cleanup.failures && runner.cleanup.failures.length ? `Cleanup needs attention: ${runner.cleanup.failures.join('; ')}` : '',
  ].filter(Boolean).map((n) => `<p class="attn small">▲ ${esc(n)}</p>`).join('');
  box.innerHTML = `<section>${notes}<details data-key="runner" ${state.openDetails.has('runner') ? 'open' : ''}><summary>${esc(summary)}</summary>
    <pre style="max-height:260px">${log.map((l) => `${clock(l.ts_ms)}  ${esc(l.text)}`).join('\n')}</pre></details></section>`;
}

async function startCampaign(scenarios) {
  if (state.busy) return;
  state.busy = true;
  try {
    const result = await change('/api/campaign/start', 'POST', { scenarios, build: state.rebuild });
    toast('Started: ' + result.done.join('\n'));
    state.followLive = true;
    state.showPicker = false;
    state.openRows.clear();
  } catch (error) {
    toast('Cannot start the campaign: ' + error.message, true);
  } finally {
    state.busy = false;
    refreshAll();
    tick();
  }
}

async function stopCampaign() {
  if (!confirm(isRemote() ? 'Cancel the OpenDUT campaign? Restoration and evidence collection finish before the owned peers are removed.'
    : 'Stop the running campaign? The scenario in progress stays unjudged; its Compose project is removed.')) return;
  if (state.busy) return;
  state.busy = true;
  try {
    const result = await change('/api/campaign/stop');
    toast(result.done.length ? result.done.join('\n') : 'nothing to stop');
  } catch (error) {
    toast('Cannot stop the campaign: ' + error.message, true);
  } finally {
    state.busy = false;
    refreshAll();
    tick();
  }
}

async function printCampaign() {
  const model = buildModel();
  const c = model.campaign;
  if (!c) { toast('No campaign loaded', true); return; }
  await Promise.all(c.scenarios.filter((x) => x.report && x.report.manifest)
    .map((x) => loadSignal(x.report.manifest.run_id)));
  const s = summarise(model);
  const table = (rows, label, titled) => `<table><thead><tr><th>${label}</th><th>${titled ? 'Title' : 'Description'}</th><th>Scenarios</th><th>Verdict</th><th>Reaction</th></tr></thead><tbody>
    ${rows.map((r) => `<tr><td>${esc(r.ts)}</td><td>${esc(titled ? state.titles[r.ts] || '' : r.scenarios[0].description)}</td>
      <td>${titled ? esc(r.scenarios.map((x) => x.id).join(', ')) : ''}</td><td>${esc(VERDICT_TEXT[r.verdict])}</td>
      <td>${r.budget ? `${formatMs(r.budget.latency)} of ${formatMs(r.budget.budget)}` : ''}</td></tr>`).join('')}</tbody></table>`;
  const plain = HEADLINES.find((h) => h.when(s)).text(s).replace(/<[^>]+>/g, '');
  printReport(`
    <h1>Fault campaign report</h1>
    <div class="meta">${esc(plain)}<br>Campaign <b>${esc(c.id)}</b> (${esc(c.mode)}), ${esc(c.state)}, started ${esc(c.started_at || '—')}, generated ${esc(new Date().toLocaleString())}</div>
    ${table(model.rows, 'HARA test', true)}
    ${model.extraRows.length ? '<h2>Further checks</h2>' + table(model.extraRows, 'Scenario', false) : ''}
    ${c.scenarios.filter((x) => x.report).map((x) => `<div class="scn-print"><h2>${esc(x.id)}: ${esc(VERDICT_TEXT[x.report.verdict] || x.report.verdict)}</h2>${scenarioReportHtml(x.report, true)}</div>`).join('')}`);
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

// --- live chain -----------------------------------------------------------------------------

function renderChainSkeleton() {
  $('view').innerHTML = `
    <div class="context"><span id="ch-updated"></span><span class="grow"></span>
      <span id="ch-all"></span></div>
    <h1 class="headline" id="ch-headline"></h1>
    <div class="strip" id="ch-strip"></div>
    <div id="ch-drawer"></div>
    <section>
      <div class="rowhead"><h2 class="title">Chain timeline</h2><select id="ch-project" aria-label="Chain"></select>
        <span class="grow"></span><span class="sub" id="ch-span"></span></div>
      <div id="ch-timeline"></div>
    </section>
    <section>
      <div class="rowhead"><h2 class="title">Events</h2><span class="sub">newest first</span></div>
      <div class="table-wrap events" id="ch-events"></div>
    </section>
    <section id="ch-streams"><div class="streams">
      <div class="stream"><h3>Sample at the Guardian input</h3><div class="box" id="ch-in"></div></div>
      <div class="stream"><h3>Guardian events</h3><div class="box" id="ch-out"></div></div>
      <div class="stream"><h3>Open DTCs in OpenSOVD</h3><div class="box" id="ch-dtc"></div></div>
    </div></section>`;
  $('ch-all').innerHTML = isRemote() ? '<span class="small muted">managed by Ankaios</span>'
    : '<button class="link" data-all="start">Start all</button><button class="link attn" data-all="stop">Stop all</button>';
  $('view').onclick = onChainClick;
}

function onChainClick(e) {
  const all = e.target.closest('button[data-all]');
  if (all) {
    if (all.dataset.all === 'stop' && !confirm('Stop every component of the stack?')) return;
    allAction(all.dataset.all);
    return;
  }
  const cell = e.target.closest('button.cell');
  if (cell) {
    state.selected = state.selected === cell.dataset.svc ? null : cell.dataset.svc;
    fillStrip();
    refreshDrawer();
    return;
  }
  const power = e.target.closest('button[data-act]');
  if (power) componentAction(power.dataset.svc, power.dataset.act);
}

async function componentAction(service, action) {
  if (state.busy) return;
  if (action === 'stop' && service === 'opensovd-dfm' &&
      !confirm('Stopping DFM also cuts off the services that share its IPC namespace (Guardian, watchdog, gateway). Continue?')) return;
  state.busy = true;
  try {
    const result = await change(`/api/components/${encodeURIComponent(service)}/${action}`);
    toast(result.done.join('\n'));
  } catch (error) {
    toast(`${action} ${service} failed: ${error.message}`, true);
  } finally {
    state.busy = false;
    tick();
  }
}

async function allAction(action) {
  if (state.busy) return;
  state.busy = true;
  toast(`${action} all components …`);
  try {
    const result = await change(`/api/all/${action}`);
    const failed = result.done.filter((line) => !line.endsWith(': ok'));
    toast(result.done.join('\n'), failed.length > 0);
  } catch (error) {
    toast(`${action} all failed: ${error.message}`, true);
  } finally {
    state.busy = false;
    tick();
  }
}

const CHAIN_ORDER = ['zenoh', 'kuksa-databroker', 'opensovd-dfm', 'opensovd-gateway', 'vss-publisher', 'kuksa-can-provider', 'guardian', 'watchdog'];

// The chain in view: the containers of the running campaign scenario, or the stack.
function chainInView() {
  const camp = state.overview && state.overview.campaign;
  if (camp && camp.projects && camp.projects.length) {
    const project = camp.projects[0];
    const list = (camp.containers || []).filter((c) => c.project === project)
      .sort((a, b) => CHAIN_ORDER.indexOf(a.service) - CHAIN_ORDER.indexOf(b.service));
    return { campaign: true, project, scenario: chainName(project), list };
  }
  if (camp && camp.running) return { campaign: true, between: true, list: [] };
  return { campaign: false, list: components() };
}

function chainHeadline(view) {
  const list = view.list;
  if (view.between) return 'The campaign is <em>between two scenarios.</em> The next chain is being built.';
  if (view.campaign) {
    const up = list.filter((c) => c.state === 'running');
    const down = list.filter((c) => c.state !== 'running');
    if (down.length && up.length) return `Scenario <em>${esc(view.scenario)}</em> is running. <em>${esc(SHORT_NAMES[down[0].service] || down[0].service)} is ${esc(down[0].state)}.</em>`;
    return `Scenario <em>${esc(view.scenario)}</em> is running. ${up.length} of ${list.length} containers are up.`;
  }
  if (!list.length) return 'The stack is <em>not available.</em>';
  const running = list.filter((c) => c.state === 'running');
  const missing = list.filter((c) => c.state === 'missing');
  const down = list.filter((c) => !['running', 'missing'].includes(c.state));
  if (running.length === list.length) return `The chain is <em>running.</em> All ${list.length} components are up.`;
  if (missing.length === list.length) return 'The chain is <em>not started.</em> Run a campaign to watch it build and tear down its own chains, or start all components here.';
  if (down.length) return `<em>${esc(SHORT_NAMES[down[0].service] || down[0].title)} is ${esc(down[0].state)}.</em> ${running.length} of ${list.length} components are running.`;
  return `The chain is <em>partly running:</em> ${running.length} of ${list.length} components are up.`;
}

function fillStrip() {
  const strip = $('ch-strip');
  if (!strip) return;
  const view = chainInView();
  $('ch-headline').innerHTML = chainHeadline(view);
  $('ch-updated').textContent = state.connected ? `Updated ${ago(state.updated)}` : '▲ Not updating';
  strip.innerHTML = view.list.map((c) => (view.campaign
    ? `<span class="cell ${esc(c.state)}"><span class="dot"></span>${esc(SHORT_NAMES[c.service] || c.service)}</span>`
    : `<button class="cell ${esc(c.state)} ${state.selected === c.service ? 'sel' : ''}" data-svc="${esc(c.service)}" title="${esc(c.role)}">
      <span class="dot"></span>${esc(SHORT_NAMES[c.service] || c.title)}</button>`)).join('');
  const streams = $('ch-streams');
  if (streams) streams.style.display = view.campaign ? 'none' : '';
}

function streamHtml(entries, empty) {
  if (!entries.length) return `<div class="empty">${esc(empty)}</div>`;
  return entries.slice(-80).map((e) => `<div class="l"><span class="t">${clock(e.ts_ms)}</span>${esc(e.text)}</div>`).join('');
}

function setStream(id, html) {
  const box = $(id);
  if (!box || box.dataset.html === html) return;
  const bottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  box.innerHTML = html;
  box.dataset.html = html;
  if (bottom) box.scrollTop = box.scrollHeight;
}

const EVENT_TONE = { started: 'pass', resumed: 'pass', paused: 'warn', killed: 'fail', 'ran out of memory': 'fail', restarted: 'warn' };
const EXPECTED_EXITS = ['exit code 0', 'exit code 137', 'exit code 143']; // 137, 143: what stopping does

const CAMPAIGN_PREFIX = /^campaign-(?:\d{8}-\d{6}(?:-\d+)?-)?/;
function chainName(project) { return project.startsWith('campaign-') ? project.replace(CAMPAIGN_PREFIX, '').replace(/-/g, '_') : 'stack'; }

// Stopping a chain kills its containers within a second or two at the end: that is
// the teardown, not a failure. A kill or a bad exit earlier is.
const TEARDOWN_MS = 2500;
function inTeardown(e, end) { return end != null && e.ts_ms >= end - TEARDOWN_MS; }

function eventTone(e, end) {
  if (e.action === 'exited') return e.detail && !EXPECTED_EXITS.includes(e.detail) && !inTeardown(e, end) ? 'fail' : 'quiet';
  if (e.action === 'killed' && inTeardown(e, end)) return 'quiet';
  return EVENT_TONE[e.action] || 'quiet';
}

// Chains seen in the events, the one that changed last first.
function chainsIn(events) {
  const last = new Map();
  for (const e of events) last.set(e.project, Math.max(last.get(e.project) || 0, e.ts_ms));
  return [...last.entries()].sort((a, b) => b[1] - a[1]).map(([project]) => project);
}

// Intervals in which each container ran or was paused, from its events.
function lanes(events, end) {
  const byService = new Map();
  for (const e of [...events].sort((a, b) => a.ts_ms - b.ts_ms)) {
    if (!byService.has(e.service)) byService.set(e.service, { segments: [], open: null, marks: [] });
    const lane = byService.get(e.service);
    const close = (t) => { if (lane.open) { lane.segments.push({ ...lane.open, end: t }); lane.open = null; } };
    if (e.action === 'started') { close(e.ts_ms); lane.open = { kind: 'running', start: e.ts_ms }; }
    else if (e.action === 'resumed') { close(e.ts_ms); lane.open = { kind: 'running', start: e.ts_ms }; }
    else if (e.action === 'paused') { close(e.ts_ms); lane.open = { kind: 'paused', start: e.ts_ms }; }
    else if (['killed', 'stopped', 'exited'].includes(e.action)) {
      if (eventTone(e, end) === 'fail') lane.marks.push({ t: e.ts_ms, tone: 'fail', text: `${e.action} ${e.detail}` });
      close(e.ts_ms);
    }
  }
  return byService;
}

function timelineHtml(events, running) {
  if (!events.length) return '<p class="state" style="padding-top:8px">Nothing has started or stopped since the dashboard started. Run a campaign to see each scenario build and tear down its chain.</p>';
  const t0 = Math.min(...events.map((e) => e.ts_ms));
  const t1 = Math.max(running ? Date.now() : 0, ...events.map((e) => e.ts_ms));
  // While the chain runs, the axis has a fixed width that grows in steps, so the bars grow
  // from left to right and do not rescale every second. A finished chain fills the axis.
  const elapsed = t1 - t0;
  const span = running ? Math.max(15000, Math.ceil((elapsed + 1500) / 5000) * 5000) : Math.max(1000, elapsed);
  const pct = (t) => (100 * (t - t0) / span).toFixed(3);
  const all = lanes(events, running ? null : Math.max(...events.map((e) => e.ts_ms)));
  const order = [...all.keys()].sort((a, b) => CHAIN_ORDER.indexOf(a) - CHAIN_ORDER.indexOf(b));
  const step = span > 60000 ? 10000 : span > 20000 ? 5000 : span > 8000 ? 2000 : 1000;
  const ticks = [];
  for (let t = 0; t <= span; t += step) ticks.push(t);
  if (running) { while (ticks.length > 12) ticks.splice(1, 1); }
  const rows = order.map((service) => {
    const lane = all.get(service);
    const segments = [...lane.segments];
    if (lane.open) segments.push({ ...lane.open, end: t1 });
    return `<div class="lane"><div class="lane-name">${esc(SHORT_NAMES[service] || service)}</div>
      <div class="lane-track">${segments.map((g) => `<span class="seg ${g.kind}" style="left:${pct(g.start)}%;width:${Math.max(0.25, pct(g.end) - pct(g.start)).toFixed(3)}%"
        title="${esc(`${SHORT_NAMES[service] || service} ${g.kind} for ${((g.end - g.start) / 1000).toFixed(1)} s`)}"></span>`).join('')}
        ${lane.marks.map((m) => `<span class="mark" style="left:${pct(m.t)}%" title="${esc(m.text)}"></span>`).join('')}</div></div>`;
  }).join('');
  return `<div class="timeline" role="img" aria-label="When each container of the chain ran">
    ${rows}
    <div class="lane axis"><div class="lane-name"></div><div class="lane-track">${ticks.map((t) => `<span class="tick" style="left:${(100 * t / span).toFixed(3)}%">${t / 1000} s</span>`).join('')}</div></div></div>
    <div class="legend-row"><span><i class="seg running"></i>running</span><span><i class="seg paused"></i>paused</span><span><i class="mark"></i>killed or failed</span></div>`;
}

function eventsTableHtml(events, running) {
  if (!events.length) return '';
  const end = running ? null : Math.max(...events.map((e) => e.ts_ms));
  const rows = [...events].sort((a, b) => b.ts_ms - a.ts_ms).slice(0, 200).map((e) => `
    <tr><td class="id">${clock(e.ts_ms)}</td><td>${esc(SHORT_NAMES[e.service] || e.service)}</td>
      <td><span class="ev ${eventTone(e, end)}">${esc(e.action)}</span></td><td class="muted">${esc(e.detail)}</td></tr>`).join('');
  return `<table><thead><tr><th>Time</th><th>Container</th><th>Event</th><th>Detail</th></tr></thead><tbody>${rows}</tbody></table>`;
}

function fillChainEvents(everything) {
  const all = everything.filter((e) => e.service !== 'dashboard'); // the dashboard restarting is not the chain
  const chains = chainsIn(all);
  const view = chainInView();
  const auto = view.campaign && view.project ? view.project : chains[0];
  const chosen = state.chainProject && chains.includes(state.chainProject) ? state.chainProject : auto;
  const select = $('ch-project');
  if (select) {
    const key = chains.join() + '|' + chosen;
    if (select.dataset.key !== key) {
      select.innerHTML = chains.map((p) => `<option value="${esc(p)}" ${p === chosen ? 'selected' : ''}>${esc(chainName(p))}</option>`).join('') || '<option>no chain yet</option>';
      select.dataset.key = key;
      select.disabled = !chains.length;
    }
    select.onchange = () => { state.chainProject = select.value === auto ? null : select.value; select.dataset.key = ''; fillChainEvents(everything); };
  }
  const events = all.filter((e) => e.project === chosen);
  const running = view.campaign && view.project === chosen;
  const t0 = events.length ? Math.min(...events.map((e) => e.ts_ms)) : 0;
  $('ch-span').textContent = events.length ? `${chainName(chosen)}, ${((Math.max(running ? Date.now() : 0, ...events.map((e) => e.ts_ms)) - t0) / 1000).toFixed(1)} s${running ? ', running' : ''}` : '';
  const html = timelineHtml(events, running);
  const box = $('ch-timeline');
  if (box && box.dataset.html !== html) { box.innerHTML = html; box.dataset.html = html; }
  const table = $('ch-events');
  const tableHtml = eventsTableHtml(events, running);
  if (table && table.dataset.html !== tableHtml) { table.innerHTML = tableHtml; table.dataset.html = tableHtml; }
}

async function refreshChain() {
  fillStrip();
  try { fillChainEvents((await api('/api/events')).events); } catch (_) { /* keep the last */ }
  if (chainInView().campaign) { await refreshDrawer(); return; }
  const guardian = components().find((c) => c.service === 'guardian');
  const live = guardian && guardian.state === 'running';
  const get = async (kind) => {
    try { return (await api(`/api/components/guardian/logs/${kind}`)).entries; } catch (_) { return []; }
  };
  const [input, output] = live ? await Promise.all([get('input'), get('output')]) : [[], []];
  const hint = live ? 'Nothing observed yet.' : 'The Guardian is not running.';
  setStream('ch-in', streamHtml(input, hint));
  setStream('ch-out', streamHtml(output.filter((e) => !String(e.source).startsWith('log:')), hint));
  let faults = [];
  try { faults = (await api('/api/sovd/faults')).items; } catch (_) { /* OpenSOVD is down */ }
  const open = faults.filter((f) => faultFlags(f).active || faultFlags(f).history);
  setStream('ch-dtc', open.length ? open.map((f) => `<div class="l">${esc(f.code)}  ${esc(statusWords(f))}</div>`).join('')
    : `<div class="empty">${faults.length ? 'No DTC has failed.' : 'OpenSOVD is not reachable.'}</div>`);
  await refreshDrawer();
}

async function refreshDrawer() {
  const box = $('ch-drawer');
  if (!box) return;
  if (!state.selected) { box.innerHTML = ''; return; }
  const c = components().find((x) => x.service === state.selected);
  let entries = [];
  let error = '';
  try {
    entries = (await api(`/api/components/${encodeURIComponent(state.selected)}/logs/container`)).entries;
  } catch (e) { error = e.message; }
  if (!box.firstChild) box.innerHTML = '<section><div class="stream"><h3 id="dr-title"></h3><div class="box" id="dr-box" style="height:240px"></div></div></section>';
  const on = c && ['running', 'restarting', 'paused'].includes(c.state);
  $('dr-title').innerHTML = `${esc(c ? SHORT_NAMES[c.service] || c.title : state.selected)}, container log
    ${isRemote() ? '<span class="small muted" style="float:right">managed by Ankaios</span>'
    : c && c.state !== 'missing' ? `<span style="float:right"><button class="link" data-act="${on ? 'stop' : 'start'}" data-svc="${esc(c.service)}">${on ? 'Stop' : 'Start'}</button>
      ${on ? `<button class="link" style="margin-left:14px" data-act="restart" data-svc="${esc(c.service)}">Restart</button>` : ''}</span>` : ''}`;
  setStream('dr-box', error ? `<div class="empty attn">${esc(error)}</div>` : streamHtml(entries.slice(-200), 'No output.'));
}

// --- diagnostics ----------------------------------------------------------------------------

function faultFlags(f) {
  const s = f.status || {};
  return {
    active: !!s.testFailed, confirmed: !!s.confirmedDtc, pending: !!s.pendingDtc,
    history: !!s.testFailedSinceLastClear, lamp: !!s.warningIndicatorRequested,
    notTested: !!s.testNotCompletedSinceLastClear && !s.testFailedSinceLastClear && !s.testFailed,
  };
}

function statusWords(f) {
  const x = faultFlags(f);
  const out = [];
  if (x.active) out.push('active');
  if (x.confirmed) out.push('confirmed');
  if (x.pending) out.push('pending');
  if (!x.active && x.history) out.push('failed since clear');
  if (x.lamp) out.push('warning lamp');
  if (x.notTested) out.push('not tested');
  else if (!x.active && !x.history) out.push('passed');
  return out.join(', ');
}

function renderDiagnosticsSkeleton() {
  $('view').innerHTML = `
    <div class="context"><span id="dg-updated"></span><span class="grow"></span><button class="link attn" data-clear-all id="dg-clear" hidden>Clear all DTCs</button></div>
    <h1 class="headline" id="dg-headline"></h1>
    <div id="dg-body"></div>`;
  $('view').onclick = onDiagnosticsClick;
}

async function refreshDiagnostics() {
  try {
    state.sovd = await api('/api/sovd/faults');
    state.sovdError = null;
  } catch (error) {
    state.sovd = null;
    state.sovdError = error.message;
  }
  // The DTCs the last campaign raised: they are in its reports, whatever runs now.
  try {
    const list = (await api('/api/campaigns')).campaigns;
    const last = list.find((c) => c.state === 'done' && c.scenarios >= 3) || list[0];
    if (last && (!state.diagCampaign || state.diagCampaign.id !== last.id || last.state !== 'done')) {
      state.diagCampaign = await api('/api/campaigns/' + encodeURIComponent(last.id));
    }
  } catch (_) { /* keep the last */ }
  await Promise.all([...state.openFaults].map(loadFaultDetail));
  fillDiagnostics();
}

// DTC -> where the campaign raised it, from the evidence chains of its reports.
function raisedDtcs(campaign) {
  const found = new Map();
  for (const sc of (campaign ? campaign.scenarios : [])) {
    const report = sc.report;
    if (!report || !report.chain) continue;
    for (const d of report.chain.diagnostics) {
      if (!found.has(d.dtc)) found.set(d.dtc, { dtc: d.dtc, severity: d.severity, type: d.fault_type, by: [], fastest: null, recovered: 0 });
      const f = found.get(d.dtc);
      f.by.push({ id: sc.id, tests: report.hara_tests || [] });
      if (d.latency_ms != null && (f.fastest == null || d.latency_ms < f.fastest)) f.fastest = d.latency_ms;
      if (d.passed_later) f.recovered += 1;
    }
  }
  return [...found.values()].sort((a, b) => (SEVERITY_RANK[b.severity] ?? -1) - (SEVERITY_RANK[a.severity] ?? -1) || a.dtc.localeCompare(b.dtc));
}

async function loadFaultDetail(code) {
  try {
    state.faultDetails[code] = await api('/api/sovd/faults/' + encodeURIComponent(code));
  } catch (error) {
    state.faultDetails[code] = { error: error.message };
  }
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

function faultDetailHtml(code) {
  const d = state.faultDetails[code];
  if (!d) return '<p class="muted">Loading…</p>';
  if (d.error) return `<p class="muted">${esc(d.error)}</p>`;
  const rows = Object.entries(d.environment_data || {});
  return `<p class="facts">${esc(d.symptom || '')}<br>Status bits: <span class="mono">${Object.entries(d.status || {}).map(([k, v]) => `${esc(k)}=${esc(v)}`).join(' · ')}</span></p>
    <h4>Environment data</h4>
    ${rows.length ? `<table><tbody>${rows.map(([k, v]) => `<tr><td class="muted">${esc(k)}</td><td class="mono">${esc(typeof v === 'object' ? JSON.stringify(v) : v)}</td></tr>`).join('')}</tbody></table>`
    : '<p class="muted small">None stored for this fault.</p>'}`;
}

function faultsTable(list) {
  return `<div class="table-wrap"><table><thead><tr><th>Severity</th><th>DTC</th><th>Summary</th><th>Status</th><th class="num">Occurrences</th><th></th></tr></thead><tbody>
    ${list.map((f) => {
    const open = state.openFaults.has(f.code);
    const failed = faultFlags(f).active;
    return `<tr class="row" data-code="${esc(f.code)}" aria-expanded="${open}" tabindex="0"><td class="${failed ? 'attn' : ''}">${failed ? '▲ ' : ''}${esc(f.severity_name)}</td><td class="id">${esc(f.code)}</td>
      <td>${esc((f.catalog && f.catalog.summary) || f.symptom || '')}</td><td class="muted">${esc(statusWords(f))}</td>
      <td class="num">${esc(f.occurrence_counter ?? '—')}</td><td class="num"><button class="link attn" data-clear="${esc(f.code)}" ${isRemote() ? 'disabled' : ''}>Clear</button></td></tr>
      ${open ? `<tr><td class="detail" colspan="6">${faultDetailHtml(f.code)}</td></tr>` : ''}`;
  }).join('')}</tbody></table></div>`;
}

function raisedTable(rows) {
  return `<div class="table-wrap"><table><thead><tr><th>Severity</th><th>DTC</th><th>Raised by</th><th class="num">Reported after</th><th style="padding-left:32px">Cleared again</th></tr></thead><tbody>
    ${rows.map((r) => `<tr><td>${esc(r.severity || '—')}</td><td class="id">${esc(r.dtc)}</td>
      <td>${esc([...new Set(r.by.flatMap((x) => x.tests))].join(', ') || r.by.map((x) => x.id).join(', '))}
        <div class="faint small mono">${esc(r.by.map((x) => x.id).join(' · '))}</div></td>
      <td class="num">${r.fastest == null ? '—' : esc(formatMs(r.fastest))}</td>
      <td style="padding-left:32px" class="${r.recovered === r.by.length ? 't-pass' : 't-warn'}">${r.recovered} of ${r.by.length}</td></tr>`).join('')}</tbody></table></div>`;
}

function fillDiagnostics() {
  const body = $('dg-body');
  if (!body) return;
  $('dg-updated').textContent = state.connected ? `Updated ${ago(state.updated)}` : '▲ Not updating';
  const campaign = state.diagCampaign;
  const raised = raisedDtcs(campaign);
  const live = !!state.sovd;
  const all = live ? sortedFaults() : [];
  const open = all.filter((f) => faultFlags(f).active || faultFlags(f).history);
  const active = all.filter((f) => faultFlags(f).active);
  $('dg-clear').hidden = !live;
  $('dg-clear').disabled = isRemote();
  const label = campaign ? campaignLabel({ id: campaign.id, mode: campaign.mode, scenarios: campaign.scenarios.length, pass: 0, state: campaign.state }).split(' · ')[0] : '';
  if (live && active.length) {
    $('dg-headline').innerHTML = `<em>${plural(active.length, 'DTC')} ${active.length === 1 ? 'is' : 'are'} active</em> in the live chain: ${esc(active.slice(0, 3).map((f) => f.code.replace(/^BTG_/, '')).join(', '))}${active.length > 3 ? ' and more' : ''}.`;
  } else if (live && open.length) {
    $('dg-headline').innerHTML = `Nothing is active now, but <em>${plural(open.length, 'DTC')} failed since the last clear</em> in the live chain.`;
  } else if (raised.length) {
    $('dg-headline').innerHTML = `The campaign of ${esc(label)} raised <em>${plural(raised.length, 'different DTC')}</em> in ${plural(campaign.scenarios.filter((x) => x.report && x.report.chain && x.report.chain.diagnostics.length).length, 'scenario')}.`;
  } else {
    $('dg-headline').innerHTML = live ? 'No DTC has failed. <em>Nothing needs your attention.</em>' : 'No DTCs to show yet. <em>Run a campaign</em> to see which faults the Guardian reports.';
  }
  body.innerHTML = `
    ${raised.length ? `<section><div class="rowhead"><div><h2 class="title">Raised in the last campaign</h2>
      <div class="sub">From the scenario reports, so it stays after the chains are torn down. ${esc(label)}.</div></div></div>${raisedTable(raised)}</section>` : ''}
    <section><div class="rowhead"><div><h2 class="title">Live chain</h2>
      <div class="sub">${live ? esc(state.sovd.entity) + ', from OpenSOVD of the stack you started.' : 'OpenSOVD of the stack is not running. Start the stack under Live chain to see its DTCs here; a campaign has its own chain per scenario.'}</div></div></div>
      ${live ? `${open.length ? faultsTable(open) : '<p class="muted">No DTC has failed in the live chain.</p>'}
      <details data-key="all-dtcs" ${state.showAllFaults ? 'open' : ''}><summary>All ${all.length} DTCs of ${esc(state.sovd.entity)}</summary>${faultsTable(all)}</details>`
    : state.sovdError ? `<details data-key="sovd-error"><summary>Technical detail</summary><pre>${esc(state.sovdError)}</pre></details>` : ''}</section>`;
}

async function onDiagnosticsClick(e) {
  if (e.target.closest('summary')) {
    const details = e.target.closest('details');
    if (details && details.dataset.key === 'all-dtcs') state.showAllFaults = !state.showAllFaults;
    return;
  }
  if (e.target.closest('button[data-clear-all]')) {
    if (!confirm('Clear every DTC in OpenSOVD?')) return;
    try {
      await change('/api/sovd/faults', 'DELETE');
      toast('All DTCs cleared');
      state.faultDetails = {};
      await refreshDiagnostics();
    } catch (error) {
      toast('Clearing failed: ' + error.message, true);
    }
    return;
  }
  const clear = e.target.closest('button[data-clear]');
  if (clear) {
    const code = clear.dataset.clear;
    if (!confirm(`Clear ${code} in OpenSOVD?`)) return;
    try {
      await change('/api/sovd/faults/' + encodeURIComponent(code), 'DELETE');
      toast(code + ' cleared');
      delete state.faultDetails[code];
      await refreshDiagnostics();
    } catch (error) {
      toast(`Clearing ${code} failed: ${error.message}`, true);
    }
    return;
  }
  const row = e.target.closest('tr[data-code]');
  if (!row) return;
  const code = row.dataset.code;
  if (state.openFaults.has(code)) state.openFaults.delete(code);
  else { state.openFaults.add(code); await loadFaultDetail(code); }
  fillDiagnostics();
}

// --- start ---------------------------------------------------------------------------------

window.addEventListener('hashchange', () => {
  const { tab, params } = parseHash();
  if (tab !== state.tab) setTab(tab, params);
});
setupTheme();
{ const { tab, params } = parseHash(); setTab(tab || 'campaign', params); }
setInterval(tick, POLL_MS);
