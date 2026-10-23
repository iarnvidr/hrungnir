(() => {
  'use strict';

  const MAX_IMPORT_BYTES = 4 * 1024 * 1024;
  const PREFERENCES_KEY = 'offline-reader.preferences.v1';
  const $ = (selector) => document.querySelector(selector);
  const ui = {
    connection: $('#connection-status'), dot: $('.status-dot'), form: $('#browse-form'), input: $('#subreddit-input'),
    cacheSearch: $('#cache-search-input'), cacheSearchForm: $('#header-search-form'), cacheSearchSubmit: $('#header-search-submit'), cacheSearchClear: $('#cache-search-clear'), cacheSearchResults: $('#cache-search-results'),
    browse: $('#browse-submit'), browseRefresh: $('#browse-refresh'),
    fetchAllMedia: $('#fetch-all-media'), fetchAllMediaStatus: $('#fetch-all-media-status'),
    newBrowseTab: $('#new-browse-tab'),
    list: $('#thread-list'), count: $('#thread-count'), empty: $('#empty-state'), state: $('#reader-state'),
    feedback: $('#reader-feedback'), view: $('#thread-view'), diagnostics: $('#diagnostics'), detail: $('#diagnostics-detail'),
    diagnosticsStatus: $('#diagnostics-status'), diagnosticsRefresh: $('#diagnostics-refresh'), diagnosticsCopy: $('#diagnostics-copy'), diagnosticsExport: $('#diagnostics-export'),
    supportExport: $('#support-export'), performanceSummary: $('#performance-summary'), theme: $('#theme-select'), accent: $('#accent-select'),
    trustRecoveryPanel: $('#trust-recovery-panel'), trustRecoveryButton: $('#trust-recovery-button'), trustRecoveryStatus: $('#trust-recovery-status'),
    mode: $('#diagnostic-mode'), demo: $('#demo-import'), fileButton: $('#file-import'), file: $('#json-file'),
    clear: $('#clear-cache'), toast: $('#toast'),
    browsePanel: $('#browse-panel'), readerPanel: $('#reader-panel'), libraryPanel: $('#library-panel'), tabs: [...document.querySelectorAll('[data-tab]')],
    subreddits: $('#subreddit-list'), subredditEmpty: $('#subreddit-empty'), subredditOrder: $('#subreddit-order'),
    subredditGroupFilter: $('#subreddit-group-filter'),
    favoriteCurrent: $('#favorite-current'), currentSubreddit: $('#current-subreddit'),
    postSort: $('#post-sort'), topPeriod: $('#top-period'), sortContractNote: $('#sort-contract-note'),
    mediaPresentation: $('#media-presentation'), diagnosticsDialog: $('#diagnostics-dialog'),
    readerTextScale: $('#reader-text-scale'), readerUiScale: $('#reader-ui-scale'),
    readerTextScaleValue: $('#reader-text-scale-value'), readerUiScaleValue: $('#reader-ui-scale-value'),
    diagnosticsOpen: $('#diagnostics-open'), diagnosticsClose: $('#diagnostics-close'),
    settingsDialog: $('#settings-dialog'), settingsOpen: $('#settings-open'), settingsClose: $('#settings-close'),
    postCacheHours: $('#post-cache-hours'), postCacheApply: $('#post-cache-apply'), cacheSettingsStatus: $('#cache-settings-status'),
    postTabs: $('#post-tabs'),
    libraryItems: $('#library-items'), libraryDetail: $('#library-detail'), libraryCount: $('#library-count'), libraryStatus: $('#library-status'),
    librarySearch: $('#library-search'), libraryCollectionFilter: $('#library-collection-filter'), libraryKindFilter: $('#library-kind-filter'),
    libraryExport: $('#library-export'), libraryImport: $('#library-import'),
    saveLinkDetails: $('#save-link-details'), saveLinkUrl: $('#save-link-url'), saveLinkKind: $('#save-link-kind'),
    saveLinkCollection: $('#save-link-collection'), saveLinkForever: $('#save-link-forever'), saveLinkButton: $('#save-link-button'),
    groupSelect: $('#group-select'), groupName: $('#group-name'), groupSubreddits: $('#group-subreddits'),
    groupPostHours: $('#group-post-hours'), groupPostForever: $('#group-post-forever'), groupMediaHours: $('#group-media-hours'),
    groupMediaForever: $('#group-media-forever'), groupSave: $('#group-save'), groupDelete: $('#group-delete'), groupStatus: $('#group-status'),
  };

  const tauri = window.__TAURI__;
  const invoke = tauri && tauri.core && typeof tauri.core.invoke === 'function'
    ? tauri.core.invoke.bind(tauri.core)
    : null;
  let snapshot = null;
  let redditSearch = null;
  let redditSearchSequence = 0;
  let activeRedditSearchOperation = null;
  let libraryState = { groups: [], items: [] };
  let librarySelectedId = null;
  let libraryBusy = false;
  let libraryItemOpenSequence = 0;
  let libraryLoaded = false;
  let libraryLoadSequence = 0;
  let librarySearchSequence = 0;
  let librarySearchTimer = 0;
  let librarySearchHits = [];
  let librarySearchStatus = '';
  let groupEditDirty = false;
  let activeId = null;
  let browseDraftOpen = false;
  let activeSubreddit = 'soccer';
  let pendingAnchor = null;
  let pendingId = null;
  let operationSequence = 0;
  let subredditRequestSequence = 0;
  let cacheSearchSequence = 0;
  let cacheSearchTimer = 0;
  let cacheSettingsSequence = 0;
  let cacheSettingsEditSequence = 0;
  let cacheSettingsDirty = false;
  let appLinksReady = false;
  let appLinkDrainBusy = false;
  let appLinkDrainRequested = false;
  let toastTimer = 0;
  let keyboardThread = 0;
  let lastSavedPosition = null;
  let diagnosticData = null;
  let trustCommandInFlight = false;
  let progressTimer = 0;
  let activeProgress = null;
  let bulkMediaRun = null;
  let lastTabShortcut = { action: '', origin: '', at: 0 };
  const mediaByThread = new Map();
  const mediaHintsByThread = new Map();
  const threadDownloadsInFlight = new Map();
  const postLoadsInFlight = new Map();
  const readerMediaStates = new Map();
  const postScrollPositions = new Map();
  const articleRequests = new Map();
  let selectedFlair = '';
  let flairSource = '';
  const cachedThreadOpensInFlight = new Map();
  const playbackEventTimes = new WeakMap();
  const mediaWatchPlayers = new Map();
  const WATCH_REPORT_INTERVAL_MS = 5000;
  const mediaPlayerSession = Date.now().toString(36);
  let mediaPlayerCounter = 0;
  let playbackEventWindowStarted = 0;
  let playbackEventWindowCount = 0;
  const inlineExpanded = new Set();
  const expandedArticles = new Set();
  const linkArticleStates = new Map();
  const linkPreviewRequests = new Set();
  const linkPreviewCompleted = new Set();
  const listingBySubreddit = new Map();
  const threadCache = new Map();
  const loadedThreadIds = new Set();
  const unavailableThreadIds = new Set();

  function loadPreferences() {
    const defaults = { tab: 'browse', subredditOrder: 'recent', postSort: 'hot', topPeriod: 'day', mediaPresentation: 'inline', readerTextScale: 100, readerUiScale: 100, theme: 'system', accent: 'blue', favorites: [], recent: [], lastSubreddit: 'soccer', listings: {}, postTabs: [], activePostId: null };
    try {
      const saved = JSON.parse(localStorage.getItem(PREFERENCES_KEY) || '{}');
      return {
        ...defaults,
        ...saved,
        tab: ['browse', 'reader', 'library'].includes(saved.tab) ? saved.tab : 'browse',
        subredditOrder: ['recent', 'favorites', 'alphabetical'].includes(saved.subredditOrder) ? saved.subredditOrder : defaults.subredditOrder,
        postSort: ['hot', 'new', 'top', 'rising'].includes(saved.postSort) ? saved.postSort : defaults.postSort,
        topPeriod: ['hour', 'day', 'week', 'month', 'year', 'all'].includes(saved.topPeriod) ? saved.topPeriod : defaults.topPeriod,
        mediaPresentation: saved.mediaPresentation === 'collapsed' ? 'collapsed' : 'inline',
        readerTextScale: Number.isInteger(saved.readerTextScale) && saved.readerTextScale >= 85 && saved.readerTextScale <= 140 ? saved.readerTextScale : defaults.readerTextScale,
        readerUiScale: Number.isInteger(saved.readerUiScale) && saved.readerUiScale >= 85 && saved.readerUiScale <= 125 ? saved.readerUiScale : defaults.readerUiScale,
        theme: ['system', 'classified', 'classified_light'].includes(saved.theme) ? saved.theme : defaults.theme,
        accent: ['blue', 'red', 'amber', 'teal'].includes(saved.accent) ? saved.accent : defaults.accent,
        lastSubreddit: /^[a-z0-9_]{2,21}$/.test(saved.lastSubreddit || '') ? saved.lastSubreddit : defaults.lastSubreddit,
        favorites: Array.isArray(saved.favorites) ? saved.favorites.filter((name) => /^[a-z0-9_]{2,21}$/.test(name)) : [],
        recent: Array.isArray(saved.recent) ? saved.recent.filter((name) => /^[a-z0-9_]{2,21}$/.test(name)).slice(0, 30) : [],
        listings: saved.listings && typeof saved.listings === 'object' ? Object.fromEntries(Object.entries(saved.listings).filter(([name, ids]) => /^[a-z0-9_]{2,21}$/.test(name) && Array.isArray(ids)).slice(0, 30).map(([name, ids]) => [name, ids.filter((id) => /^t3_[A-Za-z0-9_]+$/.test(id)).slice(0, 2000)])) : {},
        postTabs: Array.isArray(saved.postTabs) ? saved.postTabs.filter((tab) => tab && /^t3_[A-Za-z0-9_]+$/.test(tab.id)).slice(0, 30).map((tab) => ({ id: tab.id, anchor: typeof tab.anchor === 'string' ? tab.anchor.slice(0, 100) : tab.id, cachedOnly: tab.cachedOnly === true })) : [],
        activePostId: typeof saved.activePostId === 'string' && /^t3_[A-Za-z0-9_]+$/.test(saved.activePostId) ? saved.activePostId : null,
      };
    } catch (_) { return defaults; }
  }
  const preferences = loadPreferences();
  activeSubreddit = preferences.lastSubreddit;
  ui.input.value = `/r/${activeSubreddit}`;
  ui.subredditOrder.value = preferences.subredditOrder;
  ui.postSort.value = preferences.postSort;
  ui.topPeriod.value = preferences.topPeriod;
  ui.mediaPresentation.value = preferences.mediaPresentation;
  ui.readerTextScale.value = String(preferences.readerTextScale);
  ui.readerUiScale.value = String(preferences.readerUiScale);
  ui.readerTextScaleValue.value = `${preferences.readerTextScale}%`;
  ui.readerUiScaleValue.value = `${preferences.readerUiScale}%`;
  ui.theme.value = preferences.theme;
  ui.accent.value = preferences.accent;

  function applyAppearance() {
    document.documentElement.dataset.theme = preferences.theme;
    document.documentElement.dataset.accent = preferences.accent;
    document.documentElement.style.colorScheme = preferences.theme === 'system' ? 'light dark' : (preferences.theme === 'classified' ? 'dark' : 'light');
    document.documentElement.style.setProperty('--reader-text-scale', String(preferences.readerTextScale / 100));
    document.documentElement.style.setProperty('--reader-ui-scale', String(preferences.readerUiScale / 100));
  }
  applyAppearance();

  function savePreferences() {
    try { localStorage.setItem(PREFERENCES_KEY, JSON.stringify(preferences)); }
    catch (_) { toast('Preferences could not be saved in this webview.'); }
  }

  function setStatus(text, kind = '') {
    ui.connection.textContent = text;
    ui.connection.title = text;
    ui.dot.className = `status-dot${kind ? ` ${kind}` : ''}`;
  }

  function setFeedback(text = '', error = false, retry = null) {
    ui.feedback.replaceChildren();
    ui.feedback.hidden = !text;
    ui.feedback.classList.toggle('error', error);
    if (!text) return;
    ui.feedback.append(document.createTextNode(text));
    if (retry) {
      const button = element('button', 'button secondary retry-action', 'Retry');
      button.type = 'button';
      button.dataset.testid = 'retry-action';
      button.addEventListener('click', retry);
      ui.feedback.append(button);
    }
  }

  function toast(message) {
    ui.toast.textContent = message;
    ui.toast.hidden = false;
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => { ui.toast.hidden = true; }, 2800);
  }

  function explainError(error) {
    if (typeof error === 'string') return error;
    if (error && typeof error.message === 'string') return error.message;
    try { return JSON.stringify(error); } catch (_) { return String(error); }
  }

  function formatDate(timestamp) {
    const date = new Date(Number(timestamp) * 1000);
    if (!Number.isFinite(date.getTime())) return 'time unavailable';
    return date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
  }

  function formatBytes(value) {
    const bytes = Number(value);
    if (!Number.isFinite(bytes) || bytes < 0) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
    return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
  }

  function setReaderMessage(message, error = false) {
    ui.view.hidden = true;
    ui.state.hidden = false;
    ui.state.classList.toggle('error', error);
    ui.state.textContent = message;
  }

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function diagnosticsReport(diagnostics = diagnosticData) {
    return diagnostics && diagnostics.report && typeof diagnostics.report === 'object' ? diagnostics.report : diagnostics;
  }

  function sourceTrustRecovery(report = diagnosticsReport()) {
    const source = report && report.source && typeof report.source === 'object' ? report.source : null;
    return source && source.trust_recovery && typeof source.trust_recovery === 'object' ? source.trust_recovery : null;
  }

  function updateTrustRecovery(report = diagnosticsReport()) {
    if (!ui.trustRecoveryPanel) return;
    const macOS = report && report.platform && report.platform.os === 'macos';
    const trust = sourceTrustRecovery(report);
    const available = Boolean(invoke && macOS && trust && trust.available === true);
    ui.trustRecoveryPanel.hidden = !available;
    if (!available) return;

    const proxy = report.proxy && typeof report.proxy === 'object' ? report.proxy : {};
    const failed = Number(proxy.tls_failed) || 0;
    const ok = Number(proxy.tls_ok) || 0;
    const state = trust.state;
    if (state === 'installing' || trustCommandInFlight) {
      ui.trustRecoveryButton.disabled = true;
      ui.trustRecoveryButton.textContent = 'Waiting for macOS confirmation…';
      ui.trustRecoveryStatus.textContent = 'Confirm the request in the macOS system dialog to continue.';
    } else {
      ui.trustRecoveryButton.disabled = false;
      ui.trustRecoveryButton.textContent = 'Allow proxy certificate…';
      if (state === 'installed') {
        ui.trustRecoveryStatus.textContent = 'Certificate added. Fetch posts again to reconnect.';
      } else if (state === 'failed') {
        ui.trustRecoveryStatus.textContent = 'Certificate setup did not finish. You can try again.';
      } else if (failed > 0 && ok === 0) {
        ui.trustRecoveryStatus.textContent = `No successful Reddit TLS connection is recorded (${failed} failed). Allow the certificate, then fetch posts again.`;
      } else {
        ui.trustRecoveryStatus.textContent = 'Available if Reddit source TLS is failing.';
      }
    }
  }

  function sourceTrustFailureHint() {
    const report = diagnosticsReport();
    if (!invoke || !report || !report.platform || report.platform.os !== 'macos') return '';
    const proxy = report.proxy && typeof report.proxy === 'object' ? report.proxy : {};
    const failed = Number(proxy.tls_failed) || 0;
    const ok = Number(proxy.tls_ok) || 0;
    if (failed <= 0 || ok !== 0) return '';
    const trust = sourceTrustRecovery(report);
    return trust && trust.available === true
      ? ' macOS reports failed Reddit TLS connections; open Diagnostics to review the certificate option, then fetch posts again.'
      : ' macOS reports failed Reddit TLS connections; open Diagnostics to review certificate status.';
  }

  function refreshDiagnostics(data, authoritative = false) {
    const incoming = data && typeof data === 'object'
      ? (authoritative ? data : (data.diagnostics && typeof data.diagnostics === 'object' ? data.diagnostics : data))
      : null;
    if (incoming && (authoritative || !diagnosticData)) diagnosticData = incoming;
    const diagnostics = diagnosticData;
    ui.diagnostics.replaceChildren();
    if (diagnostics && typeof diagnostics === 'object') {
      for (const [key, value] of Object.entries(diagnostics)) {
        const row = element('div');
        const display = value === null || value === undefined ? '—' : typeof value === 'object' ? JSON.stringify(value) : String(value);
        row.append(element('dt', '', key.replaceAll('_', ' ')), element('dd', '', display));
        ui.diagnostics.append(row);
      }
      ui.detail.textContent = JSON.stringify(diagnostics, null, 2);
      renderPerformanceSummary(diagnostics);
      const report = diagnosticsReport(diagnostics);
      updateTrustRecovery(report);
      const platform = report && report.platform;
      const platformLabel = platform && typeof platform === 'object'
        ? [platform.os, platform.arch].filter((value) => typeof value === 'string' && value).join(' ')
        : (typeof platform === 'string' ? platform : '');
      const summary = [diagnostics.version ?? (report && report.version), platformLabel].filter((value) => value !== undefined && value !== null && value !== '').join(' · ');
      ui.mode.textContent = summary;
    } else {
      updateTrustRecovery(null);
      ui.detail.textContent = 'No diagnostics are available.';
      renderPerformanceSummary(null);
      ui.mode.textContent = '';
    }
  }

  function formatElapsed(milliseconds) {
    const seconds = Math.max(0, Number(milliseconds) || 0) / 1000;
    return seconds < 10 ? `${seconds.toFixed(1)} s` : `${Math.round(seconds)} s`;
  }

  function renderPerformanceSummary(diagnostics) {
    if (!ui.performanceSummary) return;
    const report = diagnostics && diagnostics.report && typeof diagnostics.report === 'object' ? diagnostics.report : diagnostics;
    const rows = report && Array.isArray(report.operations) ? report.operations
      .filter((item) => item && typeof item === 'object').slice(-6).reverse() : [];
    ui.performanceSummary.replaceChildren();
    if (!rows.length) {
      ui.performanceSummary.append(element('li', 'muted', 'No recent operations.'));
      return;
    }
    for (const item of rows) {
      const stage = item.stage === 'source' ? 'Subreddit fetch' : item.stage === 'post' ? 'Post and media' : (['source_fetch', 'post_fetch', 'inline_media', 'playback_loadedmetadata', 'playback_playing', 'playback_waiting', 'playback_stalled', 'playback_error', 'playback_seeked'].includes(item.name) ? item.name.replaceAll('_', ' ') : 'Operation');
      const status = item.status === 'failed' ? 'failed' : item.status === 'running' ? 'running' : 'complete';
      const duration = item.elapsed_ms ?? item.duration_ms;
      const count = Number.isFinite(Number(item.count)) ? ` · ${Number(item.count)} items` : '';
      const time = Number.isFinite(Number(duration)) ? ` · ${formatElapsed(duration)}` : '';
      ui.performanceSummary.append(element('li', `performance-${status}`, `${stage} · ${status}${time}${count}`));
    }
  }

  function recordClientEvent(operation, durationMs, count) {
    if (!invoke || !['source_fetch', 'post_fetch', 'inline_media', 'reader_render'].includes(operation)) return;
    const event = {
      name: operation,
      duration_ms: Math.max(0, Math.min(600000, Math.round(Number(durationMs) || 0))),
      count: Math.max(0, Math.min(10000, Math.round(Number(count) || 0))),
      theme: preferences.theme,
      accent: preferences.accent,
    };
    void invoke('record_client_event', { event }).catch(() => {});
  }

  function recordPlaybackEvent(name, player) {
    const allowed = ['playback_loadedmetadata', 'playback_playing', 'playback_waiting', 'playback_stalled', 'playback_error', 'playback_seeked'];
    if (!invoke || !allowed.includes(name)) return;
    const now = performance.now();
    if (now - playbackEventWindowStarted >= 60000) {
      playbackEventWindowStarted = now;
      playbackEventWindowCount = 0;
    }
    if (playbackEventWindowCount >= 30) return;
    const recent = playbackEventTimes.get(player) || new Map();
    if (now - (recent.get(name) || -Infinity) < 5000) return;
    recent.set(name, now);
    playbackEventTimes.set(player, recent);
    playbackEventWindowCount++;
    const event = {
      name,
      current_time: Number.isFinite(player.currentTime) ? Math.min(86400, Math.max(0, player.currentTime)) : undefined,
      duration: Number.isFinite(player.duration) ? Math.min(86400, Math.max(0, player.duration)) : undefined,
      ready_state: Math.max(0, Math.min(4, Number(player.readyState) || 0)),
      network_state: Math.max(0, Math.min(3, Number(player.networkState) || 0)),
      code: player.error ? Math.max(0, Math.min(4, Number(player.error.code) || 0)) : undefined,
      state: name.slice('playback_'.length),
      theme: preferences.theme,
      accent: preferences.accent,
    };
    for (const key of Object.keys(event)) if (event[key] === undefined) delete event[key];
    void invoke('record_client_event', { event }).catch(() => {});
  }

  function installPlaybackTelemetry(player) {
    for (const name of ['loadedmetadata', 'playing', 'waiting', 'stalled', 'error', 'seeked']) {
      player.addEventListener(name, () => recordPlaybackEvent(`playback_${name}`, player));
    }
  }

  function validWatchDuration(value) {
    return Number.isFinite(value) && value > 0 && value <= 86400;
  }

  function playedRanges(player, duration = Number(player.duration)) {
    if (!validWatchDuration(duration)) return [];
    const ranges = [];
    const played = player.played;
    const count = Math.min(Number(played && played.length) || 0, 128);
    for (let index = 0; index < count; index++) {
      let start;
      let end;
      try {
        start = Number(played.start(index));
        end = Number(played.end(index));
      } catch (_) { continue; }
      start = Math.max(0, Math.min(duration, start));
      end = Math.max(0, Math.min(duration, end));
      if (Number.isFinite(start) && Number.isFinite(end) && end > start) ranges.push([start, end]);
    }
    return ranges;
  }

  function sendMediaWatch(player, state, active) {
    if (!invoke || !state) return;
    const duration = Number(player.duration);
    if (Number.isFinite(duration) && duration > 86400) return;
    if (validWatchDuration(duration)) state.lastValidDuration = duration;
    const safeDuration = validWatchDuration(duration) ? duration : state.lastValidDuration;
    if (!validWatchDuration(safeDuration)) return;
    const payload = {
      player_id: state.playerId,
      thread_id: state.threadId,
      file_name: state.fileName,
      duration: safeDuration,
      played_ranges: playedRanges(player, safeDuration),
      active: Boolean(active),
    };
    state.pending = state.pending.catch(() => {}).then(() => invoke('record_media_watch', payload)).catch(() => {});
  }

  function releaseMediaWatchesWithin(container) {
    if (!container || typeof container.querySelectorAll !== 'function') return;
    for (const player of container.querySelectorAll('video, audio')) {
      const state = mediaWatchPlayers.get(player);
      if (!state) continue;
      state.actuallyPlaying = false;
      if (state.leaseTimer !== null) window.clearInterval(state.leaseTimer);
      state.leaseTimer = null;
      try { player.pause(); } catch (_) { /* detached player */ }
      sendMediaWatch(player, state, false);
      mediaWatchPlayers.delete(player);
    }
  }

  function installMediaWatchTracking(player, threadId, fileName) {
    mediaPlayerCounter += 1;
    const playerId = `player-${mediaPlayerSession}-${mediaPlayerCounter.toString(36)}`;
    const state = { playerId, threadId, fileName, actuallyPlaying: false, lastReportedAt: 0, lastValidDuration: 0, leaseTimer: null, pending: Promise.resolve() };
    mediaWatchPlayers.set(player, state);
    const stop = () => {
      state.actuallyPlaying = false;
      if (state.leaseTimer !== null) window.clearInterval(state.leaseTimer);
      state.leaseTimer = null;
      sendMediaWatch(player, state, false);
    };
    const renew = () => {
      if (!state.actuallyPlaying || player.paused || player.ended || !validWatchDuration(Number(player.duration))) {
        stop();
        return;
      }
      state.lastReportedAt = performance.now();
      sendMediaWatch(player, state, true);
    };
    player.addEventListener('playing', () => {
      if (player.paused || player.ended || player.seeking || !validWatchDuration(Number(player.duration))) return;
      state.actuallyPlaying = true;
      state.lastReportedAt = performance.now();
      sendMediaWatch(player, state, true);
      if (state.leaseTimer === null) {
        state.leaseTimer = window.setInterval(renew, WATCH_REPORT_INTERVAL_MS);
      }
    });
    player.addEventListener('timeupdate', () => {
      if (!state.actuallyPlaying || player.paused || player.ended || player.seeking) return;
      const now = performance.now();
      if (now - state.lastReportedAt < WATCH_REPORT_INTERVAL_MS) return;
      renew();
    });
    for (const name of ['pause', 'ended', 'emptied', 'error']) {
      player.addEventListener(name, stop);
    }
  }

  function releaseAllMediaWatches() {
    for (const [player, state] of mediaWatchPlayers) {
      state.actuallyPlaying = false;
      if (state.leaseTimer !== null) window.clearInterval(state.leaseTimer);
      state.leaseTimer = null;
      sendMediaWatch(player, state, false);
    }
  }

  function invalidateThreadMedia(id) {
    loadedThreadIds.delete(id);
    mediaByThread.delete(id);
  }

  function retryMediaControl(id, failure) {
    const retry = element('button', 'button secondary media-retry', 'Retry post and media download');
    retry.type = 'button';
    retry.addEventListener('click', (event) => {
      event.stopPropagation();
      invalidateThreadMedia(id);
      void selectThread(id, id, true);
    });
    failure.append(retry);
  }

  const progressStagePriority = new Map([
    ['media_download', 0], ['media_queue', 1], ['source_teardown', 2], ['parse', 3],
    ['source_startup', 4], ['cache_commit', 5], ['post_source_queue', 6],
    ['listing_queue', 7], ['source', 8], ['post', 9],
  ]);

  function progressStageLabel(stage, message) {
    const listing = /subreddit/i.test(message);
    const labels = {
      listing_queue: 'Queueing subreddit listing',
      post_source_queue: 'Queueing post source',
      source_startup: 'Starting source request',
      source: listing ? 'Fetching subreddit source' : 'Fetching post source',
      parse: 'Parsing source response',
      cache_commit: 'Saving to cache',
      source_teardown: 'Closing source request',
      media_queue: 'Queueing media download',
      media_download: 'Downloading media',
      post: listing ? 'Loading subreddit posts' : 'Fetching post and media',
    };
    return labels[stage] || 'Working';
  }

  function startOperationProgress(message, transition) {
    stopOperationProgress();
    const startedAt = performance.now();
    activeProgress = { message, transition, startedAt, polling: false };
    progressTimer = window.setInterval(async () => {
      const active = activeProgress;
      if (!active || active.transition !== operationSequence || transition !== operationSequence) {
        stopOperationProgress();
        return;
      }
      const elapsed = performance.now() - active.startedAt;
      if (elapsed < 700 || active.polling) return;
      active.polling = true;
      let stage = 'Working';
      let stageElapsed = elapsed;
      try {
        const diagnostics = await invoke('app_diagnostics');
        const report = diagnostics && diagnostics.report && typeof diagnostics.report === 'object' ? diagnostics.report : diagnostics;
        const operations = report && Array.isArray(report.active_operations)
          ? report.active_operations.filter((item) => item && typeof item === 'object') : [];
        operations.sort((a, b) => {
          const priority = (progressStagePriority.get(a.stage) ?? 100) - (progressStagePriority.get(b.stage) ?? 100);
          return priority || (Number(b.started_at_ms) || 0) - (Number(a.started_at_ms) || 0);
        });
        const running = operations[0];
        if (running) stage = progressStageLabel(running.stage, active.message);
        if (running && Number.isFinite(Number(running.elapsed_ms))) stageElapsed = Number(running.elapsed_ms);
      } catch (_) { /* Keep a useful elapsed-time fallback if diagnostics are unavailable. */ }
      if (activeProgress === active && transition === operationSequence) {
        ui.feedback.textContent = `${stage} · ${formatElapsed(stageElapsed)}`;
        setStatus(`${active.message} · ${formatElapsed(stageElapsed)}`);
      }
      active.polling = false;
    }, 800);
  }

  function stopOperationProgress(transition = null) {
    if (transition !== null && (!activeProgress || activeProgress.transition !== transition)) return;
    if (progressTimer) window.clearInterval(progressTimer);
    progressTimer = 0;
    activeProgress = null;
  }

  async function refreshAppDiagnostics() {
    if (!invoke) {
      ui.diagnosticsStatus.textContent = 'Launch the native app to read application diagnostics.';
      return;
    }
    ui.diagnosticsRefresh.disabled = true;
    ui.diagnosticsStatus.textContent = 'Refreshing diagnostics…';
    try {
      const result = await invoke('app_diagnostics');
      if (!result || typeof result !== 'object' || Array.isArray(result)) throw new Error('Native command returned invalid diagnostics.');
      refreshDiagnostics(result, true);
      ui.diagnosticsStatus.textContent = 'Diagnostics refreshed.';
    } catch (error) {
      ui.diagnosticsStatus.textContent = `Could not refresh diagnostics: ${explainError(error)}`;
    } finally {
      ui.diagnosticsRefresh.disabled = false;
    }
  }

  function diagnosticsJson() {
    return diagnosticData ? JSON.stringify(diagnosticData, null, 2) : '';
  }

  async function copyDiagnostics() {
    const json = diagnosticsJson();
    if (!json) { toast('No diagnostics are available to copy.'); return; }
    try {
      if (!navigator.clipboard || typeof navigator.clipboard.writeText !== 'function') throw new Error('Clipboard API unavailable');
      await navigator.clipboard.writeText(json);
      toast('Diagnostics copied.');
      return;
    } catch (_) { /* Try the local webview copy fallback. */ }

    let copied = false;
    const textarea = element('textarea');
    textarea.value = json;
    textarea.setAttribute('readonly', '');
    textarea.style.position = 'fixed';
    textarea.style.left = '-10000px';
    textarea.style.top = '0';
    document.body.append(textarea);
    textarea.focus();
    textarea.select();
    try { copied = document.execCommand('copy'); } catch (_) { /* Use manual selection below. */ }
    textarea.remove();
    if (copied) {
      toast('Diagnostics copied.');
    } else {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(ui.detail);
      if (selection) { selection.removeAllRanges(); selection.addRange(range); }
      ui.diagnosticsStatus.textContent = 'Clipboard access is unavailable. The JSON is selected; copy it manually.';
    }
  }

  function exportDiagnostics() {
    const json = diagnosticsJson();
    if (!json) { toast('No diagnostics are available to export.'); return; }
    let url;
    const link = element('a');
    link.download = 'offline-reader-diagnostics.json';
    link.style.position = 'fixed';
    link.style.left = '-10000px';
    try {
      url = URL.createObjectURL(new Blob([json], { type: 'application/json' }));
      link.href = url;
      document.body.append(link);
      link.click();
    } catch (error) {
      if (url) URL.revokeObjectURL(url);
      toast(`Could not export diagnostics: ${explainError(error)}`);
      return;
    } finally {
      link.remove();
    }
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    toast('Diagnostics exported.');
  }

  async function exportSupportBundle() {
    if (!invoke) {
      ui.diagnosticsStatus.textContent = 'Launch the native app to export a support ZIP.';
      return;
    }
    ui.supportExport.disabled = true;
    ui.supportExport.setAttribute('aria-busy', 'true');
    ui.supportExport.textContent = 'Preparing support ZIP…';
    ui.diagnosticsStatus.textContent = 'Preparing support ZIP…';
    try {
      const result = await invoke('export_support_bundle');
      if (result === null) {
        ui.diagnosticsStatus.textContent = 'Support ZIP export canceled.';
        return;
      }
      if (!result || typeof result.path !== 'string' || !result.path || !Number.isFinite(Number(result.bytes))) {
        throw new Error('Native command returned an invalid support export result.');
      }
      const events = Array.isArray(result.events) ? result.events.length : Number(result.events);
      const eventSummary = Number.isFinite(events) ? ` · ${events} events` : '';
      ui.diagnosticsStatus.textContent = `Support ZIP saved to ${result.path} · ${formatBytes(result.bytes)}${eventSummary}`;
      toast('Support ZIP exported.');
    } catch (error) {
      ui.diagnosticsStatus.textContent = `Could not export support ZIP: ${explainError(error)}`;
    } finally {
      ui.supportExport.disabled = false;
      ui.supportExport.removeAttribute('aria-busy');
      ui.supportExport.textContent = 'Export support ZIP…';
    }
  }

  async function allowProxyCertificate() {
    const report = diagnosticsReport();
    const trust = sourceTrustRecovery(report);
    if (!invoke || trustCommandInFlight || !report || !report.platform || report.platform.os !== 'macos' ||
        !trust || trust.available !== true || ui.trustRecoveryButton.disabled) return;

    let feedback = '';
    trustCommandInFlight = true;
    ui.trustRecoveryButton.disabled = true;
    ui.trustRecoveryButton.textContent = 'Waiting for macOS confirmation…';
    ui.trustRecoveryStatus.textContent = 'Waiting for the native confirmation dialog…';
    ui.diagnosticsStatus.textContent = 'Waiting for macOS confirmation. No certificate is added unless you confirm in the system dialog.';
    try {
      const result = await invoke('trust_source_certificate');
      if (result === null) {
        feedback = 'No certificate was added. You can choose again if you want to continue.';
      } else if (result && result.state === 'installed' && result.host === 'www.reddit.com') {
        feedback = 'Certificate added for www.reddit.com. Fetch posts again to reconnect.';
      } else {
        throw new Error('Native command returned an unexpected certificate result.');
      }
    } catch (error) {
      feedback = `Could not add the proxy certificate: ${explainError(error)}`;
    } finally {
      try {
        const latest = await invoke('app_diagnostics');
        if (latest && typeof latest === 'object' && !Array.isArray(latest)) refreshDiagnostics(latest, true);
      } catch (_) { /* Keep the action result visible if the follow-up diagnostics refresh fails. */ }
      trustCommandInFlight = false;
      updateTrustRecovery(diagnosticsReport());
      if (feedback) {
        ui.trustRecoveryStatus.textContent = feedback;
        ui.diagnosticsStatus.textContent = feedback;
      }
    }
  }

  function setTab(tab, persist = true) {
    const active = ['browse', 'reader', 'library'].includes(tab) ? tab : 'browse';
    ui.browsePanel.hidden = active !== 'browse';
    ui.readerPanel.hidden = active !== 'reader';
    ui.libraryPanel.hidden = active !== 'library';
    for (const button of ui.tabs) {
      const selected = button.dataset.tab === active;
      button.setAttribute('aria-selected', String(selected));
      button.tabIndex = selected ? 0 : -1;
    }
    if (persist) { preferences.tab = active; savePreferences(); }
    if (active === 'library' && invoke && !libraryLoaded) void loadLibraryState();
  }

  function renderSubreddits() {
    const favorites = new Set(preferences.favorites);
    const groupNames = [...new Set(libraryState.groups.flatMap((group) => group.subreddits || []))];
    const selectedGroup = ui.subredditGroupFilter.value;
    ui.subredditGroupFilter.replaceChildren(new Option('All subreddits', ''));
    for (const group of libraryState.groups) ui.subredditGroupFilter.append(new Option(group.name, group.id));
    ui.subredditGroupFilter.value = libraryState.groups.some((group) => group.id === selectedGroup) ? selectedGroup : '';
    const names = [...new Set(preferences.recent.concat(preferences.favorites, groupNames))];
    const group = libraryState.groups.find((item) => item.id === ui.subredditGroupFilter.value);
    let ordered = group ? [...group.subreddits] : names;
    if (preferences.subredditOrder === 'favorites') ordered = ordered.filter((name) => favorites.has(name));
    else if (preferences.subredditOrder === 'alphabetical') ordered = [...ordered].sort((a, b) => a.localeCompare(b));
    ui.subreddits.replaceChildren();
    ui.subredditEmpty.hidden = ordered.length > 0;
    ui.subredditEmpty.textContent = ui.subredditGroupFilter.value ? 'No subreddits in this group.' : (preferences.subredditOrder === 'favorites' ? 'Favorite a subreddit to see it here.' : 'Fetched subreddits will appear here.');
    for (const name of ordered) {
      const row = element('div', 'subreddit-row');
      const visit = element('button', 'subreddit-visit', `r/${name}`);
      visit.type = 'button';
      visit.addEventListener('click', () => void browseSubreddit(`/r/${name}`));
      const favorite = element('button', 'subreddit-star', favorites.has(name) ? '★' : '☆');
      favorite.type = 'button';
      favorite.setAttribute('aria-label', favorites.has(name) ? `Remove r/${name} from favorites` : `Favorite r/${name}`);
      favorite.setAttribute('aria-pressed', String(favorites.has(name)));
      favorite.addEventListener('click', () => toggleFavorite(name));
      row.append(visit, favorite);
      ui.subreddits.append(row);
    }
    const current = (() => { try { return normalizeSubreddit(ui.input.value); } catch (_) { return ''; } })();
    ui.favoriteCurrent.disabled = !current;
    ui.favoriteCurrent.textContent = favorites.has(current) ? '★ Favorited' : '☆ Favorite current';
    ui.favoriteCurrent.setAttribute('aria-pressed', String(favorites.has(current)));
  }

  function normalizeLibraryState(state) {
    if (!state || !Array.isArray(state.groups) || !Array.isArray(state.items)) throw new Error('Native library returned an invalid result.');
    return {
      groups: state.groups.filter((group) => group && typeof group.id === 'string' && typeof group.name === 'string' && Array.isArray(group.subreddits)),
      items: state.items.filter((item) => item && typeof item.id === 'string' && ['post', 'article', 'video', 'image'].includes(item.kind) && item.thread && typeof item.thread === 'object' && Array.isArray(item.media)),
    };
  }

  function acceptLibraryState(state) {
    libraryState = normalizeLibraryState(state);
    libraryLoaded = true;
    for (const item of libraryState.items) {
      if (item.kind !== 'post') {
        mediaByThread.set(item.id, item.media);
      }
    }
    renderSubreddits();
    renderLibrary();
    renderGroupEditor();
    if (ui.librarySearch.value.trim()) scheduleLibrarySearch(0);
  }

  async function loadLibraryState({ quiet = false } = {}) {
    const request = ++libraryLoadSequence;
    if (!quiet) ui.libraryStatus.textContent = 'Loading saved library…';
    try {
      const state = await invoke('library_state');
      if (request !== libraryLoadSequence) return;
      acceptLibraryState(state);
      ui.libraryStatus.textContent = `${libraryState.items.length} saved ${libraryState.items.length === 1 ? 'item' : 'items'}.`;
    } catch (error) {
      if (request === libraryLoadSequence) ui.libraryStatus.textContent = `Could not load the saved library. ${explainError(error)}`;
    }
  }

  function renderLibrary() {
    const query = ui.librarySearch.value.trim();
    const collection = ui.libraryCollectionFilter.value;
    const kind = ui.libraryKindFilter.value;
    const collections = [...new Set(libraryState.items.map((item) => String(item.collection || '').trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));
    const selectedCollection = collection;
    ui.libraryCollectionFilter.replaceChildren(new Option('All collections', ''));
    for (const name of collections) ui.libraryCollectionFilter.append(new Option(name, name));
    ui.libraryCollectionFilter.value = collections.includes(selectedCollection) ? selectedCollection : '';
    const itemsById = new Map(libraryState.items.map((item) => [item.id, item]));
    const filtered = (query
      ? librarySearchHits.map((hit) => ({ hit, item: itemsById.get(hit.id) })).filter((entry) => entry.item)
      : libraryState.items.map((item) => ({ item, hit: null })))
      .filter(({ item }) => (!kind || item.kind === kind) && (!ui.libraryCollectionFilter.value || String(item.collection || '') === ui.libraryCollectionFilter.value));
    ui.libraryCount.textContent = `${filtered.length} of ${libraryState.items.length} saved ${libraryState.items.length === 1 ? 'item' : 'items'}`;
    ui.libraryItems.replaceChildren();
    for (const { item, hit } of filtered) {
      const thread = item.thread || {};
      const row = element('article', `library-item${item.id === librarySelectedId ? ' selected' : ''}`);
      row.dataset.libraryId = item.id;
      const title = element('h2', 'library-item-title', String((hit && hit.title) || thread.title || item.id));
      const commentCount = Number(item.cached_comments) > 0 ? ` · ${item.cached_comments} comments` : '';
      const matchCue = hit && hit.comment_id ? ' · Comment match' : '';
      const meta = element('p', 'library-item-meta', `${item.kind} · ${item.collection || 'Unsorted'}${matchCue}${commentCount}${item.keep_forever ? ' · keep forever' : ''}`);
      const preview = element('p', 'library-item-preview', String((hit && hit.snippet) || thread.body || '').slice(0, 180));
      const actions = element('div', 'library-item-actions');
      const open = element('button', 'button secondary', item.kind === 'post' ? 'Open cached post' : 'Open saved content');
      open.type = 'button';
      open.addEventListener('click', () => void openLibraryItem(item, hit && hit.comment_id));
      const remove = element('button', 'button danger', 'Remove');
      remove.type = 'button';
      remove.addEventListener('click', () => void removeLibraryItem(item.id));
      actions.append(open, remove);
      row.append(title, meta);
      if (preview.textContent) row.append(preview);
      row.append(actions);
      ui.libraryItems.append(row);
    }
    if (!filtered.length) {
      const emptyText = query && librarySearchStatus ? librarySearchStatus
        : libraryState.items.length ? 'No saved items match these filters.' : 'No saved items yet. Save a post or linked item while reading.';
      ui.libraryItems.append(element('p', 'muted empty', emptyText));
    }
    const selected = libraryState.items.find((item) => item.id === librarySelectedId);
    if (selected) renderLibraryDetail(selected);
    else if (!librarySelectedId) ui.libraryDetail.replaceChildren(element('p', 'muted', 'Choose a saved item to read it here. Saved content opens from disk only.'));
  }

  function scheduleLibrarySearch(delay = 200) {
    const query = ui.librarySearch.value.trim();
    const request = ++librarySearchSequence;
    window.clearTimeout(librarySearchTimer);
    if (!query) {
      librarySearchHits = [];
      librarySearchStatus = '';
      renderLibrary();
      return;
    }
    librarySearchHits = [];
    librarySearchStatus = 'Searching saved posts and comments…';
    renderLibrary();
    librarySearchTimer = window.setTimeout(async () => {
      try {
        const rows = await invoke('search_saved_library', { query, limit: 100 });
        if (request !== librarySearchSequence || query !== ui.librarySearch.value.trim()) return;
        if (!Array.isArray(rows)) throw new Error('Native library search returned an invalid result.');
        librarySearchHits = rows.filter((row) => row && typeof row.id === 'string');
        librarySearchStatus = '';
      } catch (error) {
        if (request !== librarySearchSequence || query !== ui.librarySearch.value.trim()) return;
        librarySearchHits = [];
        librarySearchStatus = `Library search failed. ${explainError(error)}`;
      }
      renderLibrary();
    }, Math.max(0, delay));
  }

  function renderLibraryDetail(item) {
    ui.libraryDetail.replaceChildren();
    const thread = item.thread || {};
    ui.libraryDetail.append(element('p', 'library-detail-meta', `${item.kind} · ${item.collection || 'Unsorted'} · saved ${formatDate(item.saved_at)}`));
    ui.libraryDetail.append(element('h2', '', String(thread.title || item.id)));
    const articleBody = element('div', 'thread-body library-article-body');
    appendLinkedText(articleBody, String(thread.body || ''));
    ui.libraryDetail.append(articleBody);
    mediaByThread.set(item.id, item.media || []);
    if (item.media && item.media.length) {
      ui.libraryDetail.append(renderMedia(item.id, { collapsible: false }));
    }
    if (item.kind === 'post' && Array.isArray(thread.comments) && thread.comments.length) {
      ui.libraryDetail.append(element('p', 'muted small', `${thread.comments.length} saved comments · open the post reader for the full discussion.`));
    }
  }

  async function openLibraryItem(item, commentId = null) {
    if (!item || typeof item.id !== 'string') return;
    const operation = ++operationSequence;
    const transition = ++libraryItemOpenSequence;
    ui.libraryStatus.textContent = 'Opening saved content from disk…';
    if (item.kind === 'post') {
      const id = item.id;
      if (!/^t3_[A-Za-z0-9_]+$/.test(id)) {
        ui.libraryStatus.textContent = 'This saved post has an invalid local identifier.';
        return;
      }
      librarySelectedId = id;
      unavailableThreadIds.delete(id);
      threadCache.set(id, item.thread);
      ensurePostTab(item.thread, id, { cachedOnly: true });
      pendingId = id;
      setFeedback('Opening saved post from the local cache…');
      setStatus('Opening saved post from the local cache…');
      try {
        const download = await getCachedThreadDownload(id);
        if (transition !== libraryItemOpenSequence || operation !== operationSequence) return;
        activeId = id;
        pendingId = null;
        const anchor = typeof commentId === 'string' && /^t1_[A-Za-z0-9_]+$/.test(commentId) ? commentId : id;
        pendingAnchor = anchor;
        ensurePostTab(download.thread, anchor, { cachedOnly: true });
        draw();
        await focusAnchor(id, anchor, null);
        if (operation !== operationSequence) return;
        setTab('reader');
        setFeedback('');
        setStatus('Opened saved post · no source fetch', 'ready');
        ui.libraryStatus.textContent = 'Opened saved post from the local cache.';
      } catch (error) {
        if (transition === libraryItemOpenSequence && operation === operationSequence) {
          pendingId = null;
          ui.libraryStatus.textContent = `Could not open saved post from the local cache. ${explainError(error)}`;
          setFeedback(`Could not open this saved post from the local cache. ${explainError(error)}`, true);
          setStatus('Saved post unavailable', 'error');
        }
      }
      return;
    }
    try {
      const download = await invoke('open_cached_thread', { id: item.id });
      if (transition !== libraryItemOpenSequence || operation !== operationSequence) return;
      const thread = validateThreadDownload(download, item.id, { allowNotLoaded: true });
      librarySelectedId = item.id;
      mediaByThread.set(item.id, download.media);
      if (item.kind !== 'post') {
        renderLibrary();
        renderLibraryDetail({ ...item, thread, media: download.media });
        ui.libraryStatus.textContent = 'Opened saved content from the local cache.';
        return;
      }
    } catch (error) {
      if (transition === libraryItemOpenSequence) ui.libraryStatus.textContent = `Could not open saved content from the local cache. ${explainError(error)}`;
      return;
    }
  }

  async function runLibraryMutation(command, args = {}, success = 'Library updated.') {
    if (libraryBusy) throw new Error('Another library operation is still in progress.');
    libraryBusy = true;
    ui.libraryStatus.textContent = 'Saving to the local library…';
    try {
      const state = await invoke(command, args);
      acceptLibraryState(state);
      ui.libraryStatus.textContent = success;
      return state;
    } catch (error) {
      ui.libraryStatus.textContent = `Library action failed. ${explainError(error)}`;
      throw error;
    } finally { libraryBusy = false; }
  }

  async function removeLibraryItem(id) {
    try {
      await runLibraryMutation('remove_library_item', { id }, 'Saved item removed; cached post text and media are no longer protected by the library.');
      if (librarySelectedId === id) {
        librarySelectedId = null;
        renderLibrary();
      }
    } catch (_) { /* Inline library status contains the native error. */ }
  }

  function renderGroupEditor(group = null) {
    const selected = group || libraryState.groups.find((item) => item.id === ui.groupSelect.value) || null;
    const selectedId = selected ? selected.id : '';
    ui.groupSelect.replaceChildren(new Option('New group', ''));
    for (const item of libraryState.groups) ui.groupSelect.append(new Option(item.name, item.id));
    ui.groupSelect.value = selectedId;
    if (!groupEditDirty) {
      ui.groupName.value = selected ? selected.name : '';
      ui.groupSubreddits.value = selected ? selected.subreddits.join('\n') : '';
      ui.groupPostForever.checked = selected ? selected.post_cache_hours === null : false;
      ui.groupPostHours.value = String(selected && selected.post_cache_hours !== null ? selected.post_cache_hours : 48);
      ui.groupMediaForever.checked = selected ? selected.watched_media_hours === null : false;
      ui.groupMediaHours.value = String(selected && selected.watched_media_hours !== null ? selected.watched_media_hours : 24);
    }
    ui.groupDelete.disabled = !selected;
    ui.groupPostHours.disabled = ui.groupPostForever.checked;
    ui.groupMediaHours.disabled = ui.groupMediaForever.checked;
  }

  async function saveGroup() {
    const hours = (input, forever, label) => {
      if (forever.checked) return null;
      const value = Number(input.value);
      if (!Number.isInteger(value) || value < 1 || value > 8760) throw new Error(`${label} must be 1–8760 hours or forever.`);
      return value;
    };
    const name = ui.groupName.value.trim();
    if (!name) { ui.groupStatus.textContent = 'Enter a group name.'; ui.groupName.focus(); return; }
    const subreddits = [...new Set(ui.groupSubreddits.value.split(/[\s,]+/).map((value) => value.trim().replace(/^\/?r\//i, '').toLowerCase()).filter(Boolean))];
    if (subreddits.some((subreddit) => !/^[a-z0-9_]{2,21}$/.test(subreddit))) {
      ui.groupStatus.textContent = 'Use subreddit names such as soccer, one per line or separated by commas.';
      ui.groupSubreddits.focus();
      return;
    }
    const selected = libraryState.groups.find((item) => item.id === ui.groupSelect.value);
    let postHours;
    let mediaHours;
    try {
      postHours = hours(ui.groupPostHours, ui.groupPostForever, 'Post lifetime');
      mediaHours = hours(ui.groupMediaHours, ui.groupMediaForever, 'Watched-media lifetime');
    } catch (error) {
      ui.groupStatus.textContent = error.message;
      const field = error.message.startsWith('Post') ? ui.groupPostHours : ui.groupMediaHours;
      field.setAttribute('aria-invalid', 'true');
      field.focus();
      return;
    }
    ui.groupPostHours.removeAttribute('aria-invalid');
    ui.groupMediaHours.removeAttribute('aria-invalid');
    const group = {
      id: selected ? selected.id : (globalThis.crypto && typeof crypto.randomUUID === 'function' ? crypto.randomUUID() : `group_${Date.now().toString(36)}`),
      name, subreddits, post_cache_hours: postHours, watched_media_hours: mediaHours,
    };
    ui.groupSave.disabled = true;
    ui.groupStatus.textContent = 'Saving group…';
    let groupSaved = false;
    try {
      const state = await invoke('put_subreddit_group', { group });
      groupSaved = true;
      groupEditDirty = false;
      acceptLibraryState(state);
      await refreshCacheMembership();
      ui.groupStatus.textContent = 'Group saved.';
    } catch (error) {
      ui.groupStatus.textContent = groupSaved
        ? `Group saved, but cached posts could not be refreshed. ${explainError(error)}`
        : `Could not save group. ${explainError(error)}`;
    }
    finally { ui.groupSave.disabled = false; }
  }

  async function deleteGroup() {
    const group = libraryState.groups.find((item) => item.id === ui.groupSelect.value);
    if (!group) return;
    ui.groupDelete.disabled = true;
    ui.groupStatus.textContent = 'Deleting group…';
    let groupDeleted = false;
    try {
      const state = await invoke('delete_subreddit_group', { id: group.id });
      groupDeleted = true;
      groupEditDirty = false;
      acceptLibraryState(state);
      await refreshCacheMembership();
      ui.groupSelect.value = '';
      renderGroupEditor();
      ui.groupStatus.textContent = `Deleted “${group.name}”. Subreddits are no longer in a group.`;
    } catch (error) {
      ui.groupStatus.textContent = groupDeleted
        ? `Group deleted, but cached posts could not be refreshed. ${explainError(error)}`
        : `Could not delete group. ${explainError(error)}`;
    }
    finally { ui.groupDelete.disabled = !ui.groupSelect.value; }
  }

  async function saveCurrentPost() {
    const thread = currentThread();
    if (!thread) return;
    const collection = $('#save-post-collection').value.trim();
    const keepForever = $('#save-post-forever').checked;
    const button = $('#save-post-button');
    const status = $('#save-post-status');
    button.disabled = true;
    button.textContent = 'Saving…';
    status.textContent = 'Downloading an offline copy. You can keep browsing.';
    try {
      await runLibraryMutation('save_library_post', { id: thread.id, collection, keep_forever: keepForever }, 'Post saved in the local library.');
      if (activeId === thread.id) {
        ui.view.querySelector('.reader-library-actions')?.replaceWith(renderReaderLibraryActions(thread));
        $('#save-post-status').textContent = 'Saved for offline reading.';
        const item = libraryState.items.find((item) => item.id === thread.id);
        if (item?.media?.length) { mediaByThread.set(thread.id, item.media); refreshReaderMedia(thread.id); }
      }
    } catch (error) {
      status.textContent = `Could not save this post. ${explainError(error)}`;
    } finally { if (button.isConnected) { button.disabled = false; button.textContent = 'Save post'; } }
  }

  async function saveLinkedContent() {
    const url = ui.saveLinkUrl.value.trim();
    const kind = ui.saveLinkKind.value;
    const collection = ui.saveLinkCollection.value.trim();
    const keepForever = ui.saveLinkForever.checked;
    const status = $('#save-link-status');
    if (!url) { status.textContent = 'Enter a public link URL.'; ui.saveLinkUrl.focus(); return; }
    if (libraryBusy) { status.textContent = 'Another library operation is still in progress.'; return; }
    libraryBusy = true;
    status.textContent = 'Saving linked content and downloading its approved local media…';
    ui.saveLinkButton.disabled = true;
    try {
      const state = await invoke('save_linked_content', { url, kind, collection, keep_forever: keepForever });
      acceptLibraryState(state);
      status.textContent = 'Linked content saved locally.';
    } catch (error) { status.textContent = `Could not save linked content. ${explainError(error)}`; }
    finally { libraryBusy = false; ui.saveLinkButton.disabled = false; }
  }

  function openLibraryLinkForm(thread) {
    const suggestedUrl = String(thread && thread.link_url || '').trim();
    if (suggestedUrl) ui.saveLinkUrl.value = suggestedUrl;
    const hint = thread && mediaHintsByThread.get(thread.id);
    ui.saveLinkKind.value = ['video', 'image'].includes(hint?.kind) ? hint.kind : 'article';
    const url = ui.saveLinkUrl.value.trim();
    const kind = ui.saveLinkKind.value;
    const existing = libraryState.items.find((item) => item.kind === kind && String(item.thread && item.thread.link_url || '').trim() === url);
    if (existing) {
      ui.saveLinkCollection.value = existing.collection || 'Saved';
      ui.saveLinkForever.checked = Boolean(existing.keep_forever);
    } else {
      ui.saveLinkCollection.value = 'Saved';
      ui.saveLinkForever.checked = true;
    }
    setTab('library');
    ui.saveLinkDetails.open = true;
    ui.saveLinkUrl.focus();
  }

  async function exportLibrary() {
    ui.libraryExport.disabled = true;
    ui.libraryStatus.textContent = 'Preparing library export…';
    try {
      const result = await invoke('export_library_bundle');
      ui.libraryStatus.textContent = result ? `Library exported to ${result.path || 'the selected file'}.` : 'Export cancelled.';
    } catch (error) { ui.libraryStatus.textContent = `Could not export the library. ${explainError(error)}`; }
    finally { ui.libraryExport.disabled = false; }
  }

  async function importLibrary() {
    ui.libraryImport.disabled = true;
    ui.libraryStatus.textContent = 'Choose a library bundle to import…';
    try {
      const state = await invoke('import_library_bundle');
      if (state) {
        acceptLibraryState(state);
        ui.libraryStatus.textContent = 'Library bundle imported.';
      } else ui.libraryStatus.textContent = 'Import cancelled.';
    } catch (error) { ui.libraryStatus.textContent = `Could not import the library. ${explainError(error)}`; }
    finally { ui.libraryImport.disabled = false; }
  }

  function toggleFavorite(name) {
    const favorites = new Set(preferences.favorites);
    if (favorites.has(name)) favorites.delete(name); else favorites.add(name);
    preferences.favorites = [...favorites].sort((a, b) => a.localeCompare(b));
    savePreferences();
    renderSubreddits();
  }

  function recordRecent(name) {
    preferences.recent = [name, ...preferences.recent.filter((item) => item !== name)].slice(0, 30);
    savePreferences();
    renderSubreddits();
  }

  function updateSortNotice() {
    const sort = redditSearch ? redditSearch.sort : preferences.postSort;
    const period = redditSearch ? redditSearch.period : preferences.topPeriod;
    for (const option of ui.postSort.options) {
      const hiddenForContext = redditSearch
        ? option.dataset.sortScope === 'subreddit'
        : option.dataset.sortScope === 'search';
      option.hidden = hiddenForContext;
      option.disabled = hiddenForContext;
    }
    ui.sortContractNote.hidden = redditSearch ? true : (sort === 'top' || period === 'day');
    $('#top-period-label').textContent = redditSearch ? 'Time period' : 'Top period';
    $('#top-period-wrap').hidden = redditSearch ? false : sort !== 'top';
  }

  function renderList() {
    const sourceThreads = redditSearch ? redditSearch.threads : (listingBySubreddit.get(activeSubreddit) || []);
    const sourceKey = redditSearch ? `search:${redditSearch.query}` : activeSubreddit;
    if (flairSource !== sourceKey) { selectedFlair = ''; flairSource = sourceKey; }
    const flairs = [...new Set(sourceThreads.map((thread) => thread.flair).filter(Boolean))].sort();
    if (!flairs.includes(selectedFlair)) selectedFlair = '';
    const filter = $('#flair-filter');
    filter.replaceChildren(new Option('All flairs', ''));
    for (const flair of flairs) filter.append(new Option(flair, flair));
    filter.value = selectedFlair;
    $('#flair-filter-wrap').hidden = flairs.length === 0;
    const threads = selectedFlair ? sourceThreads.filter((thread) => thread.flair === selectedFlair) : sourceThreads;
    updateSortNotice();
    ui.postSort.value = redditSearch ? redditSearch.sort : preferences.postSort;
    ui.topPeriod.value = redditSearch ? redditSearch.period : preferences.topPeriod;
    const current = activeSubreddit || (threads.length ? threads[0].subreddit : '');
    ui.currentSubreddit.textContent = redditSearch ? `Reddit results for “${redditSearch.query}”` : (current ? `Posts in r/${current}` : 'Choose a subreddit');
    ui.count.textContent = `${threads.length} ${threads.length === 1 ? 'post' : 'posts'}`;
    updateFetchAllMediaControl();
    releaseMediaWatchesWithin(ui.list);
    ui.list.replaceChildren();
    ui.empty.hidden = threads.length > 0;
    ui.empty.textContent = redditSearch ? `No Reddit posts found for “${redditSearch.query}”. Try another query or sort.` : 'Fetch a subreddit above. Previously fetched posts remain available here.';
    threads.forEach((thread, index) => {
      const card = element('article', 'thread-item');
      card.dataset.threadId = thread.id;
      card.dataset.threadIndex = String(index);
      card.setAttribute('aria-current', String(thread.id === activeId));
      card.setAttribute('aria-busy', String(thread.id === pendingId));
      const sub = element('span', 'thread-subreddit', `r/${thread.subreddit}`);
      const title = element('span', 'thread-title', thread.title);
      const open = element('button', 'thread-open');
      open.type = 'button';
      const preview = snapshot && snapshot.link_previews && snapshot.link_previews[thread.id];
      const previewUrl = preview && localAssetUrl(preview.thumbnail);
      const thumb = element('span', 'thread-thumb', previewUrl ? '' : linkPreviewMark(preview, thread));
      thumb.dataset.previewFor = thread.id;
      if (preview && preview.domain) thumb.title = String(preview.domain);
      if (previewUrl) {
        const image = element('img', '', '');
        image.src = previewUrl;
        image.alt = '';
        image.loading = 'lazy';
        thumb.append(image);
      }
      const meta = element('span', 'thread-meta');
      meta.append(sub);
      if (isExternalWebLink(thread.link_url)) meta.append(element('span', 'thread-domain', safeDomain(thread.link_url)));
      open.append(thumb, element('span', 'thread-open-copy'));
      open.lastChild.append(title, meta);
      open.addEventListener('click', (event) => {
        if (event.metaKey || event.ctrlKey) openPostInBackground(thread);
        else void selectThread(thread.id, thread.id);
      });
      open.addEventListener('auxclick', (event) => {
        if (event.button === 1) { event.preventDefault(); openPostInBackground(thread); }
      });
      const actions = element('div', 'thread-card-actions');
      if (thread.flair) actions.append(flairButton(thread.flair));
      const background = element('button', 'link-button background-tab-open', 'Open in background');
      background.type = 'button';
      background.textContent = '';
      background.innerHTML = '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><rect x="7" y="7" width="14" height="14" rx="2"/><path d="M16 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h2"/></svg>';
      background.title = 'Open in background tab (⌘/Ctrl-click or middle-click)';
      background.setAttribute('aria-label', `Open ${thread.title} in a background tab`);
      background.addEventListener('click', () => openPostInBackground(thread));
      actions.append(background);
      const hint = mediaHintsByThread.get(thread.id);
      const assets = mediaByThread.get(thread.id) || [];
      const cachedKinds = [...new Set(assets.map((asset) => asset && asset.kind).filter((kind) => ['image', 'video', 'audio'].includes(kind)))];
      const kindLabel = (kind) => ({ image: 'image', video: 'video', audio: 'audio', media: 'media' })[kind] || 'media';
      const cachedLabel = cachedKinds.length ? `Cached ${cachedKinds.map(kindLabel).join(' + ')}` : '';
      const hintedLabel = hint && ['image', 'video', 'audio', 'media'].includes(hint.kind)
        ? `${kindLabel(hint.kind)}${Number(hint.count) > 1 ? ` · ${Number(hint.count)}` : ''}` : '';
      const hintCue = hintedLabel ? `${hintedLabel[0].toUpperCase()}${hintedLabel.slice(1)}` : '';
      const cue = cachedLabel || hintCue;
      const articleLink = isExternalWebLink(thread.link_url) && !['video', 'image', 'audio'].includes((hint && hint.kind) || '') && !cachedKinds.some((kind) => ['video', 'image', 'audio'].includes(kind));
      if (articleLink) {
        const expanded = expandedArticles.has(thread.id);
        const expand = element('button', 'button secondary link-article-toggle', expanded ? 'Hide article' : 'Expand article');
        expand.type = 'button';
        expand.setAttribute('aria-expanded', String(expanded));
        expand.setAttribute('aria-label', `${expanded ? 'Hide' : 'Expand'} linked article for ${thread.title}`);
        expand.addEventListener('click', (event) => { event.stopPropagation(); void toggleLinkArticle(thread); });
        actions.append(expand);
        const external = element('button', 'link-button open-web-link', 'Open link');
        external.type = 'button';
        external.setAttribute('aria-label', `Open external link for ${thread.title}`);
        external.addEventListener('click', (event) => { event.stopPropagation(); void openWebLink(thread.link_url); });
        actions.append(external);
      }
      if (cue) {
        actions.append(element('span', cachedLabel ? 'media-kind-cue cached' : 'media-kind-cue', cue));
        const expanded = inlineExpanded.has(thread.id);
        const verb = expanded ? 'Hide' : (cachedLabel ? 'Show' : 'Load');
        const actionKind = cachedKinds.length > 1 ? 'media' : (cachedKinds[0] || (hint && hint.kind) || 'media');
        const mediaButton = element('button', 'button secondary inline-media-toggle', `${verb} ${kindLabel(actionKind)}`);
        mediaButton.type = 'button';
        mediaButton.dataset.testid = 'inline-media-toggle';
        mediaButton.setAttribute('aria-expanded', String(expanded));
        mediaButton.addEventListener('click', () => void toggleInlineMedia(thread.id, card));
        actions.append(mediaButton);
      }
      const inline = element('div', 'inline-media');
      inline.hidden = !inlineExpanded.has(thread.id) && !expandedArticles.has(thread.id);
      inline.dataset.inlineMediaFor = thread.id;
      if (inlineExpanded.has(thread.id) && mediaByThread.has(thread.id)) inline.append(renderMedia(thread.id, { collapsible: false, inline: true }));
      if (expandedArticles.has(thread.id)) inline.append(renderLinkArticle(thread));
      card.append(open, actions, inline);
      ui.list.append(card);
    });
    const index = threads.findIndex((thread) => thread.id === activeId);
    keyboardThread = index < 0 ? 0 : index;
  }

  function flairButton(flair) {
    const button = element('button', 'post-flair', flair);
    button.type = 'button';
    button.setAttribute('aria-label', `Filter posts by ${flair}`);
    button.setAttribute('aria-pressed', String(selectedFlair === flair));
    button.addEventListener('click', (event) => {
      event.stopPropagation();
      selectedFlair = selectedFlair === flair ? '' : flair;
      setTab('browse');
      renderList();
    });
    return button;
  }
  $('#flair-filter').addEventListener('change', (event) => { selectedFlair = event.target.value; renderList(); });

  function requestLinkPreviews(threads) {
    if (!invoke || !Array.isArray(threads)) return;
    const ids = [...new Set(threads.map((thread) => thread && thread.id).filter((id) => typeof id === 'string' && id))]
      .filter((id) => {
        const preview = snapshot && snapshot.link_previews && snapshot.link_previews[id];
        if (preview && localAssetUrl(preview.thumbnail)) {
          linkPreviewCompleted.add(id);
          return false;
        }
        return !linkPreviewCompleted.has(id) && !linkPreviewRequests.has(id);
      });
    if (!ids.length) return;
    ids.forEach((id) => linkPreviewRequests.add(id));
    void fetchLinkPreviewBatches(ids);
  }

  async function fetchLinkPreviewBatches(ids) {
    // Publish each small group as it completes instead of waiting for every
    // thumbnail in the listing (including slow or unavailable hosts).
    for (let start = 0; start < ids.length; start += 4) {
      const batch = ids.slice(start, start + 4);
      try {
        const result = await invoke('fetch_link_previews', { ids: batch });
        const previews = result && result.link_previews && typeof result.link_previews === 'object' ? result.link_previews : result;
        for (const id of batch) {
          linkPreviewRequests.delete(id);
          if (!snapshot || !Array.isArray(snapshot.threads)) continue;
          const thread = snapshot.threads.find((item) => item.id === id);
          const preview = previews && previews[id];
          if (!thread) continue;
          if (!preview || typeof preview !== 'object') {
            if (!thread.thumbnail_url) linkPreviewCompleted.add(id);
            continue;
          }
          const previewsById = { ...(snapshot.link_previews || {}) };
          const thumbnail = localAssetUrl(preview.thumbnail);
          previewsById[id] = { ...preview, thumbnail: thumbnail || undefined };
          snapshot.link_previews = previewsById;
          patchLinkPreviewRow(id, thread, previewsById[id]);
          if (thumbnail || !(preview.thumbnail_url || thread.thumbnail_url)) linkPreviewCompleted.add(id);
        }
      } catch (_) {
        batch.forEach((id) => linkPreviewRequests.delete(id));
      }
    }
  }

  function patchLinkPreviewRow(id, thread, preview) {
    const thumb = [...ui.list.querySelectorAll('[data-preview-for]')]
      .find((node) => node.dataset.previewFor === id);
    if (!thumb || !thumb.closest('.thread-item') || thumb.closest('.thread-item').dataset.threadId !== id) return;
    thumb.replaceChildren();
    const url = localAssetUrl(preview.thumbnail);
    if (url) {
      const image = element('img');
      image.src = url;
      image.alt = '';
      image.loading = 'lazy';
      thumb.append(image);
    } else thumb.textContent = linkPreviewMark(preview, thread);
    thumb.title = preview.domain ? String(preview.domain) : '';
  }

  function linkPreviewMark(preview, thread) {
    const kind = String(preview && preview.kind || '').toLowerCase();
    if (kind === 'video') return '▶';
    if (kind === 'image') return '▧';
    if (kind === 'article' || isExternalWebLink(thread.link_url)) return '↗';
    return '•';
  }

  function renderLinkArticle(thread) {
    const state = linkArticleStates.get(thread.id) || { status: 'idle' };
    const panel = element('section', 'link-article-panel');
    panel.setAttribute('aria-label', `Linked article for ${thread.title}`);
    if (state.status === 'loading') panel.append(element('p', 'small muted', 'Loading readable article…'));
    else if (state.status === 'error') panel.append(element('p', 'small error-text', state.message || 'Could not load the linked article.'));
    else if (state.article) {
      panel.append(element('h2', 'link-article-title', state.article.title || thread.title));
      if (state.article.url) panel.append(element('p', 'link-article-domain muted small', safeDomain(state.article.url)));
      const body = element('div', 'link-article-body');
      appendLinkedText(body, state.article.body || 'No readable article text was found.');
      panel.append(body);
      const openSource = element('button', 'link-button open-web-link', 'Open original link');
      openSource.type = 'button';
      openSource.addEventListener('click', () => void openWebLink(state.article.url || thread.link_url));
      panel.append(openSource);
    } else panel.append(element('p', 'small muted', 'Expand to fetch a readable text copy of this link.'));
    return panel;
  }

  function safeDomain(value) {
    try { return new URL(value).hostname; } catch (_) { return ''; }
  }

  function appendLinkedText(container, value) {
    const text = String(value || '');
    const pattern = /https:\/\/[^\s<>"'`]+/gi;
    let cursor = 0;
    for (const match of text.matchAll(pattern)) {
      const start = match.index;
      let candidate = match[0];
      while (/[.,;:!?]$/.test(candidate)) candidate = candidate.slice(0, -1);
      while (candidate.endsWith(')') && (candidate.match(/\(/g) || []).length < (candidate.match(/\)/g) || []).length) candidate = candidate.slice(0, -1);
      while (candidate.endsWith(']') && (candidate.match(/\[/g) || []).length < (candidate.match(/\]/g) || []).length) candidate = candidate.slice(0, -1);
      if (!candidate || start < cursor) continue;
      container.append(document.createTextNode(text.slice(cursor, start)));
      const url = safeHttpsUrl(candidate);
      if (!url) {
        container.append(document.createTextNode(candidate));
      } else {
        const link = element('a', 'reader-text-link', candidate);
        link.href = url;
        link.setAttribute('aria-label', `Open link ${safeDomain(url)}`);
        link.addEventListener('click', (event) => { event.preventDefault(); void openTextLink(url, event.metaKey || event.ctrlKey); });
        link.addEventListener('auxclick', (event) => { if (event.button === 1) { event.preventDefault(); void openTextLink(url, true); } });
        container.append(link);
      }
      cursor = start + candidate.length;
    }
    container.append(document.createTextNode(text.slice(cursor)));
  }

  function safeHttpsUrl(value) {
    try {
      const url = new URL(value);
      return url.protocol === 'https:' && !url.username && !url.password ? url.href : null;
    } catch (_) { return null; }
  }

  function isExternalWebLink(value) {
    try {
      const url = new URL(value);
      return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password &&
        url.hostname !== 'reddit.com' && !url.hostname.endsWith('.reddit.com');
    } catch (_) { return false; }
  }

  async function toggleLinkArticle(thread) {
    if (expandedArticles.has(thread.id)) {
      expandedArticles.delete(thread.id);
      refreshArticlePanels(thread);
      return;
    }
    expandedArticles.add(thread.id);
    refreshArticlePanels(thread);
    const cached = linkArticleStates.get(thread.id);
    if (cached && cached.status === 'loaded') return;
    await loadArticle(thread);
  }

  function refreshArticlePanels(thread) {
    const card = ui.list.querySelector(`[data-thread-id="${CSS.escape(thread.id)}"]`);
    if (card) {
      const region = card.querySelector('.inline-media');
      const toggle = card.querySelector('.link-article-toggle');
      const expanded = expandedArticles.has(thread.id);
      if (toggle) { toggle.textContent = expanded ? 'Hide article' : 'Expand article'; toggle.setAttribute('aria-expanded', String(expanded)); }
      if (region && !inlineExpanded.has(thread.id)) {
        region.replaceChildren();
        region.hidden = !expanded;
        if (expanded) region.append(renderLinkArticle(thread));
      }
    }
    if (activeId === thread.id) {
      const slot = ui.view.querySelector('.reader-article-slot');
      if (slot) slot.replaceChildren(renderLinkArticle(thread));
    }
  }

  function loadArticle(thread) {
    if (articleRequests.has(thread.id)) return articleRequests.get(thread.id);
    if (linkArticleStates.get(thread.id)?.status === 'loaded') return Promise.resolve();
    linkArticleStates.set(thread.id, { status: 'loading' });
    refreshArticlePanels(thread);
    const pending = invoke('fetch_link_article', { id: thread.id }).then((article) => {
      linkArticleStates.set(thread.id, { status: 'loaded', article: article || {} });
    }).catch((error) => {
      linkArticleStates.set(thread.id, { status: 'error', message: `Could not load article. ${explainError(error)}` });
    }).finally(() => { articleRequests.delete(thread.id); refreshArticlePanels(thread); });
    articleRequests.set(thread.id, pending);
    return pending;
  }

  async function openWebLink(url) {
    try { await invoke('open_web_link', { url }); }
    catch (error) { toast(`Could not open link. ${explainError(error)}`); }
  }

  async function openTextLink(value, background = false) {
    const url = new URL(value);
    const reddit = url.hostname === 'reddit.com' || url.hostname.endsWith('.reddit.com');
    const post = reddit && url.pathname.match(/^\/r\/([a-z0-9_]{2,21})\/comments\/([a-z0-9]+)(?:\/|$)/i);
    if (!post) {
      const subreddit = reddit && url.pathname.match(/^\/r\/([a-z0-9_]{2,21})\/?$/i);
      if (subreddit) return browseSubreddit(`/r/${subreddit[1]}`);
      return openWebLink(value);
    }
    const id = `t3_${post[2]}`;
    const known = threadCache.get(id);
    if (known && !unavailableThreadIds.has(id)) {
      if (background) openPostInBackground(known);
      else await selectThread(id);
      return;
    }
    const transition = background ? null : ++operationSequence;
    if (!background) { setTab('reader'); setReaderMessage('Opening linked post…'); }
    try {
      const result = await invoke('open_link_post', { url: value });
      const thread = acceptThreadDownload(result, id, { allowNotLoaded: true });
      mediaByThread.set(id, result.media);
      ensurePostTab(thread, id, { activate: !background });
      if (!background && transition === operationSequence) {
        activeId = id;
        browseDraftOpen = false;
        pendingAnchor = null;
        draw();
        window.scrollTo(0, 0);
      }
    } catch (error) { toast(`Could not open linked post. ${explainError(error)}`); }
  }

  function makeCommentTree(thread) {
    const byParent = new Map();
    for (const comment of thread.comments || []) {
      const siblings = byParent.get(comment.parent_id) || [];
      siblings.push(comment);
      byParent.set(comment.parent_id, siblings);
    }
    return byParent;
  }

  function findComment(thread, id) {
    return (thread.comments || []).find((comment) => comment.id === id) || null;
  }

  async function focusAnchor(threadId, anchor, node) {
    document.querySelectorAll('.anchor-target').forEach((item) => item.classList.remove('anchor-target'));
    if (node) node.classList.add('anchor-target');
    const key = `${threadId}:${anchor}`;
    if (lastSavedPosition === key) return;
    lastSavedPosition = key;
    const tab = preferences.postTabs.find((item) => item.id === threadId);
    if (tab) { tab.anchor = anchor; savePreferences(); }
    try {
      await invoke('save_position', { id: threadId, anchor });
    } catch (error) {
      lastSavedPosition = null;
      toast(`Could not save reading position: ${explainError(error)}`);
    }
  }

  function renderComment(thread, comment, byParent, depth, flat = false, ancestors = new Set()) {
    const section = element('section', `comment depth-${Math.min(depth, 5)}`);
    section.dataset.commentId = comment.id;
    section.tabIndex = 0;
    section.setAttribute('aria-label', `Comment ${comment.id}`);
    const card = element('div', 'comment-card');
    const head = element('div', 'comment-head');
    head.append(element('strong', '', comment.author || 'Comment'), element('span', '', comment.score === undefined ? '' : `${comment.score} points`));
    const content = element('div', 'comment-content');
    const body = element('p', 'comment-body');
    appendLinkedText(body, comment.body);
    content.append(body);
    const actions = element('div', 'comment-actions');
    const children = flat ? [] : (byParent.get(comment.id) || []);
    const collapse = element('button', '', children.length ? `− hide ${children.length} ${children.length === 1 ? 'reply' : 'replies'}` : 'no replies');
    collapse.type = 'button';
    collapse.dataset.testid = `collapse-${comment.id}`;
    collapse.disabled = children.length === 0;
    if (children.length) {
      collapse.setAttribute('aria-expanded', 'true');
      collapse.addEventListener('click', (event) => {
        event.stopPropagation();
        const isExpanded = collapse.getAttribute('aria-expanded') === 'true';
        collapse.setAttribute('aria-expanded', String(!isExpanded));
        childList.hidden = isExpanded;
        collapse.textContent = isExpanded ? `+ show ${children.length} ${children.length === 1 ? 'reply' : 'replies'}` : `− hide ${children.length} ${children.length === 1 ? 'reply' : 'replies'}`;
      });
    }
    actions.append(collapse);
    if (!flat) {
      const parentButton = element('button', '', 'parent');
      parentButton.type = 'button';
      parentButton.dataset.testid = `parent-${comment.id}`;
      parentButton.addEventListener('click', (event) => {
        event.stopPropagation();
        const current = section.querySelector(':scope > .comment-card > .parent-peek');
        if (current) { current.remove(); return; }
        const parentComment = findComment(thread, comment.parent_id);
        const previewText = parentComment ? parentComment.body : thread.body;
        const peek = element('div', 'parent-peek');
        appendLinkedText(peek, previewText || '(empty parent text)');
        peek.setAttribute('aria-label', 'Parent comment preview');
        head.after(peek);
      });
      actions.append(parentButton);
    }
    content.append(actions);
    const childList = element('div', 'children');
    if (!flat && !ancestors.has(comment.id)) {
      const nextAncestors = new Set(ancestors);
      nextAncestors.add(comment.id);
      for (const child of children) childList.append(renderComment(thread, child, byParent, depth + 1, false, nextAncestors));
    }
    card.append(head, content);
    section.append(card, childList);
    section.addEventListener('click', (event) => {
      if (event.target.closest('button') || event.target.closest('.comment') !== section) return;
      focusAnchor(thread.id, comment.id, section);
    });
    section.addEventListener('focusin', (event) => {
      if (event.target === section) focusAnchor(thread.id, comment.id, section);
    });
    return section;
  }

  function currentThread() {
    return activeId ? threadCache.get(activeId) || (snapshot && (snapshot.threads || []).find((thread) => thread.id === activeId)) : null;
  }

  function localAssetUrl(value) {
    try {
      const url = new URL(value);
      if (url.protocol === 'asset:' && url.hostname === 'localhost' && !url.username && !url.password) return url.href;
      if (url.protocol === 'http:' && url.hostname === '127.0.0.1' && url.port &&
          Number(url.port) >= 1 && Number(url.port) <= 65535 && !url.username && !url.password &&
          !url.search && !url.hash && /^\/[0-9a-f]{64}\/t3_[A-Za-z0-9_]+\/[A-Za-z0-9_-][A-Za-z0-9._-]*$/.test(url.pathname)) return url.href;
      if ((url.protocol === 'http:' || url.protocol === 'https:') && url.hostname === 'asset.localhost' && !url.port && !url.username && !url.password) return url.href;
    } catch (_) { /* Reject malformed and non-local asset URLs. */ }
    return null;
  }

  function renderMedia(threadId, options = {}) {
    const assets = mediaByThread.get(threadId) || [];
    const thread = threadCache.get(threadId) || (snapshot && (snapshot.threads || []).find((item) => item.id === threadId));
    const sourceUrl = thread && isExternalWebLink(thread.link_url) ? thread.link_url : '';
    const section = element('section', 'media-section');
    let content = section;
    if (options.collapsible !== false) {
      const details = element('details', 'media-details');
      details.open = preferences.mediaPresentation === 'inline';
      details.append(element('summary', '', `Local media · ${assets.length}`));
      section.append(details);
      content = details;
    } else if (!options.inline) {
      section.append(element('h2', 'comments-header', `Local media · ${assets.length}`));
    }
    if (!assets.length) {
      content.append(element('p', 'muted comment-note', 'No media was found in this post.'));
      return section;
    }
    const grid = element('div', 'media-grid');
    for (const asset of assets) {
      const url = localAssetUrl(asset && asset.url);
      const kind = asset && asset.kind;
      const name = String((asset && asset.name) || 'Media').slice(0, 200);
      const singleInlineAsset = options.inline === true && assets.length === 1;
      const card = element('figure', options.inline ? 'media-card inline-media-card' : 'media-card');
      const failure = element('p', 'small muted media-error', 'This local media file could not be opened.');
      failure.hidden = true;
      retryMediaControl(threadId, failure);
      if (!singleInlineAsset) card.append(element('figcaption', '', name));
      if (!url) {
        card.append(element('p', 'small muted', 'Local media address is invalid or unsupported.'));
      } else if (kind === 'image') {
        const image = document.createElement('img');
        image.src = url;
        image.alt = singleInlineAsset ? 'Local image' : name;
        image.loading = 'lazy';
        image.addEventListener('error', () => { invalidateThreadMedia(threadId); failure.hidden = false; });
        card.append(image);
      } else if (kind === 'video' || kind === 'audio') {
        const player = document.createElement(kind);
        player.controls = true;
        player.preload = 'none';
        player.src = url;
        if (kind === 'video' && asset && typeof asset.poster === 'string') {
          const poster = localAssetUrl(asset.poster);
          if (poster) {
            player.poster = poster;
            const posterImage = new Image();
            posterImage.onload = () => {
              if (posterImage.naturalHeight > posterImage.naturalWidth) {
                player.classList.add('portrait-video');
                player.style.aspectRatio = `${posterImage.naturalWidth} / ${posterImage.naturalHeight}`;
              }
            };
            posterImage.src = poster;
          }
        }
        if (kind === 'video') {
          player.addEventListener('loadedmetadata', () => {
            if (player.videoHeight > player.videoWidth) {
              player.classList.add('portrait-video');
              player.style.aspectRatio = `${player.videoWidth} / ${player.videoHeight}`;
            }
          });
        }
        player.setAttribute('aria-label', singleInlineAsset ? `Local ${kind} player` : name);
        installPlaybackTelemetry(player);
        if (options.watch !== false) installMediaWatchTracking(player, threadId, name);
        player.addEventListener('error', () => { invalidateThreadMedia(threadId); failure.hidden = false; });
        card.append(player);
        if (kind === 'video' && /^t3_[A-Za-z0-9_]+$/.test(threadId)) {
          const savedPost = libraryState.items.find((item) => item.id === threadId);
          const kept = Boolean(savedPost && savedPost.keep_forever);
          const keepButton = element('button', 'button secondary keep-video-button', kept ? 'Kept forever' : 'Keep video');
          keepButton.type = 'button';
          keepButton.disabled = kept;
          keepButton.setAttribute('aria-label', kept ? 'Video and post kept forever' : 'Keep video and its post forever');
          const keepStatus = element('p', 'small muted keep-video-status');
          keepStatus.setAttribute('role', 'status');
          keepStatus.setAttribute('aria-live', 'polite');
          keepButton.addEventListener('click', async () => {
            const currentSaved = libraryState.items.find((item) => item.id === threadId);
            keepButton.disabled = true;
            keepStatus.textContent = 'Keeping video and post…';
            try {
              await runLibraryMutation('save_library_post', {
                id: threadId,
                collection: String(currentSaved && currentSaved.collection || 'Videos'),
                keep_forever: true,
              }, 'Video and post will be kept forever.');
              keepButton.textContent = 'Kept forever';
              keepButton.setAttribute('aria-label', 'Video and post kept forever');
              keepStatus.textContent = 'Video and its post are kept forever in the local library.';
            } catch (error) {
              keepButton.disabled = false;
              keepStatus.textContent = `Could not keep this video. ${explainError(error)}`;
            }
          });
          card.append(keepButton, keepStatus);
        }
      } else {
        card.append(element('p', 'small muted', 'Unsupported local media type.'));
      }
      if (sourceUrl && ['image', 'video', 'audio'].includes(kind)) {
        const openSource = element('button', 'link-button open-web-link', 'Open original link');
        openSource.type = 'button';
        openSource.setAttribute('aria-label', `Open original link for ${thread.title}`);
        openSource.addEventListener('click', () => void openWebLink(sourceUrl));
        card.append(openSource);
      }
      if (url && (kind === 'image' || kind === 'video' || kind === 'audio')) card.append(failure);
      grid.append(card);
    }
    content.append(grid);
    return section;
  }

  function openThread(thread, anchor = null) {
    ui.state.hidden = true;
    ui.view.hidden = false;
    releaseMediaWatchesWithin(ui.view);
    ui.view.replaceChildren();
    const topline = element('div', 'thread-topline');
    topline.append(element('span', 'subreddit', `r/${thread.subreddit}`));
    if (thread.flair) topline.append(flairButton(thread.flair));
    ui.view.append(topline, element('h1', '', thread.title));
    ui.view.append(renderReaderLibraryActions(thread));
    const body = element('p', 'thread-body');
    appendLinkedText(body, thread.body);
    if (thread.body) ui.view.append(body);
    const hint = mediaHintsByThread.get(thread.id);
    const articleLink = isExternalWebLink(thread.link_url) && !hint && !(mediaByThread.get(thread.id) || []).length;
    if (articleLink) {
      const articleSlot = element('div', 'reader-article-slot');
      articleSlot.append(renderLinkArticle(thread));
      ui.view.append(articleSlot);
      void loadArticle(thread);
    }
    const mediaSlot = element('div', 'reader-media-slot');
    ui.view.append(mediaSlot);
    refreshReaderMedia(thread.id);
    const comments = thread.comments || [];
    const isPartial = thread.comments_status === 'partial_browser' || thread.comments_status === 'partial_proxy';
    const header = element('div', 'comments-header', thread.comments_status === 'not_loaded' ? 'Comments' : `${comments.length} ${comments.length === 1 ? 'comment' : 'comments'}`);
    ui.view.append(header);
    if (isPartial) {
      header.title = 'Some additional replies may be unavailable.';
    }
    if (thread.comments_status === 'not_loaded') {
      ui.view.append(element('p', 'muted comment-note cached-listing-note', postLoadsInFlight.has(thread.id) ? 'Loading comments…' : 'Open this post to load its discussion.'));
    } else if (comments.length) {
      const list = element('div', 'comment-list');
      const byParent = makeCommentTree(thread);
      for (const root of (byParent.get(thread.id) || [])) list.append(renderComment(thread, root, byParent, 0));
      ui.view.append(list);
    } else {
      ui.view.append(element('p', 'muted comment-note', 'No comments yet.'));
    }
    if (anchor && anchor !== thread.id) requestAnimationFrame(() => {
      const target = ui.view.querySelector(`[data-comment-id="${CSS.escape(anchor)}"]`);
      if (target) {
        target.classList.add('anchor-target');
        target.scrollIntoView({ block: 'center', behavior: 'auto' });
        if (target.matches('.comment')) target.focus({ preventScroll: true });
      }
    });
  }

  function renderReaderLibraryActions(thread) {
    const wrap = element('div', 'reader-library-actions');
    const saved = libraryState.items.some((item) => item.id === thread.id && item.kind === 'post');
    const save = element('button', 'button primary save-post-button', saved ? 'Saved to Library' : 'Save post');
    save.id = 'save-post-button';
    save.type = 'button';
    save.setAttribute('aria-pressed', String(saved));
    save.addEventListener('click', () => { if (saved) post.open = !post.open; else void saveCurrentPost(); });
    wrap.append(save);
    const post = element('details', 'reader-save-panel');
    const summary = element('summary', '', 'Save options');
    post.append(summary);
    const postFields = element('div', 'reader-save-fields');
    const collectionLabel = element('label', '', 'Collection');
    const collection = element('input', 'select-control');
    collection.id = 'save-post-collection';
    collection.value = libraryState.items.find((item) => item.id === thread.id && item.kind === 'post')?.collection || 'Saved';
    collectionLabel.append(collection);
    const foreverLabel = element('label', 'checkbox-label');
    const forever = document.createElement('input');
    forever.id = 'save-post-forever';
    forever.type = 'checkbox';
    const existingPost = libraryState.items.find((item) => item.id === thread.id && item.kind === 'post');
    forever.checked = existingPost ? Boolean(existingPost.keep_forever) : true;
    foreverLabel.append(forever, document.createTextNode('Keep offline until I remove it'));
    const saveStatus = element('p', 'small muted');
    saveStatus.id = 'save-post-status';
    saveStatus.setAttribute('role', 'status');
    saveStatus.setAttribute('aria-live', 'polite');
    postFields.append(element('p', 'small muted', 'Saves this post, its comments and media for offline reading.'), collectionLabel, foreverLabel);
    const update = element('button', 'button secondary', 'Save with these options');
    update.type = 'button';
    update.addEventListener('click', () => void saveCurrentPost());
    postFields.append(update);
    if (saved) {
      const remove = element('button', 'link-button', 'Remove from Library');
      remove.type = 'button';
      remove.addEventListener('click', async () => { await removeLibraryItem(thread.id); if (activeId === thread.id) ui.view.querySelector('.reader-library-actions')?.replaceWith(renderReaderLibraryActions(thread)); });
      postFields.append(remove);
    }
    post.append(postFields);
    wrap.append(post);

    const saveLink = element('button', 'button secondary', 'Save article or link separately…');
    saveLink.id = 'reader-save-link';
    saveLink.type = 'button';
    saveLink.addEventListener('click', () => openLibraryLinkForm(thread));
    if (isExternalWebLink(thread.link_url)) postFields.append(saveLink);
    wrap.append(saveStatus);
    return wrap;
  }

  function refreshReaderMedia(id) {
    if (activeId !== id) return;
    const slot = ui.view.querySelector('.reader-media-slot');
    if (!slot) return;
    releaseMediaWatchesWithin(slot);
    slot.replaceChildren();
    if ((mediaByThread.get(id) || []).length) { slot.append(renderMedia(id, { collapsible: false, inline: true })); return; }
    const hint = mediaHintsByThread.get(id);
    if (!hint) return;
    const state = readerMediaStates.get(id);
    const label = { image: 'image', video: 'video', audio: 'audio' }[hint.kind] || 'media';
    const panel = element('div', 'reader-media-pending');
    const button = element('button', 'button secondary', state?.loading ? `Loading ${label}…` : `Load ${label}`);
    button.type = 'button';
    button.disabled = Boolean(state?.loading);
    button.addEventListener('click', () => void loadReaderMedia(id));
    panel.append(button);
    if (state?.error) panel.append(element('p', 'small error-text', state.error));
    slot.append(panel);
  }

  async function loadReaderMedia(id) {
    if (readerMediaStates.get(id)?.loading) return;
    readerMediaStates.set(id, { loading: true });
    refreshReaderMedia(id);
    try {
      await getThreadDownload(id);
      readerMediaStates.delete(id);
    } catch (error) { readerMediaStates.set(id, { error: `Could not load media. ${explainError(error)}` }); }
    refreshReaderMedia(id);
  }

  function renderPostTabs() {
    ui.postTabs.replaceChildren();
    ui.postTabs.hidden = !browseDraftOpen && preferences.postTabs.length === 0;
    for (const tab of preferences.postTabs) {
      const thread = threadCache.get(tab.id);
      const title = thread ? thread.title : tab.id;
      const unavailable = unavailableThreadIds.has(tab.id);
      const item = element('div', 'post-tab');
      const selected = !browseDraftOpen && tab.id === activeId;
      item.classList.toggle('active', selected);
      item.setAttribute('role', 'presentation');
      const open = element('button', `post-tab-open${unavailable ? ' unavailable' : ''}`, unavailable ? `${title} · not cached` : title);
      open.type = 'button';
      open.setAttribute('role', 'tab');
      open.setAttribute('aria-selected', String(selected));
      open.tabIndex = selected ? 0 : -1;
      open.setAttribute('aria-label', `Open ${title}${thread ? ` from r/${thread.subreddit}` : ''}`);
      open.addEventListener('click', () => void selectThread(tab.id, tab.anchor || tab.id, true, 'restore'));
      open.addEventListener('keydown', (event) => {
        const tabs = [...ui.postTabs.querySelectorAll('[role="tab"]')];
        const index = tabs.indexOf(open);
        const next = event.key === 'ArrowRight' ? Math.min(tabs.length - 1, index + 1)
          : event.key === 'ArrowLeft' ? Math.max(0, index - 1)
            : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : -1;
        if (next < 0) return;
        event.preventDefault();
        tabs[next].focus();
        tabs[next].scrollIntoView({ block: 'nearest', inline: 'nearest' });
        tabs[next].click();
      });
      const close = element('button', 'post-tab-close', '×');
      close.type = 'button';
      close.setAttribute('aria-label', `Close ${title}`);
      close.addEventListener('click', (event) => { event.stopPropagation(); closePostTab(tab.id); });
      item.append(open, close);
      ui.postTabs.append(item);
    }
    if (browseDraftOpen) {
      const item = element('div', 'post-tab post-tab-draft active');
      item.setAttribute('role', 'presentation');
      const open = element('button', 'post-tab-open', 'New subreddit search');
      open.type = 'button';
      open.setAttribute('role', 'tab');
      open.setAttribute('aria-selected', 'true');
      open.setAttribute('aria-label', 'Open new subreddit search tab');
      open.tabIndex = 0;
      open.addEventListener('click', openBrowseDraftTab);
      open.addEventListener('keydown', (event) => {
        const tabs = [...ui.postTabs.querySelectorAll('[role="tab"]')];
        const index = tabs.indexOf(open);
        const next = event.key === 'ArrowRight' ? Math.min(tabs.length - 1, index + 1)
          : event.key === 'ArrowLeft' ? Math.max(0, index - 1)
            : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : -1;
        if (next < 0) return;
        event.preventDefault();
        tabs[next].focus();
        tabs[next].scrollIntoView({ block: 'nearest', inline: 'nearest' });
        tabs[next].click();
      });
      const close = element('button', 'post-tab-close', '×');
      close.type = 'button';
      close.setAttribute('aria-label', 'Close new subreddit search tab');
      close.addEventListener('click', closeBrowseDraftTab);
      item.append(open, close);
      ui.postTabs.append(item);
    }
  }

  function openBrowseDraftTab() {
    browseDraftOpen = true;
    setTab('browse');
    renderPostTabs();
    ui.input.focus();
    ui.input.select();
  }

  function closeBrowseDraftTab() {
    if (!browseDraftOpen) return;
    browseDraftOpen = false;
    renderPostTabs();
    setTab('browse');
    ui.newBrowseTab.focus();
  }

  function dispatchTabShortcut(action, origin) {
    const now = performance.now();
    if (action === lastTabShortcut.action && origin !== lastTabShortcut.origin && now - lastTabShortcut.at < 150) return;
    lastTabShortcut = { action, origin, at: now };
    if (action === 'new') openBrowseDraftTab();
    else if (action === 'close') handleCloseActivePostTab();
  }

  function ensurePostTab(thread, anchor, { cachedOnly = false, activate = true } = {}) {
    let tab = preferences.postTabs.find((item) => item.id === thread.id);
    if (!tab) {
      tab = { id: thread.id, anchor: anchor || thread.id, cachedOnly };
      preferences.postTabs.push(tab);
    } else {
      tab.anchor = anchor || tab.anchor || thread.id;
      tab.cachedOnly = cachedOnly;
    }
    if (activate) preferences.activePostId = thread.id;
    savePreferences();
    renderPostTabs();
  }

  function closePostTab(id) {
    const index = preferences.postTabs.findIndex((tab) => tab.id === id);
    if (index < 0) return;
    preferences.postTabs.splice(index, 1);
    const wasActive = activeId === id;
    operationSequence++;
    pendingId = null;
    if (wasActive) {
      activeId = null;
      const next = preferences.postTabs[Math.min(index, preferences.postTabs.length - 1)];
      preferences.activePostId = next ? next.id : null;
      pendingAnchor = null;
    } else if (preferences.activePostId === id) {
      preferences.activePostId = activeId;
    }
    savePreferences();
    renderPostTabs();
    if (wasActive) {
      if (preferences.activePostId) {
        const next = preferences.postTabs.find((tab) => tab.id === preferences.activePostId);
        if (next) void selectThread(next.id, next.anchor || next.id, true, 'restore');
      } else {
        setReaderMessage('No post tabs are open. Choose a post from the Subreddits & posts tab.');
        setTab('browse');
      }
    }
  }

  function acceptThreadDownload(result, id, { allowNotLoaded = false, updateListings = true, fetchPreviews = true } = {}) {
    const thread = validateThreadDownload(result, id, { allowNotLoaded });
    const mergedThreads = new Map((snapshot && Array.isArray(snapshot.threads) ? snapshot.threads : []).map((item) => [item.id, item]));
    for (const returned of result.snapshot.threads) mergedThreads.set(returned.id, returned);
    snapshot = mergeSnapshot(result.snapshot, { threads: [...mergedThreads.values()] });
    threadCache.set(id, thread);
    loadedThreadIds.add(id);
    unavailableThreadIds.delete(id);
    if (updateListings) {
      for (const [subreddit, listing] of listingBySubreddit) {
        listingBySubreddit.set(subreddit, listing.map((item) => item.id === id ? thread : item));
      }
    }
    lastSavedPosition = snapshot.position ? `${snapshot.position[0]}:${snapshot.position[1]}` : null;
    refreshDiagnostics(snapshot);
    renderPostTabs();
    if (fetchPreviews) requestLinkPreviews(result.snapshot.threads);
    return thread;
  }

  function mergeSnapshot(incoming, overrides = {}) {
    const { ...snapshotOverrides } = overrides;
    const mergedHints = new Map(mediaHintsByThread);
    for (const thread of Array.isArray(incoming && incoming.threads) ? incoming.threads : []) mergedHints.delete(thread.id);
    const incomingHints = incoming && incoming.media_hints && typeof incoming.media_hints === 'object' ? incoming.media_hints : {};
    for (const [id, hint] of Object.entries(incomingHints)) {
      if (hint && ['video', 'image', 'audio', 'media'].includes(hint.kind) && Number.isFinite(Number(hint.count))) {
        mergedHints.set(id, { kind: hint.kind, count: Math.max(0, Number(hint.count)) });
      }
    }
    mediaHintsByThread.clear();
    for (const [id, hint] of mergedHints) mediaHintsByThread.set(id, hint);
    return { ...(snapshot || {}), ...incoming, ...snapshotOverrides, media_hints: Object.fromEntries(mediaHintsByThread) };
  }

  function isSubredditListingThread(thread) {
    return Boolean(thread && !['linked', 'linked_bookmark'].includes(thread.source));
  }

  function draw() {
    if (!snapshot || !Array.isArray(snapshot.threads)) {
      setReaderMessage('The native cache returned an invalid snapshot.', true);
      return;
    }
    renderList();
    renderSubreddits();
    renderPostTabs();
    refreshDiagnostics(snapshot);
    if (activeId && unavailableThreadIds.has(activeId)) {
      setReaderMessage('This post expired from the local cache. Fetch its subreddit again to read it.');
      return;
    }
    const selected = currentThread();
    if (selected && loadedThreadIds.has(activeId)) {
      const anchor = pendingAnchor;
      pendingAnchor = null;
      openThread(selected, anchor);
      return;
    }
    const threads = redditSearch ? redditSearch.threads : (listingBySubreddit.get(activeSubreddit) || []);
    if (!threads.length) {
      if (!activeId) setReaderMessage('No source listing is loaded. Fetch a subreddit above.');
      return;
    }
    if (!activeId || !selected) {
      setReaderMessage('Choose a post to fetch its comments and media.');
      return;
    }
    setReaderMessage('Fetching this post’s comments and media…');
  }

  function rememberSnapshotThreads(result) {
    for (const thread of result.threads) {
      if (!loadedThreadIds.has(thread.id)) threadCache.set(thread.id, thread);
    }
    for (const [subreddit, listing] of listingBySubreddit) {
      listingBySubreddit.set(subreddit, listing.map((thread) => threadCache.get(thread.id) || thread));
    }
  }

  function persistListingIds() {
    preferences.listings = Object.fromEntries([...listingBySubreddit.entries()].map(([subreddit, threads]) => [subreddit, threads.map((thread) => thread.id)]));
    savePreferences();
  }

  function installSourceListing(subreddit, threads) {
    const sourceListing = threads.filter(isSubredditListingThread).map((thread) => {
      unavailableThreadIds.delete(thread.id);
      if (!threadCache.has(thread.id)) threadCache.set(thread.id, thread);
      return thread;
    });
    listingBySubreddit.set(subreddit, sourceListing);
    preferences.lastSubreddit = subreddit;
    persistListingIds();
  }

  function mergeImportedListings(threads) {
    const groups = new Map();
    for (const thread of threads.filter(isSubredditListingThread)) {
      const group = groups.get(thread.subreddit) || [];
      group.push(thread);
      groups.set(thread.subreddit, group);
    }
    for (const [subreddit, incoming] of groups) {
      const listing = listingBySubreddit.get(subreddit) || [];
      const known = new Set(listing.map((thread) => thread.id));
      listingBySubreddit.set(subreddit, listing.concat(incoming.filter((thread) => !known.has(thread.id))));
    }
    if (!listingBySubreddit.has(activeSubreddit) && groups.size) {
      activeSubreddit = groups.keys().next().value;
      preferences.lastSubreddit = activeSubreddit;
      ui.input.value = `/r/${activeSubreddit}`;
    }
    persistListingIds();
  }

  function hydrateListings(result) {
    rememberSnapshotThreads(result);
    let droppedLinkedIds = false;
    for (const [subreddit, storedIds] of Object.entries(preferences.listings)) {
      const ids = storedIds.filter((id) => {
        const thread = threadCache.get(id);
        return !thread || isSubredditListingThread(thread);
      });
      if (ids.length !== storedIds.length) {
        preferences.listings[subreddit] = ids;
        droppedLinkedIds = true;
      }
      const listing = ids.map((id) => threadCache.get(id)).filter(isSubredditListingThread);
      if (listing.length === ids.length && listing.length) listingBySubreddit.set(subreddit, listing);
    }
    if (!listingBySubreddit.has(activeSubreddit)) {
      const browseableThreads = result.threads.filter(isSubredditListingThread);
      const fallback = browseableThreads.filter((thread) => thread.subreddit === activeSubreddit);
      if (fallback.length && !preferences.listings[activeSubreddit]) listingBySubreddit.set(activeSubreddit, fallback);
      else if (browseableThreads.length) {
        const fallbackName = browseableThreads[0].subreddit;
        const fallbackIds = preferences.listings[fallbackName];
        const fallbackThreads = fallbackIds ? fallbackIds.map((id) => threadCache.get(id)).filter(isSubredditListingThread) : browseableThreads.filter((thread) => thread.subreddit === fallbackName);
        if (fallbackThreads.length && (!fallbackIds || fallbackThreads.length === fallbackIds.length)) {
          activeSubreddit = fallbackName;
          listingBySubreddit.set(activeSubreddit, fallbackThreads);
          preferences.lastSubreddit = activeSubreddit;
          ui.input.value = `/r/${activeSubreddit}`;
        }
      }
    }
    if (droppedLinkedIds) savePreferences();
  }

  async function receiveSnapshot(result, selectedId = null, selectedAnchor = null) {
    if (!result || !Array.isArray(result.threads) || !result.diagnostics) {
      throw new Error('Native command returned an invalid Snapshot.');
    }
    snapshot = mergeSnapshot(result);
    for (const thread of result.threads) unavailableThreadIds.delete(thread.id);
    rememberSnapshotThreads(result);
    if (listingBySubreddit.size === 0) {
      hydrateListings(result);
    }
    activeId = selectedId && loadedThreadIds.has(selectedId) ? selectedId : null;
    pendingAnchor = selectedAnchor || null;
    lastSavedPosition = result.position ? `${result.position[0]}:${result.position[1]}` : null;
    renderPostTabs();
    draw();
    requestLinkPreviews(snapshot.threads);
  }

  function validateThreadDownload(result, id, { allowNotLoaded = false } = {}) {
    if (!result || !result.snapshot || !Array.isArray(result.media)) {
      throw new Error('Native command returned an invalid ThreadDownload.');
    }
    if (!Array.isArray(result.snapshot.threads) || !result.snapshot.threads.some((thread) => thread.id === id)) {
      throw new Error('ThreadDownload did not include the requested post.');
    }
    const thread = result.snapshot.threads.find((item) => item.id === id);
    if (thread.comments_status === 'not_loaded' && !allowNotLoaded) throw new Error('The post comments were not fetched.');
    if (!result.snapshot.diagnostics) throw new Error('ThreadDownload snapshot diagnostics are missing.');
    return thread;
  }

  function getThreadDownload(id) {
    let pending = threadDownloadsInFlight.get(id);
    if (!pending) {
      const memoryReady = loadedThreadIds.has(id) && mediaByThread.has(id) && threadCache.has(id) && threadCache.get(id).comments_status !== 'not_loaded';
      const fetch = () => invoke('fetch_thread', { id }).then((result) => {
        const thread = acceptThreadDownload(result, id);
        const media = result.media;
        mediaByThread.set(id, media);
        const cached = result.cache?.comments_hit === true && result.cache?.media_hit === true;
        return { cached, thread, media };
      });
      pending = (memoryReady
        ? invoke('validate_cached_media', { id }).then((valid) => {
          if (typeof valid !== 'boolean') throw new Error('Native cache validation returned an invalid result.');
          if (valid && loadedThreadIds.has(id) && mediaByThread.has(id) && threadCache.has(id)) {
            return { cached: true, thread: threadCache.get(id), media: mediaByThread.get(id) };
          }
          invalidateThreadMedia(id);
          return fetch();
        })
        : fetch()
      ).finally(() => threadDownloadsInFlight.delete(id));
      threadDownloadsInFlight.set(id, pending);
    }
    return pending;
  }

  function listedMediaCandidates() {
    const threads = redditSearch ? redditSearch.threads : (listingBySubreddit.get(activeSubreddit) || []);
    return threads.filter((thread) => {
      const hint = mediaHintsByThread.get(thread.id);
      const assets = mediaByThread.get(thread.id);
      return Boolean((hint && Number(hint.count) > 0) || (assets && assets.length));
    });
  }

  function updateFetchAllMediaControl() {
    const candidates = listedMediaCandidates();
    ui.fetchAllMedia.disabled = ui.list.inert || (!bulkMediaRun && candidates.length === 0);
    ui.fetchAllMedia.textContent = bulkMediaRun ? 'Cancel fetch all' : 'Fetch all media';
    ui.fetchAllMedia.setAttribute('aria-label', bulkMediaRun ? 'Cancel fetching media for listed posts' : `Fetch media for ${candidates.length} listed posts`);
  }

  async function loadInlineMedia(id) {
    const inline = ui.list.querySelector(`[data-inline-media-for="${CSS.escape(id)}"]`);
    if (!inline) return false;
    const existingMedia = inline.querySelector('.media-section');
    const progress = element('p', 'muted small inline-media-progress', existingMedia ? 'Checking cached post and media…' : 'Opening post and media…');
    if (existingMedia) inline.append(progress);
    else inline.replaceChildren(progress);
    const startedAt = performance.now();
    try {
      const download = await getThreadDownload(id);
      const current = ui.list.querySelector(`[data-inline-media-for="${CSS.escape(id)}"]`);
      if (!current || !inlineExpanded.has(id)) return true;
      if (existingMedia && download.cached && existingMedia.isConnected) {
        progress.remove();
        updateFetchAllMediaControl();
        recordClientEvent('inline_media', performance.now() - startedAt, download.media.length);
        return true;
      }
      current.replaceChildren(renderMedia(id, { collapsible: false, inline: true }));
      updateFetchAllMediaControl();
      recordClientEvent('inline_media', performance.now() - startedAt, download.media.length);
      return true;
    } catch (error) {
      recordClientEvent('inline_media', performance.now() - startedAt, 0);
      const current = ui.list.querySelector(`[data-inline-media-for="${CSS.escape(id)}"]`);
      if (!current) return;
      const message = element('p', 'small error-text', `Could not load local media. ${explainError(error)} `);
      const retry = element('button', 'link-button', 'Retry');
      retry.type = 'button';
      retry.addEventListener('click', () => void loadInlineMedia(id));
      message.append(retry);
      current.replaceChildren(message);
      return false;
    }
  }

  async function fetchAllListedMedia() {
    if (bulkMediaRun) {
      bulkMediaRun.cancelled = true;
      ui.fetchAllMediaStatus.textContent = 'Cancelling after current downloads…';
      updateFetchAllMediaControl();
      return;
    }
    const candidates = listedMediaCandidates();
    if (!candidates.length) return;
    const run = { cancelled: false, completed: 0, failed: 0, total: candidates.length, subreddit: activeSubreddit };
    bulkMediaRun = run;
    let active = 0;
    const announce = () => {
      const phase = run.cancelled ? 'Cancelling' : 'Fetching';
      ui.fetchAllMediaStatus.textContent = `${phase} ${run.completed} of ${run.total} · ${active} active${run.failed ? ` · ${run.failed} failed` : ''}`;
    };
    updateFetchAllMediaControl();
    announce();
    let cursor = 0;
    const worker = async () => {
      while (!run.cancelled) {
        const index = cursor++;
        if (index >= candidates.length) return;
        const thread = candidates[index];
        const id = thread.id;
        inlineExpanded.add(id);
        const card = ui.list.querySelector(`[data-thread-id="${CSS.escape(id)}"]`);
        if (card) {
          const region = card.querySelector('.inline-media');
          const button = card.querySelector('.inline-media-toggle');
          if (region) region.hidden = false;
          if (button) { button.textContent = 'Hide media'; button.setAttribute('aria-expanded', 'true'); }
        }
        active++;
        announce();
        const succeeded = await loadInlineMedia(id);
        if (!succeeded && !run.cancelled && run.subreddit === activeSubreddit) run.failed++;
        active--;
        run.completed++;
        announce();
      }
    };
    try {
      await Promise.all([worker(), worker()]);
      ui.fetchAllMediaStatus.textContent = run.cancelled
        ? `Cancelled · ${run.completed} of ${run.total} processed${run.failed ? ` · ${run.failed} failed` : ''}`
        : `Finished · ${run.completed} posts${run.failed ? ` · ${run.failed} failed` : ''}`;
    } finally {
      if (bulkMediaRun === run) bulkMediaRun = null;
      updateFetchAllMediaControl();
    }
  }

  async function toggleInlineMedia(id, card) {
    const wasExpanded = inlineExpanded.has(id);
    if (wasExpanded) {
      inlineExpanded.delete(id);
      const region = card.querySelector('.inline-media');
      if (region) region.hidden = true;
      const button = card.querySelector('.inline-media-toggle');
      if (button) { button.textContent = 'Expand media'; button.setAttribute('aria-expanded', 'false'); }
      return;
    }
    inlineExpanded.add(id);
    const region = card.querySelector('.inline-media');
    const button = card.querySelector('.inline-media-toggle');
    if (region) region.hidden = false;
    if (button) { button.textContent = 'Hide media'; button.setAttribute('aria-expanded', 'true'); }
    await loadInlineMedia(id);
  }

  function normalizeSubreddit(value) {
    const text = String(value || '').trim();
    const match = text.match(/^(?:https?:\/\/(?:www\.)?reddit\.com)?\/?r\/([a-z0-9_]{2,21})\/?$/i);
    const plain = text.match(/^([a-z0-9_]{2,21})$/i);
    const name = (match && match[1]) || (plain && plain[1]);
    if (!name) throw new Error('Enter a subreddit such as /r/soccer.');
    return name.toLowerCase();
  }

  async function browseSubreddit(rawValue = ui.input.value, { authoritative = false } = {}) {
    let subreddit;
    try { subreddit = normalizeSubreddit(rawValue); }
    catch (error) { setFeedback(explainError(error), true); ui.input.focus(); return; }
    ui.input.value = `/r/${subreddit}`;
    redditSearch = null;
    cancelRedditSearch();
    ui.cacheSearchSubmit.disabled = false;
    window.clearTimeout(cacheSearchTimer);
    cacheSearchSequence++;
    ui.cacheSearch.value = '';
    ui.cacheSearchClear.disabled = true;
    ui.cacheSearchResults.hidden = true;
    updateSortNotice();
    const transition = ++operationSequence;
    const request = ++subredditRequestSequence;
    const startedAt = performance.now();
    pendingId = null;
    renderList();
    ui.list.inert = true;
    ui.list.setAttribute('aria-busy', 'true');
    if (bulkMediaRun) bulkMediaRun.cancelled = true;
    updateFetchAllMediaControl();
    ui.browse.disabled = true;
    ui.browseRefresh.disabled = true;
    ui.input.disabled = true;
    ui.postSort.disabled = true;
    ui.topPeriod.disabled = true;
    setFeedback(`Fetching posts from /r/${subreddit}…`);
    setStatus(`Fetching /r/${subreddit}…`);
    startOperationProgress('Fetching subreddit posts', transition);
    try {
      const result = await invoke('fetch_subreddit', { subreddit, sort: preferences.postSort, top_period: preferences.topPeriod });
      if (!authoritative && transition !== operationSequence) return;
      if (!result || !Array.isArray(result.threads) || !result.diagnostics) throw new Error('Native command returned an invalid Snapshot.');
      snapshot = mergeSnapshot(result);
      rememberSnapshotThreads(result);
      activeSubreddit = subreddit;
      preferences.lastSubreddit = subreddit;
      installSourceListing(subreddit, result.threads);
      recordRecent(subreddit);
      setTab('browse');
      refreshDiagnostics(snapshot);
      renderList();
      renderPostTabs();
      requestLinkPreviews(result.threads);
      const elapsed = performance.now() - startedAt;
      recordClientEvent('source_fetch', elapsed, result.threads.length);
      setFeedback('');
      setStatus(`/r/${subreddit} · ${result.threads.length} posts · ${formatElapsed(elapsed)}`, 'ready');
    } catch (error) {
      if (authoritative || transition === operationSequence) {
        recordClientEvent('source_fetch', performance.now() - startedAt, 0);
        setFeedback(`Could not fetch /r/${subreddit}. Your current reader content is unchanged. ${explainError(error)}${sourceTrustFailureHint()}`, true, () => void browseSubreddit(`/r/${subreddit}`));
        setStatus(`Fetch failed · /r/${subreddit}`, 'error');
      }
    } finally {
      stopOperationProgress(transition);
      if (authoritative || request === subredditRequestSequence) {
        ui.browse.disabled = false;
        ui.browseRefresh.disabled = false;
        ui.input.disabled = false;
        if (transition === operationSequence) {
          ui.postSort.disabled = false;
          ui.topPeriod.disabled = false;
          ui.list.inert = false;
          ui.list.removeAttribute('aria-busy');
          updateFetchAllMediaControl();
        }
      }
    }
  }

  async function searchReddit(query = ui.cacheSearch.value.trim(), sort = ui.postSort.value, period = ui.topPeriod.value) {
    const normalizedQuery = String(query || '').trim();
    if (!normalizedQuery) {
      setFeedback('Enter a search term to search Reddit.', true);
      ui.cacheSearch.focus();
      return;
    }
    const validSort = ['relevance', 'new', 'top', 'comments'].includes(sort) ? sort : 'relevance';
    const validPeriod = ['hour', 'day', 'week', 'month', 'year', 'all'].includes(period) ? period : 'all';
    cancelCacheSearch({ hide: true });
    const transition = ++operationSequence;
    const request = ++redditSearchSequence;
    const operation = { request, transition };
    activeRedditSearchOperation = operation;
    const startedAt = performance.now();
    if (bulkMediaRun) {
      bulkMediaRun.cancelled = true;
      ui.fetchAllMediaStatus.textContent = 'Cancelling after current downloads…';
    }
    ui.cacheSearchResults.hidden = true;
    ui.cacheSearchSubmit.disabled = true;
    ui.postSort.disabled = true;
    ui.topPeriod.disabled = true;
    ui.list.inert = true;
    ui.list.setAttribute('aria-busy', 'true');
    updateFetchAllMediaControl();
    setFeedback(`Searching Reddit for “${normalizedQuery}”…`);
    setStatus(`Searching Reddit · ${normalizedQuery}`);
    startOperationProgress('Searching Reddit', transition);
    try {
      const result = await invoke('search_reddit', {
        query: normalizedQuery,
        subreddit: null,
        sort: validSort,
        period: validPeriod,
      });
      if (request !== redditSearchSequence || transition !== operationSequence) return;
      if (!result || !Array.isArray(result.threads) || !result.diagnostics) throw new Error('Native Reddit search returned an invalid Snapshot.');
      snapshot = mergeSnapshot(result);
      for (const thread of result.threads) unavailableThreadIds.delete(thread.id);
      rememberSnapshotThreads(result);
      redditSearch = { query: normalizedQuery, sort: validSort, period: validPeriod, threads: result.threads.slice() };
      pendingId = null;
      setTab('browse');
      updateSortNotice();
      refreshDiagnostics(snapshot);
      renderList();
      renderPostTabs();
      requestLinkPreviews(result.threads);
      const elapsed = performance.now() - startedAt;
      recordClientEvent('source_fetch', elapsed, result.threads.length);
      setFeedback('');
      setStatus(`Reddit results · ${result.threads.length} posts · ${formatElapsed(elapsed)}`, 'ready');
    } catch (error) {
      if (request === redditSearchSequence && transition === operationSequence) {
        recordClientEvent('source_fetch', performance.now() - startedAt, 0);
        setFeedback(`Could not search Reddit for “${normalizedQuery}”. ${explainError(error)}${sourceTrustFailureHint()}`, true, () => void searchReddit(normalizedQuery, validSort, validPeriod));
        setStatus('Reddit search failed', 'error');
      }
    } finally {
      stopOperationProgress(transition);
      if (activeRedditSearchOperation === operation) {
        activeRedditSearchOperation = null;
        if (request === redditSearchSequence) {
          ui.cacheSearchSubmit.disabled = false;
          ui.postSort.disabled = false;
          ui.topPeriod.disabled = false;
          ui.list.inert = false;
          ui.list.removeAttribute('aria-busy');
          updateFetchAllMediaControl();
        }
      }
    }
  }

  function cancelRedditSearch({ clearFeedback = false } = {}) {
    redditSearchSequence++;
    const operation = activeRedditSearchOperation;
    activeRedditSearchOperation = null;
    if (!operation) return;
    const ownedCurrentStatus = operation.transition === operationSequence;
    operationSequence++;
    stopOperationProgress(operation.transition);
    ui.cacheSearchSubmit.disabled = false;
    ui.postSort.disabled = false;
    ui.topPeriod.disabled = false;
    ui.list.inert = false;
    ui.list.removeAttribute('aria-busy');
    updateFetchAllMediaControl();
    if (clearFeedback && ownedCurrentStatus) {
      setFeedback('');
      setStatus('Ready', 'ready');
    }
  }

  function renderCacheSearchResults(rows, query) {
    ui.cacheSearchResults.replaceChildren();
    if (!query) {
      ui.cacheSearchResults.hidden = true;
      return;
    }
    if (!rows.length) {
      ui.cacheSearchResults.append(element('p', 'cache-search-empty muted', `No cached matches for “${query}”. Try fewer words or a subreddit name.`));
      ui.cacheSearchResults.hidden = false;
      return;
    }
    for (const row of rows.slice(0, 50)) {
      if (!row || typeof row.id !== 'string' || !/^t3_[A-Za-z0-9_]+$/.test(row.id)) continue;
      const hit = element('button', 'cache-search-hit');
      hit.type = 'button';
      hit.dataset.threadId = row.id;
      if (typeof row.comment_id === 'string' && /^t1_[A-Za-z0-9_]+$/.test(row.comment_id)) hit.dataset.commentId = row.comment_id;
      hit.setAttribute('role', 'option');
      const title = element('span', 'cache-search-title', typeof row.title === 'string' ? row.title : row.id);
      const metadata = element('span', 'cache-search-meta');
      metadata.append(element('span', 'cache-search-subreddit', `r/${typeof row.subreddit === 'string' ? row.subreddit : 'unknown'}`));
      if (row.comment_id) metadata.append(element('span', 'cache-search-comment-hit', 'Comment match'));
      hit.append(title, metadata);
      if (typeof row.snippet === 'string' && row.snippet) hit.append(element('span', 'cache-search-snippet', row.snippet.slice(0, 320)));
      hit.addEventListener('click', () => void openCachedSearchHit({
        id: row.id,
        comment_id: typeof row.comment_id === 'string' ? row.comment_id : null,
      }));
      hit.addEventListener('keydown', (event) => {
        if (event.key !== 'Enter') return;
        event.preventDefault();
        event.stopPropagation();
        hit.click();
      });
      ui.cacheSearchResults.append(hit);
    }
    if (!ui.cacheSearchResults.childElementCount) ui.cacheSearchResults.append(element('p', 'cache-search-empty muted', 'No cached matches.'));
    ui.cacheSearchResults.hidden = false;
  }

  function cancelCacheSearch({ hide = false } = {}) {
    window.clearTimeout(cacheSearchTimer);
    cacheSearchSequence++;
    if (hide) {
      ui.cacheSearchResults.replaceChildren();
      ui.cacheSearchResults.hidden = true;
    }
  }

  function clearSearch({ focus = false } = {}) {
    cancelCacheSearch({ hide: true });
    ui.cacheSearch.value = '';
    ui.cacheSearchClear.disabled = true;
    cancelRedditSearch();
    redditSearch = null;
    setFeedback('');
    renderList();
    setStatus('Search cleared', 'ready');
    if (focus) ui.cacheSearch.focus();
  }

  function scheduleCacheSearch(delay = 0) {
    const query = ui.cacheSearch.value.trim();
    const request = ++cacheSearchSequence;
    window.clearTimeout(cacheSearchTimer);
    ui.cacheSearchClear.disabled = !ui.cacheSearch.value;
    if (!query) {
      ui.cacheSearchResults.replaceChildren();
      ui.cacheSearchResults.hidden = true;
      return;
    }
    cacheSearchTimer = window.setTimeout(async () => {
      ui.cacheSearchResults.replaceChildren(element('p', 'cache-search-empty muted', 'Searching this device’s cache…'));
      ui.cacheSearchResults.hidden = false;
      try {
        const rows = await invoke('search_cache', { query, subreddit: null, limit: 50 });
        if (request !== cacheSearchSequence || query !== ui.cacheSearch.value.trim()) return;
        if (!Array.isArray(rows)) throw new Error('Native cache search returned an invalid result.');
        renderCacheSearchResults(rows.slice(0, 50), query);
      } catch (error) {
        if (request !== cacheSearchSequence) return;
        ui.cacheSearchResults.replaceChildren(element('p', 'cache-search-error', `Cached search failed. ${explainError(error)}`));
        ui.cacheSearchResults.hidden = false;
      }
    }, delay);
  }

  async function loadPostCacheSettings() {
    if (cacheSettingsDirty) return;
    const request = ++cacheSettingsSequence;
    const editAtStart = cacheSettingsEditSequence;
    ui.cacheSettingsStatus.textContent = 'Loading saved post lifetime…';
    try {
      const result = await invoke('get_cache_settings');
      const hours = Number(result && result.post_cache_hours);
      if (!Number.isInteger(hours) || hours < 1 || hours > 8760) throw new Error('Native post lifetime setting is outside the allowed range.');
      if (request !== cacheSettingsSequence || editAtStart !== cacheSettingsEditSequence || cacheSettingsDirty) return;
      ui.postCacheHours.value = String(hours);
      ui.cacheSettingsStatus.textContent = `Saved · ${hours} hour${hours === 1 ? '' : 's'}.`;
    } catch (error) {
      if (request === cacheSettingsSequence && editAtStart === cacheSettingsEditSequence && !cacheSettingsDirty) {
        ui.cacheSettingsStatus.textContent = `Could not load the saved setting. ${explainError(error)}`;
      }
    }
  }

  function applyCacheMembershipSnapshot(result) {
    if (!result || !Array.isArray(result.threads) || !result.diagnostics) throw new Error('Native command returned an invalid Snapshot.');
    snapshot = mergeSnapshot(result);
    const available = new Set(result.threads.map((thread) => thread.id));
    for (const tab of preferences.postTabs) {
      if (available.has(tab.id)) unavailableThreadIds.delete(tab.id);
      else unavailableThreadIds.add(tab.id);
    }
    for (const [subreddit, listing] of listingBySubreddit) {
      listingBySubreddit.set(subreddit, listing.filter((thread) => available.has(thread.id)));
    }
    for (const subreddit of Object.keys(preferences.listings)) {
      preferences.listings[subreddit] = preferences.listings[subreddit].filter((id) => available.has(id));
    }
    savePreferences();
    renderPostTabs();
    draw();
    requestLinkPreviews(result.threads);
    return available;
  }

  async function refreshCacheMembership() {
    const result = await invoke('snapshot');
    return applyCacheMembershipSnapshot(result);
  }

  async function applyPostCacheSettings() {
    const hours = Number(ui.postCacheHours.value);
    if (!Number.isInteger(hours) || hours < 1 || hours > 8760) {
      ui.postCacheHours.setAttribute('aria-invalid', 'true');
      ui.cacheSettingsStatus.textContent = 'Enter a whole number from 1 to 8760 hours.';
      ui.postCacheHours.focus();
      return;
    }
    ui.postCacheHours.removeAttribute('aria-invalid');
    const editAtStart = cacheSettingsEditSequence;
    cacheSettingsSequence++;
    ui.postCacheApply.disabled = true;
    ui.cacheSettingsStatus.textContent = 'Saving post lifetime…';
    try {
      const result = await invoke('set_cache_settings', { post_cache_hours: hours });
      applyCacheMembershipSnapshot(result);
      cacheSettingsDirty = cacheSettingsEditSequence !== editAtStart;
      const currentExpired = Boolean(activeId && unavailableThreadIds.has(activeId));
      const savedStatus = `Saved · ${hours} hour${hours === 1 ? '' : 's'}.`;
      ui.cacheSettingsStatus.textContent = `${savedStatus}${currentExpired ? ' The open post expired from cache.' : ''}${cacheSettingsDirty ? ' The edited field has another unsaved value.' : ''}`;
      setStatus(currentExpired ? 'Post expired from cache' : 'Post cache lifetime updated', currentExpired ? 'error' : 'ready');
    } catch (error) {
      ui.cacheSettingsStatus.textContent = `Could not save the post lifetime. ${explainError(error)}`;
    } finally {
      ui.postCacheApply.disabled = false;
    }
  }

  async function selectThread(id, anchor = id, switchToReader = true, intent = 'normal') {
    if (unavailableThreadIds.has(id)) {
      if (preferences.postTabs.some((tab) => tab.id === id)) {
        activeId = id;
        preferences.activePostId = id;
        savePreferences();
        renderPostTabs();
      }
      setReaderMessage('This post expired from the local cache. Fetch its subreddit again to read it.');
      if (switchToReader) setTab('reader');
      return;
    }
    const selected = threadCache.get(id) || [...listingBySubreddit.values()].flat().find((thread) => thread.id === id) || (preferences.postTabs.some((tab) => tab.id === id) ? { id, title: id } : null);
    if (!selected) return;
    if (activeId && !ui.readerPanel.hidden) postScrollPositions.set(activeId, window.scrollY);
    const transition = ++operationSequence;
    const startedAt = performance.now();
    pendingId = id;
    const tab = preferences.postTabs.find((item) => item.id === id);
    const cachedOnly = Boolean(intent === 'restore' && tab && tab.cachedOnly);
    const pendingDownload = cachedOnly ? getCachedThreadDownload(id) : getPostDownload(id);
    if (tab && !cachedOnly) tab.cachedOnly = false;
    activeId = id;
    browseDraftOpen = false;
    loadedThreadIds.add(id);
    ensurePostTab(selected, anchor, { cachedOnly });
    pendingAnchor = anchor !== id ? anchor : null;
    draw();
    if (switchToReader) setTab('reader');
    if (!ui.readerPanel.hidden) window.scrollTo(0, postScrollPositions.get(id) || 0);
    setFeedback('');
    setStatus('Opening post…');
    try {
      const download = await pendingDownload;
      if (transition !== operationSequence) return;
      if (browseDraftOpen) browseDraftOpen = false;
      const downloadedThread = download.thread;
      pendingId = null;
      activeId = id;
      pendingAnchor = anchor && anchor !== id ? anchor : null;
      ensurePostTab(downloadedThread, anchor || id, { cachedOnly: Boolean(cachedOnly) });
      const renderStartedAt = performance.now();
      const readerScroll = window.scrollY;
      draw();
      requestAnimationFrame(() => { if (activeId === id) window.scrollTo(0, readerScroll); });
      recordClientEvent('reader_render', performance.now() - renderStartedAt, downloadedThread.comments.length + download.media.length);
      await focusAnchor(id, anchor, null);
      setFeedback('');
      const elapsed = performance.now() - startedAt;
      recordClientEvent('post_fetch', elapsed, downloadedThread.comments.length + download.media.length);
      setStatus('Ready', 'ready');
      if (!cachedOnly && !ui.readerPanel.hidden && mediaHintsByThread.has(id) && !(mediaByThread.get(id) || []).length) void loadReaderMedia(id);
    } catch (error) {
      if (transition !== operationSequence) return;
      recordClientEvent('post_fetch', performance.now() - startedAt, 0);
      pendingId = null;
      renderList();
      if (!currentThread()) setReaderMessage('This post could not be loaded.', true);
      setFeedback(`Could not load the discussion. ${explainError(error)}`, true, () => void selectThread(id, anchor, true, cachedOnly ? 'restore' : 'normal'));
      setStatus('Discussion unavailable', 'error');
    } finally {
      stopOperationProgress(transition);
    }
    const item = [...ui.list.querySelectorAll('.thread-item')].find((card) => card.getAttribute('aria-current') === 'true');
    if (item) keyboardThread = Number(item.dataset.threadIndex);
    const thread = currentThread();
    if (thread) ui.input.value = `/r/${thread.subreddit}`;
  }

  function getPostDownload(id) {
    let pending = postLoadsInFlight.get(id);
    if (!pending) {
      pending = invoke('fetch_thread', { id, include_media: false }).then((result) => {
        const thread = acceptThreadDownload(result, id, { allowNotLoaded: true });
        if (result.media.length || !mediaByThread.has(id)) mediaByThread.set(id, result.media);
        return { cached: result.cache?.comments_hit === true, thread, media: result.media };
      }).finally(() => postLoadsInFlight.delete(id));
      postLoadsInFlight.set(id, pending);
    }
    return pending;
  }

  function openPostInBackground(thread) {
    ensurePostTab(thread, thread.id, { activate: false });
    void getPostDownload(thread.id).then(() => renderPostTabs()).catch((error) => toast(`Could not load background post. ${explainError(error)}`));
  }

  async function getCachedThreadDownload(id) {
    let pending = cachedThreadOpensInFlight.get(id);
    if (!pending) {
      pending = invoke('open_cached_thread', { id }).then((result) => {
        const thread = acceptThreadDownload(result, id, { allowNotLoaded: true, updateListings: false, fetchPreviews: false });
        const media = result.media;
        mediaByThread.set(id, media);
        return { cached: true, thread, media };
      }).finally(() => cachedThreadOpensInFlight.delete(id));
      cachedThreadOpensInFlight.set(id, pending);
    }
    return pending;
  }

  async function openCachedSearchHit(hit) {
    const id = hit && hit.id;
    if (typeof id !== 'string' || !/^t3_[A-Za-z0-9_]+$/.test(id)) return;
    const anchor = typeof hit.comment_id === 'string' && /^t1_[A-Za-z0-9_]+$/.test(hit.comment_id) ? hit.comment_id : id;
    cancelCacheSearch({ hide: true });
    const transition = ++operationSequence;
    const startedAt = performance.now();
    ui.cacheSearchResults.hidden = true;
    if (!currentThread()) setReaderMessage('Opening cached post…');
    setFeedback('Opening the cached copy…');
    setStatus('Opening cached post…');
    startOperationProgress('Opening cached post', transition);
    try {
      const download = await getCachedThreadDownload(id);
      if (transition !== operationSequence) return;
      const thread = download.thread;
      const media = download.media;
      activeId = id;
      pendingAnchor = anchor;
      ensurePostTab(thread, anchor, { cachedOnly: true });
      draw();
      await focusAnchor(id, anchor, null);
      setTab('reader');
      setFeedback('');
      recordClientEvent('cache_search_open', performance.now() - startedAt, media.length);
      setStatus('Opened cached post · no source fetch', 'ready');
    } catch (error) {
      if (transition !== operationSequence) return;
      setFeedback(`Could not open this cached result. No source fetch was attempted. ${explainError(error)}`, true, () => void openCachedSearchHit(hit));
      setStatus('Cached post unavailable', 'error');
    } finally {
      stopOperationProgress(transition);
    }
  }

  async function refreshLocal() {
    const transition = ++operationSequence;
    pendingId = null;
    renderList();
    const selectedId = activeId;
    const anchor = snapshot && snapshot.position && snapshot.position[0] === selectedId ? snapshot.position[1] : selectedId;
    try {
      const result = await invoke('snapshot');
      if (transition !== operationSequence) return;
      await receiveSnapshot(result, selectedId, selectedId ? anchor : null);
      setStatus(`${result.threads.length} local posts`, 'ready');
      setFeedback('');
    } catch (error) {
      if (transition !== operationSequence) return;
      setFeedback(`Could not refresh the local library. Current reader content is unchanged. ${explainError(error)}`, true, () => void refreshLocal());
      setStatus('Local refresh failed', 'error');
    }
  }

  async function runImport(command, args, preferredId = null) {
    const transition = ++operationSequence;
    pendingId = null;
    renderList();
    try {
      const result = await invoke(command, args);
      if (transition !== operationSequence) return;
      rememberSnapshotThreads(result);
      mergeImportedListings(result.threads);
      await receiveSnapshot(result);
      mediaByThread.clear();
      await loadLibraryState({ quiet: true });
      const first = preferredId || (result.threads[0] && result.threads[0].id);
      setStatus('Import complete · choose a post to load its thread', 'ready');
      setFeedback('');
      toast('Snapshot imported into the local library.');
      if (first) {
        const button = ui.list.querySelector(`[data-thread-id="${CSS.escape(first)}"]`);
        if (button) button.querySelector('.thread-open')?.focus();
      }
    } catch (error) {
      if (transition !== operationSequence) return;
      setStatus('Import failed', 'error');
      setFeedback(`Import failed. Existing reader content is unchanged. ${explainError(error)}`, true);
    }
  }

  async function pruneAll() {
    if (!window.confirm('Remove unprotected cached posts? Saved Keep forever items and protected groups remain. This prunes cached post text only; downloaded media is not deleted.')) return;
    ui.clear.disabled = true;
    const transition = ++operationSequence;
    pendingId = null;
    renderList();
    try {
      const result = await invoke('prune_cache', { before: 253402300800 });
      if (transition !== operationSequence) return;
      if (!result || !Array.isArray(result.threads)) throw new Error('Native prune returned an invalid Snapshot.');
      const available = new Set(result.threads.map((thread) => thread.id));
      for (const id of [...threadCache.keys()]) {
        if (available.has(id)) continue;
        threadCache.delete(id);
        loadedThreadIds.delete(id);
        mediaByThread.delete(id);
        mediaHintsByThread.delete(id);
        inlineExpanded.delete(id);
        unavailableThreadIds.add(id);
      }
      for (const id of [...mediaHintsByThread.keys()]) if (!available.has(id)) mediaHintsByThread.delete(id);
      for (const [subreddit, listing] of listingBySubreddit) {
        listingBySubreddit.set(subreddit, listing.filter((thread) => available.has(thread.id)));
      }
      for (const subreddit of Object.keys(preferences.listings)) {
        preferences.listings[subreddit] = preferences.listings[subreddit].filter((id) => available.has(id));
      }
      for (const tab of preferences.postTabs) {
        if (available.has(tab.id)) unavailableThreadIds.delete(tab.id);
        else unavailableThreadIds.delete(tab.id);
      }
      const selectedId = activeId && available.has(activeId) ? activeId : null;
      const selectedAnchor = selectedId ? pendingAnchor : null;
      if (!selectedId) activeId = null;
      preferences.postTabs = preferences.postTabs.filter((tab) => available.has(tab.id));
      if (preferences.activePostId && !available.has(preferences.activePostId)) preferences.activePostId = null;
      savePreferences();
      await receiveSnapshot(result, selectedId, selectedAnchor);
      await loadLibraryState({ quiet: true });
      setStatus('Unprotected post cache pruned · protected items remain', 'ready');
      setFeedback('Unprotected cached post text was pruned. Saved Keep forever items and protected groups remain; downloaded media was not deleted.');
      toast('Unprotected cached posts pruned.');
    } catch (error) {
      if (transition !== operationSequence) return;
      setStatus(`Cache clear failed`, 'error');
      setFeedback(`Could not clear the local cache. ${explainError(error)}`, true, () => void pruneAll());
    } finally {
      ui.clear.disabled = false;
    }
  }

  function validateAppLink(link) {
    if (!link || !['post', 'subreddit'].includes(link.kind) || typeof link.url !== 'string') return null;
    if (!/^[a-z0-9_]{2,21}$/i.test(link.subreddit || '') || !['hot', 'new', 'top', 'rising'].includes(link.sort) || !['hour', 'day', 'week', 'month', 'year', 'all'].includes(link.top_period)) return null;
    let url;
    try { url = new URL(link.url); } catch (_) { return null; }
    if (url.protocol !== 'https:' || url.hostname !== 'www.reddit.com' || url.port || url.username || url.password || url.hash) return null;
    const pathSubreddit = url.pathname.match(/^\/r\/([a-z0-9_]{2,21})(?:\/(.*))?$/i);
    if (!pathSubreddit || pathSubreddit[1].toLowerCase() !== link.subreddit.toLowerCase()) return null;
    if (link.kind === 'subreddit') {
      if (link.id !== null || link.anchor !== null) return null;
      const suffix = pathSubreddit[2] || '';
      if (suffix.toLowerCase() !== `${link.sort}/`) return null;
      const expectedSearch = link.sort === 'top' ? `?t=${link.top_period}` : '';
      if (url.search !== expectedSearch) return null;
    } else {
      if (url.search) return null;
      const postPath = (pathSubreddit[2] || '').match(/^comments\/([A-Za-z0-9]+)(?:\/|$)/i);
      if (!postPath || typeof link.id !== 'string' || !/^t3_[A-Za-z0-9_]+$/.test(link.id) || postPath[1].toLowerCase() !== link.id.slice(3).toLowerCase()) return null;
      if (link.anchor !== null && (typeof link.anchor !== 'string' || !/^t1_[A-Za-z0-9_]+$/.test(link.anchor))) return null;
    }
    return { ...link, subreddit: link.subreddit.toLowerCase() };
  }

  function closeRoutingDialogs() {
    for (const dialog of [ui.settingsDialog, ui.diagnosticsDialog]) {
      if (!dialog.open) continue;
      if (typeof dialog.close === 'function') dialog.close();
      else dialog.removeAttribute('open');
    }
    ui.cacheSearchResults.hidden = true;
  }

  async function handleAppLink(rawLink) {
    const link = validateAppLink(rawLink);
    if (!link) {
      setFeedback('Ignored an invalid app link.');
      return;
    }
    closeRoutingDialogs();
    if (link.kind === 'subreddit') {
      preferences.postSort = link.sort;
      preferences.topPeriod = link.top_period;
      ui.postSort.value = link.sort;
      ui.topPeriod.value = link.top_period;
      updateSortNotice();
      savePreferences();
      setTab('browse');
      ui.input.value = `/r/${link.subreddit}`;
      await browseSubreddit(`/r/${link.subreddit}`, { authoritative: true });
      return;
    }

    const transition = ++operationSequence;
    setTab('reader');
    setFeedback('Opening shared post…');
    setStatus('Opening shared post…');
    startOperationProgress('Opening shared post', transition);
    try {
      const result = await invoke('open_link_post', { url: link.url });
      if (transition !== operationSequence) return;
      const thread = acceptThreadDownload(result, link.id);
      const media = result.media;
      mediaByThread.set(link.id, media);
      activeId = link.id;
      pendingId = null;
      pendingAnchor = link.anchor || link.id;
      ensurePostTab(thread, pendingAnchor);
      draw();
      await focusAnchor(link.id, pendingAnchor, null);
      setTab('reader');
      setFeedback('');
      setStatus(`Opened shared post · ${media.length} local media item${media.length === 1 ? '' : 's'}`, 'ready');
    } catch (error) {
      if (transition !== operationSequence) return;
      setFeedback(`Could not open the shared post. ${explainError(error)}`, true);
      setStatus('Shared post could not be opened', 'error');
    } finally {
      stopOperationProgress(transition);
    }
  }

  function requestAppLinkDrain() {
    appLinkDrainRequested = true;
    if (appLinksReady) void drainAppLinks();
  }

  async function drainAppLinks() {
    if (!invoke || !appLinksReady || appLinkDrainBusy || !appLinkDrainRequested) return;
    appLinkDrainBusy = true;
    try {
      while (appLinkDrainRequested) {
        appLinkDrainRequested = false;
        const links = await invoke('take_app_links');
        if (!Array.isArray(links)) throw new Error('Native app-link queue returned an invalid result.');
        for (const link of links) await handleAppLink(link);
      }
    } catch (error) {
      setFeedback(`Could not read queued app links. ${explainError(error)}`, true);
      setStatus('App link queue unavailable', 'error');
    } finally {
      appLinkDrainBusy = false;
      if (appLinkDrainRequested) queueMicrotask(() => void drainAppLinks());
    }
  }

  function listenForAppLinks() {
    const eventApi = tauri && tauri.event;
    if (!eventApi || typeof eventApi.listen !== 'function') return;
    try {
      Promise.resolve(eventApi.listen('hrungnir-app-links', () => requestAppLinkDrain()))
        .then(() => requestAppLinkDrain())
        .catch((error) => {
          setFeedback(`Could not listen for app links. ${explainError(error)}`, true);
          setStatus('App links unavailable', 'error');
        });
    } catch (error) {
      setFeedback(`Could not listen for app links. ${explainError(error)}`, true);
      setStatus('App links unavailable', 'error');
    }
  }

  function handleCloseActivePostTab() {
    if (browseDraftOpen) closeBrowseDraftTab();
    else if (activeId) closePostTab(activeId);
  }

  function listenForTabShortcuts() {
    const eventApi = tauri && tauri.event;
    if (!eventApi || typeof eventApi.listen !== 'function') return;
    Promise.resolve(eventApi.listen('tab-shortcut', (event) => {
      const action = event && event.payload;
      if (action === 'new' || action === 'close') dispatchTabShortcut(action, 'native');
    })).catch((error) => {
      console.warn('Could not listen for native tab shortcuts.', error);
    });
  }

  function configure() {
    window.addEventListener('pagehide', releaseAllMediaWatches);
    window.addEventListener('keydown', (event) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key !== 't' && key !== 'w') return;
      event.preventDefault();
      event.stopPropagation();
      if (key === 't') dispatchTabShortcut('new', 'keyboard');
      else dispatchTabShortcut('close', 'keyboard');
    });
    ui.fetchAllMedia.addEventListener('click', () => void fetchAllListedMedia());
    ui.newBrowseTab.addEventListener('click', openBrowseDraftTab);
    ui.theme.addEventListener('change', () => {
      preferences.theme = ['system', 'classified', 'classified_light'].includes(ui.theme.value) ? ui.theme.value : 'system';
      applyAppearance();
      savePreferences();
    });
    ui.accent.addEventListener('change', () => {
      preferences.accent = ['blue', 'red', 'amber', 'teal'].includes(ui.accent.value) ? ui.accent.value : 'blue';
      applyAppearance();
      savePreferences();
    });
    ui.tabs.forEach((button) => button.addEventListener('click', () => setTab(button.dataset.tab)));
    ui.tabs.forEach((button, index) => button.addEventListener('keydown', (event) => {
      const next = event.key === 'ArrowRight' ? (index + 1) % ui.tabs.length
        : event.key === 'ArrowLeft' ? (index + ui.tabs.length - 1) % ui.tabs.length
          : event.key === 'Home' ? 0 : event.key === 'End' ? ui.tabs.length - 1 : -1;
      if (next < 0) return;
      event.preventDefault();
      ui.tabs[next].focus();
      setTab(ui.tabs[next].dataset.tab);
    }));
    ui.settingsOpen.addEventListener('click', () => {
      if (typeof ui.settingsDialog.showModal === 'function') ui.settingsDialog.showModal();
      else ui.settingsDialog.setAttribute('open', '');
      void loadPostCacheSettings();
      void loadLibraryState({ quiet: true });
    });
    ui.settingsClose.addEventListener('click', () => {
      if (typeof ui.settingsDialog.close === 'function') ui.settingsDialog.close();
      else ui.settingsDialog.removeAttribute('open');
      ui.settingsOpen.focus();
    });
    ui.settingsDialog.addEventListener('click', (event) => {
      if (event.target === ui.settingsDialog && typeof ui.settingsDialog.close === 'function') ui.settingsDialog.close();
    });
    ui.settingsDialog.addEventListener('close', () => ui.settingsOpen.focus());
    const nativeControls = [ui.input, ui.browse, ui.browseRefresh, ui.fetchAllMedia, ui.postSort, ui.topPeriod, ui.demo, ui.fileButton, ui.file, ui.clear, ui.diagnosticsRefresh, ui.supportExport, ui.cacheSearch, ui.cacheSearchSubmit, ui.cacheSearchClear, ui.postCacheHours, ui.postCacheApply, ui.subredditGroupFilter, ui.librarySearch, ui.libraryCollectionFilter, ui.libraryKindFilter, ui.libraryExport, ui.libraryImport, ui.saveLinkUrl, ui.saveLinkKind, ui.saveLinkCollection, ui.saveLinkForever, ui.saveLinkButton, ui.groupSelect, ui.groupName, ui.groupSubreddits, ui.groupPostHours, ui.groupPostForever, ui.groupMediaHours, ui.groupMediaForever, ui.groupSave, ui.groupDelete];
    if (!invoke) {
      setStatus('Native app required', 'error');
      setReaderMessage('Launch this reader in the Tauri app to fetch posts, comments, and local media. The standalone page has no network or mock fallback.', true);
      for (const control of nativeControls) control.disabled = true;
      setFeedback('Native operations are unavailable outside the app.', true);
      return;
    }
    updateFetchAllMediaControl();

    for (const control of nativeControls) control.disabled = true;
    listenForAppLinks();
    listenForTabShortcuts();

    ui.form.addEventListener('submit', (event) => { event.preventDefault(); void browseSubreddit(); });
    ui.cacheSearchClear.disabled = !ui.cacheSearch.value;
    ui.cacheSearchForm.addEventListener('submit', (event) => {
      event.preventDefault();
      cancelCacheSearch({ hide: true });
      void searchReddit(ui.cacheSearch.value.trim(), 'relevance', 'all');
    });
    ui.cacheSearch.addEventListener('input', () => {
      ui.cacheSearchClear.disabled = !ui.cacheSearch.value;
      cancelRedditSearch({ clearFeedback: true });
      scheduleCacheSearch();
    });
    ui.cacheSearchClear.addEventListener('click', () => clearSearch({ focus: true }));
    ui.cacheSearch.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        if (!ui.cacheSearchResults.hidden || ui.cacheSearch.value || redditSearch || activeRedditSearchOperation) {
          event.preventDefault();
          event.stopPropagation();
          clearSearch({ focus: true });
        }
      } else if (event.key === 'ArrowDown' && !ui.cacheSearchResults.hidden) {
        const first = ui.cacheSearchResults.querySelector('.cache-search-hit');
        if (first) { event.preventDefault(); first.focus(); }
      }
    });
    ui.cacheSearchResults.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        clearSearch({ focus: true });
      }
    });
    document.addEventListener('pointerdown', (event) => {
      if (!(event.target instanceof Element) || !event.target.closest('.header-search')) {
        cancelCacheSearch({ hide: true });
      }
    });
    ui.postCacheHours.addEventListener('input', () => {
      cacheSettingsEditSequence++;
      cacheSettingsDirty = true;
      ui.postCacheHours.removeAttribute('aria-invalid');
      ui.cacheSettingsStatus.textContent = 'Unsaved change · apply to update the native cache.';
    });
    ui.postCacheApply.addEventListener('click', () => void applyPostCacheSettings());
    ui.browseRefresh.addEventListener('click', () => void browseSubreddit());
    ui.favoriteCurrent.addEventListener('click', () => {
      try { toggleFavorite(normalizeSubreddit(ui.input.value)); } catch (_) { /* disabled until valid */ }
    });
    ui.input.addEventListener('input', renderSubreddits);
    ui.subredditOrder.addEventListener('change', () => {
      preferences.subredditOrder = ui.subredditOrder.value;
      savePreferences();
      renderSubreddits();
    });
    ui.subredditGroupFilter.addEventListener('change', renderSubreddits);
    ui.librarySearch.addEventListener('input', () => scheduleLibrarySearch());
    ui.libraryCollectionFilter.addEventListener('change', renderLibrary);
    ui.libraryKindFilter.addEventListener('change', renderLibrary);
    ui.libraryExport.addEventListener('click', () => void exportLibrary());
    ui.libraryImport.addEventListener('click', () => void importLibrary());
    ui.saveLinkButton.addEventListener('click', () => void saveLinkedContent());
    ui.groupSelect.addEventListener('change', () => {
      groupEditDirty = false;
      ui.groupStatus.textContent = '';
      renderGroupEditor();
    });
    for (const field of [ui.groupName, ui.groupSubreddits, ui.groupPostHours, ui.groupPostForever, ui.groupMediaHours, ui.groupMediaForever]) {
      field.addEventListener('input', () => { groupEditDirty = true; });
      field.addEventListener('change', () => { groupEditDirty = true; renderGroupEditor(libraryState.groups.find((item) => item.id === ui.groupSelect.value) || null); });
    }
    ui.groupSave.addEventListener('click', () => void saveGroup());
    ui.groupDelete.addEventListener('click', () => void deleteGroup());
    ui.postSort.addEventListener('change', () => {
      if (redditSearch) {
        void searchReddit(redditSearch.query, ui.postSort.value, redditSearch.period);
        return;
      }
      preferences.postSort = ui.postSort.value;
      savePreferences();
      updateSortNotice();
      if (listingBySubreddit.has(activeSubreddit)) void browseSubreddit(`/r/${activeSubreddit}`);
    });
    ui.topPeriod.addEventListener('change', () => {
      if (redditSearch) {
        void searchReddit(redditSearch.query, redditSearch.sort, ui.topPeriod.value);
        return;
      }
      preferences.topPeriod = ui.topPeriod.value;
      savePreferences();
      updateSortNotice();
      if (preferences.postSort === 'top' && listingBySubreddit.has(activeSubreddit)) void browseSubreddit(`/r/${activeSubreddit}`);
    });
    ui.mediaPresentation.addEventListener('change', () => {
      preferences.mediaPresentation = ui.mediaPresentation.value;
      savePreferences();
      ui.view.querySelectorAll('.media-details').forEach((details) => { details.open = preferences.mediaPresentation === 'inline'; });
    });
    ui.readerTextScale.addEventListener('input', () => {
      preferences.readerTextScale = Math.max(85, Math.min(140, Number(ui.readerTextScale.value) || 100));
      ui.readerTextScaleValue.value = `${preferences.readerTextScale}%`;
      applyAppearance();
      savePreferences();
    });
    ui.readerUiScale.addEventListener('input', () => {
      preferences.readerUiScale = Math.max(85, Math.min(125, Number(ui.readerUiScale.value) || 100));
      ui.readerUiScaleValue.value = `${preferences.readerUiScale}%`;
      applyAppearance();
      savePreferences();
    });
    ui.diagnosticsOpen.addEventListener('click', () => {
      if (typeof ui.diagnosticsDialog.showModal === 'function') ui.diagnosticsDialog.showModal();
      else ui.diagnosticsDialog.setAttribute('open', '');
      void refreshAppDiagnostics();
    });
    ui.diagnosticsClose.addEventListener('click', () => {
      if (typeof ui.diagnosticsDialog.close === 'function') ui.diagnosticsDialog.close();
      else ui.diagnosticsDialog.removeAttribute('open');
    });
    ui.diagnosticsDialog.addEventListener('close', () => ui.diagnosticsOpen.focus());
    ui.demo.addEventListener('click', () => void runImport('import_fixture'));
    ui.fileButton.addEventListener('click', () => ui.file.click());
    ui.file.addEventListener('change', async () => {
      const file = ui.file.files && ui.file.files[0];
      ui.file.value = '';
      if (!file) return;
      if (file.size > MAX_IMPORT_BYTES) { toast('File exceeds the 4 MiB import limit.'); return; }
      try {
        const json = await file.text();
        if (new TextEncoder().encode(json).byteLength > MAX_IMPORT_BYTES) { toast('File exceeds the 4 MiB import limit.'); return; }
        let preferredId = null;
        try { const parsed = JSON.parse(json); if (parsed && typeof parsed.id === 'string') preferredId = parsed.id; }
        catch (_) { toast('The selected file is not valid JSON.'); return; }
        await runImport('import_json', { json }, preferredId);
      } catch (error) { toast(`Could not read JSON file: ${explainError(error)}`); }
    });
    ui.clear.addEventListener('click', () => void pruneAll());
    ui.diagnosticsRefresh.addEventListener('click', () => void refreshAppDiagnostics());
    ui.diagnosticsCopy.addEventListener('click', () => void copyDiagnostics());
    ui.diagnosticsExport.addEventListener('click', exportDiagnostics);
    ui.supportExport.addEventListener('click', () => void exportSupportBundle());
    ui.trustRecoveryButton.addEventListener('click', () => void allowProxyCertificate());
    setTab(preferences.tab, false);
    updateSortNotice();
    renderSubreddits();
    renderLibrary();

    document.addEventListener('keydown', (event) => {
      if (ui.settingsDialog.open || ui.diagnosticsDialog.open) return;
      const target = event.target instanceof Element ? event.target : null;
      const threadItem = target && target.closest('.thread-item');
      if (event.altKey || event.ctrlKey || event.metaKey || (target && target.closest('input,textarea,select,button') && !threadItem)) return;
      if (ui.list.inert) return;
      const threads = listingBySubreddit.get(activeSubreddit) || [];
      if (!threads.length) return;
      if (event.key.toLowerCase() === 'j' || event.key.toLowerCase() === 'k') {
        event.preventDefault();
        const delta = event.key.toLowerCase() === 'j' ? 1 : -1;
        keyboardThread = (keyboardThread + delta + threads.length) % threads.length;
        selectThread(threads[keyboardThread].id, threads[keyboardThread].id);
      } else if (event.key === 'Escape') {
        const focusedComment = document.activeElement.closest && document.activeElement.closest('.comment');
        if (focusedComment) document.activeElement.blur();
        else ui.list.querySelector('.thread-item[aria-current="true"] .thread-open')?.focus();
      }
    });

    void (async () => {
      try {
        setStatus('Loading local library…');
        const result = await invoke('snapshot');
        await receiveSnapshot(result);
        await loadLibraryState({ quiet: true });
        setStatus(`${result.threads.length} local posts`, 'ready');
        const position = result.position;
        const persistedTab = preferences.postTabs.find((tab) => tab.id === preferences.activePostId)
          || (preferences.postTabs.length ? preferences.postTabs[preferences.postTabs.length - 1] : null);
        const restoreId = persistedTab ? persistedTab.id : (position && position[0]);
        if (restoreId) {
          const anchor = (persistedTab && persistedTab.anchor) || (position && position[0] === restoreId ? position[1] : restoreId);
          await selectThread(restoreId, anchor, preferences.tab === 'reader', persistedTab && persistedTab.cachedOnly ? 'restore' : 'normal');
        }
      } catch (error) {
        setStatus('Could not load the local library', 'error');
        setReaderMessage(`Could not load local posts: ${explainError(error)}`, true);
        setFeedback(`Local library error. Retry the refresh after checking storage. ${explainError(error)}`, true, () => void refreshLocal());
      } finally {
        for (const control of nativeControls) control.disabled = false;
        renderGroupEditor();
        appLinksReady = true;
        requestAppLinkDrain();
      }
    })();
  }

  configure();
})();
