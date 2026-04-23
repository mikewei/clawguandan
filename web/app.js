const SESSION_KEY = "clawguandan.web.session.v1";
const PLAYER_NAME_KEY = "clawguandan.web.player_name.v1";
const POLL_TIMEOUT_MS = 60000;
const LOBBY_AUTO_REFRESH_MS = 2000;
const SEATS = ["E", "S", "W", "N"];
const CLOCKWISE_SEATS = ["N", "E", "S", "W"];
const FEED_MODE_MEDIA_QUERY = "(max-width: 640px) and (orientation: portrait)";
const TOUCH_POINTER_MEDIA_QUERY = "(hover: none), (pointer: coarse)";
const TRICK_RESET_PASS_COUNT = 3;
const TRICK_LOOKBACK_LIMIT = 16;
const HAND_GROUP_CHAIN_MS = 250;
const ID_HOVERCARD_HIDE_DELAY_MS = 120;
const SVG_CARDS_SPRITE_PATH = "/cards/svg-cards/svg-cards.svg";
const SVG_CARD_VIEWBOX = "0 0 169.075 244.64";
const SESSION_MODE_PLAYER = "player";
const SESSION_MODE_OBSERVER = "observer";
const CREATE_RANK_OPTIONS = new Set([
  "2",
  "3",
  "4",
  "5",
  "6",
  "7",
  "8",
  "9",
  "10",
  "J",
  "Q",
  "K",
  "A",
]);

const state = {
  session: loadSession(),
  tableState: null,
  privateView: null,
  expect: null,
  prompt: "",
  tables: [],
  polling: false,
  stopPolling: false,
  pendingReadySubmit: false,
  selectedHandIndexes: new Set(),
  lastDeltaPaths: [],
  preferredPlayerName: loadPreferredPlayerName(),
  pendingTributeGhost: null,
};

const el = {
  connectionState: document.getElementById("connectionState"),
  lobbyView: document.getElementById("lobbyView"),
  tableView: document.getElementById("tableView"),
  portraitSeatOverview: document.getElementById("portraitSeatOverview"),
  portraitSeatGrid: document.getElementById("portraitSeatGrid"),
  portraitTableInPlayTitle: document.getElementById("portraitTableInPlayTitle"),
  tableStage: document.getElementById("tableStage"),
  tableInPlayTitle: document.getElementById("tableInPlayTitle"),
  tablesList: document.getElementById("tablesList"),
  tablesEmptyHint: document.getElementById("tablesEmptyHint"),
  refreshTablesBtn: document.getElementById("refreshTablesBtn"),
  playerNameModal: document.getElementById("playerNameModal"),
  playerNameModalInput: document.getElementById("playerNameModalInput"),
  playerNameCancelBtn: document.getElementById("playerNameCancelBtn"),
  playerNameConfirmBtn: document.getElementById("playerNameConfirmBtn"),
  createTableModal: document.getElementById("createTableModal"),
  createTableModalInput: document.getElementById("createTableModalInput"),
  createTableModalRank: document.getElementById("createTableModalRank"),
  createTableCancelBtn: document.getElementById("createTableCancelBtn"),
  createTableConfirmBtn: document.getElementById("createTableConfirmBtn"),
  sessionInfo: document.getElementById("sessionInfo"),
  expectInfo: document.getElementById("expectInfo"),
  promptInfo: document.getElementById("promptInfo"),
  errorBox: document.getElementById("errorBox"),
  actionToast: document.getElementById("actionToast"),
  appMetaVersion: document.getElementById("appMetaVersion"),
  debugPanel: document.getElementById("debugPanel"),
  tablePlayersDetail: document.getElementById("tablePlayersDetail"),
  latestStateJsonTree: document.getElementById("latestStateJsonTree"),
  readyCta: document.getElementById("readyCta"),
  readyBtn: document.getElementById("readyBtn"),
  readyFlowRow: document.getElementById("readyFlowRow"),
  readyFlowBtn: document.getElementById("readyFlowBtn"),
  tableMeta: document.getElementById("tableMeta"),
  tableNarration: document.getElementById("tableNarration"),
  tableTurnInfo: document.getElementById("tableTurnInfo"),
  tableLegalActions: document.getElementById("tableLegalActions"),
  seatGrid: document.getElementById("seatGrid"),
  trickFeedWrap: document.getElementById("trickFeedWrap"),
  trickFeed: document.getElementById("trickFeed"),
  topPlay: document.getElementById("topPlay"),
  trickBySeat: document.getElementById("trickBySeat"),
  history: document.getElementById("history"),
  passBtn: document.getElementById("passBtn"),
  tributeRow: document.getElementById("tributeRow"),
  tributeBtn: document.getElementById("tributeBtn"),
  returnRow: document.getElementById("returnRow"),
  returnCardBtn: document.getElementById("returnCardBtn"),
  privateHand: document.getElementById("privateHand"),
  privateHandArea: document.querySelector(".private-hand-area"),
  playBtn: document.getElementById("playBtn"),
};

const stateReplay = window.stateReplay;
if (!stateReplay || typeof stateReplay.applyTransitionBody !== "function") {
  throw new Error("stateReplay module is missing applyTransitionBody()");
}

let t = window.i18n && window.i18n.t ? window.i18n.t : (key) => key;
let tf = window.i18n && window.i18n.tf
  ? window.i18n.tf
  : (key, vars) => {
      let msg = t(key);
      if (!vars) return msg;
      Object.keys(vars).forEach((k) => {
        msg = msg.replaceAll(`{${k}}`, String(vars[k]));
      });
      return msg;
    };
let actionToastTimer = null;
let lobbyRefreshTimer = null;
let lobbyRefreshInFlight = false;
let debugPanelUnlocked = false;
let playerNameModalResolver = null;
let createTableModalResolver = null;
let layoutRenderFrameId = 0;
let layoutSettleTimer = null;
let lastLayoutRenderKey = "";
let idHovercardRoot = null;
let idHovercardLabel = null;
let idHovercardValue = null;
let idHovercardAnchor = null;
let idHovercardPoint = null;
let idHovercardPinned = false;
let idHovercardHideTimer = null;
/** @type {{ type: 'select'|'deselect', groupKey: string, timerId: ReturnType<typeof setTimeout> } | null} */
let handGroupChainState = null;

function handGroupKeyFromIndices(groupIndices) {
  return [...groupIndices].sort((a, b) => a - b).join(",");
}

function clearHandGroupChainState() {
  if (handGroupChainState?.timerId != null) {
    clearTimeout(handGroupChainState.timerId);
  }
  handGroupChainState = null;
}

function armHandGroupChainTimer() {
  if (!handGroupChainState) return;
  if (handGroupChainState.timerId != null) {
    clearTimeout(handGroupChainState.timerId);
  }
  const tid = setTimeout(() => {
    if (handGroupChainState && handGroupChainState.timerId === tid) {
      handGroupChainState = null;
    }
  }, HAND_GROUP_CHAIN_MS);
  handGroupChainState.timerId = tid;
}

function onHandCardClick(idx, groupIndices) {
  const gk = handGroupKeyFromIndices(groupIndices);

  // Same group while chain active: every tap (each within 250ms of the last) repeats the
  // full-group action; timer refresh keeps mobile multi-taps stable.
  if (handGroupChainState && handGroupChainState.groupKey === gk) {
    if (handGroupChainState.type === "select") {
      groupIndices.forEach((i) => state.selectedHandIndexes.add(i));
    } else {
      groupIndices.forEach((i) => state.selectedHandIndexes.delete(i));
    }
    armHandGroupChainTimer();
    syncPrivateHandSelection();
    return;
  }

  clearHandGroupChainState();

  if (state.selectedHandIndexes.has(idx)) {
    state.selectedHandIndexes.delete(idx);
    handGroupChainState = { type: "deselect", groupKey: gk, timerId: null };
    armHandGroupChainTimer();
  } else {
    state.selectedHandIndexes.add(idx);
    handGroupChainState = { type: "select", groupKey: gk, timerId: null };
    armHandGroupChainTimer();
  }
  syncPrivateHandSelection();
}

const renderCache = {
  privateHandContentSig: null,
  privateHandSelectionSig: null,
  privateHandButtons: [],
  trickLayerSig: null,
  topPlaySig: null,
  historySig: null,
  trickFeedSig: null,
};

function clearTableRenderCache() {
  renderCache.privateHandContentSig = null;
  renderCache.privateHandSelectionSig = null;
  renderCache.privateHandButtons = [];
  renderCache.trickLayerSig = null;
  renderCache.topPlaySig = null;
  renderCache.historySig = null;
  renderCache.trickFeedSig = null;
}

function isTouchPointerMode() {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return false;
  }
  return window.matchMedia(TOUCH_POINTER_MEDIA_QUERY).matches;
}

function ensureIdHovercard() {
  if (idHovercardRoot) return;
  const root = document.createElement("div");
  root.className = "id-hovercard hidden";
  root.setAttribute("aria-hidden", "true");
  const label = document.createElement("div");
  label.className = "id-hovercard-label";
  const value = document.createElement("div");
  value.className = "id-hovercard-value mono";
  root.appendChild(label);
  root.appendChild(value);
  root.addEventListener("mouseenter", () => {
    if (!idHovercardPinned) {
      cancelIdHovercardHide();
    }
  });
  root.addEventListener("mouseleave", () => {
    if (!idHovercardPinned) {
      scheduleIdHovercardHide();
    }
  });
  root.addEventListener("click", (ev) => {
    ev.stopPropagation();
  });
  document.body.appendChild(root);
  idHovercardRoot = root;
  idHovercardLabel = label;
  idHovercardValue = value;
}

function cancelIdHovercardHide() {
  if (idHovercardHideTimer) {
    clearTimeout(idHovercardHideTimer);
    idHovercardHideTimer = null;
  }
}

function hideIdHovercard() {
  cancelIdHovercardHide();
  if (!idHovercardRoot) return;
  idHovercardRoot.classList.add("hidden");
  idHovercardRoot.setAttribute("aria-hidden", "true");
  idHovercardAnchor = null;
  idHovercardPoint = null;
  idHovercardPinned = false;
}

function scheduleIdHovercardHide() {
  cancelIdHovercardHide();
  idHovercardHideTimer = setTimeout(() => {
    hideIdHovercard();
  }, ID_HOVERCARD_HIDE_DELAY_MS);
}

function positionIdHovercard(anchor) {
  if (!idHovercardRoot || !anchor) return;
  const margin = 10;
  const gap = 6;
  const cardWidth = idHovercardRoot.offsetWidth;
  const cardHeight = idHovercardRoot.offsetHeight;
  const maxLeft = Math.max(margin, window.innerWidth - cardWidth - margin);
  let left = margin;
  let top = margin;
  if (idHovercardPoint && Number.isFinite(idHovercardPoint.x) && Number.isFinite(idHovercardPoint.y)) {
    left = Math.min(Math.max(idHovercardPoint.x + 10, margin), maxLeft);
    top = idHovercardPoint.y - cardHeight - 10;
    if (top < margin) {
      top = idHovercardPoint.y + 12;
    }
  } else {
    const rect = anchor.getBoundingClientRect();
    const preferredLeft = rect.right - cardWidth + 4;
    left = Math.min(Math.max(preferredLeft, margin), maxLeft);
    top = rect.top - cardHeight - gap;
    if (top < margin) {
      top = rect.bottom + gap;
    }
  }
  if (top + cardHeight > window.innerHeight - margin) {
    top = Math.max(margin, window.innerHeight - cardHeight - margin);
  }
  idHovercardRoot.style.left = `${Math.round(left)}px`;
  idHovercardRoot.style.top = `${Math.round(top)}px`;
}

function showIdHovercard(anchor, kind, idValue, options = {}) {
  const value = String(idValue || "").trim();
  if (!anchor || !value) return;
  ensureIdHovercard();
  cancelIdHovercardHide();
  idHovercardPinned = Boolean(options.pinned);
  idHovercardAnchor = anchor;
  idHovercardPoint = options.point && Number.isFinite(options.point.x) && Number.isFinite(options.point.y)
    ? { x: options.point.x, y: options.point.y }
    : null;
  idHovercardLabel.textContent = kind;
  idHovercardValue.textContent = `Id: ${value}`;
  idHovercardRoot.classList.remove("hidden");
  idHovercardRoot.setAttribute("aria-hidden", "false");
  positionIdHovercard(anchor);
  window.requestAnimationFrame(() => {
    if (idHovercardAnchor === anchor && idHovercardRoot && !idHovercardRoot.classList.contains("hidden")) {
      positionIdHovercard(anchor);
    }
  });
}

function bindIdHovercardTrigger(node, kind, idValue) {
  if (!node) return;
  const value = String(idValue || "").trim();
  node.classList.toggle("id-hover-trigger", Boolean(value));
  node.dataset.idHovercardKind = kind;
  node.dataset.idHovercardValue = value;
  if (node.dataset.idHovercardBound === "1") return;
  node.dataset.idHovercardBound = "1";
  node.addEventListener("mouseenter", (ev) => {
    if (isTouchPointerMode()) return;
    const hoverKind = String(node.dataset.idHovercardKind || "").trim();
    const hoverValue = String(node.dataset.idHovercardValue || "").trim();
    if (!hoverValue) return;
    showIdHovercard(node, hoverKind, hoverValue, {
      pinned: false,
      point: { x: ev.clientX, y: ev.clientY },
    });
  });
  node.addEventListener("mouseleave", () => {
    if (isTouchPointerMode()) return;
    scheduleIdHovercardHide();
  });
  node.addEventListener("click", (ev) => {
    const hoverKind = String(node.dataset.idHovercardKind || "").trim();
    const hoverValue = String(node.dataset.idHovercardValue || "").trim();
    if (!hoverValue) return;
    if (!isTouchPointerMode()) return;
    ev.preventDefault();
    ev.stopPropagation();
    const rootVisible = idHovercardRoot && !idHovercardRoot.classList.contains("hidden");
    const sameAnchor = idHovercardAnchor === node;
    if (rootVisible && sameAnchor && idHovercardPinned) {
      hideIdHovercard();
      return;
    }
    showIdHovercard(node, hoverKind, hoverValue, {
      pinned: true,
      point: { x: ev.clientX, y: ev.clientY },
    });
  });
}

function syncIdHovercardAnchor() {
  if (!idHovercardAnchor) return;
  if (!document.body.contains(idHovercardAnchor)) {
    hideIdHovercard();
    return;
  }
  if (idHovercardAnchor instanceof HTMLElement && idHovercardAnchor.offsetParent === null) {
    hideIdHovercard();
    return;
  }
  if (idHovercardRoot && !idHovercardRoot.classList.contains("hidden")) {
    positionIdHovercard(idHovercardAnchor);
  }
}

if (!state.preferredPlayerName && state.session?.playerName) {
  savePreferredPlayerName(state.session.playerName);
}

function loadSession() {
  try {
    const raw = sessionStorage.getItem(SESSION_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || !parsed.tableId) return null;
    if (parsed.mode === SESSION_MODE_OBSERVER) {
      return {
        mode: SESSION_MODE_OBSERVER,
        tableId: String(parsed.tableId),
        lastAppliedSeq: Number(parsed.lastAppliedSeq || 0),
      };
    }
    if (!parsed.playerId || !parsed.playerKey) return null;
    return {
      mode: SESSION_MODE_PLAYER,
      tableId: String(parsed.tableId),
      playerId: String(parsed.playerId),
      playerKey: String(parsed.playerKey),
      playerName: String(parsed.playerName || ""),
      lastAppliedSeq: Number(parsed.lastAppliedSeq || 0),
    };
  } catch (_err) {
    return null;
  }
}

function isObserverSession(session) {
  return Boolean(session && session.mode === SESSION_MODE_OBSERVER);
}

function isPlayerSession(session) {
  return Boolean(
    session
      && session.mode !== SESSION_MODE_OBSERVER
      && session.playerId
      && session.playerKey,
  );
}

function createObserverSession(tableId) {
  return {
    mode: SESSION_MODE_OBSERVER,
    tableId: String(tableId || ""),
    lastAppliedSeq: 0,
  };
}

function createPlayerSession(data) {
  return {
    mode: SESSION_MODE_PLAYER,
    tableId: String(data.tableId || ""),
    playerId: String(data.playerId || ""),
    playerKey: String(data.playerKey || ""),
    playerName: String(data.playerName || ""),
    lastAppliedSeq: Number(data.lastAppliedSeq || 0),
  };
}

function loadPreferredPlayerName() {
  try {
    return String(sessionStorage.getItem(PLAYER_NAME_KEY) || "").trim();
  } catch (_err) {
    return "";
  }
}

function savePreferredPlayerName(playerName) {
  const nextName = String(playerName || "").trim();
  state.preferredPlayerName = nextName;
  try {
    if (!nextName) {
      sessionStorage.removeItem(PLAYER_NAME_KEY);
    } else {
      sessionStorage.setItem(PLAYER_NAME_KEY, nextName);
    }
  } catch (_err) {}
}

function saveSession() {
  if (!state.session) return;
  sessionStorage.setItem(SESSION_KEY, JSON.stringify(state.session));
}

function clearSession() {
  state.session = null;
  state.tableState = null;
  state.privateView = null;
  state.expect = null;
  state.prompt = "";
  state.pendingReadySubmit = false;
  state.selectedHandIndexes.clear();
  state.lastDeltaPaths = [];
  state.pendingTributeGhost = null;
  state.stopPolling = true;
  clearTableRenderCache();
  sessionStorage.removeItem(SESSION_KEY);
  stopLobbyAutoRefresh();
  refreshLobby().catch(() => {});
  startLobbyAutoRefresh();
  render();
}

function isServerStateMissingError(err) {
  const status = Number(err?.status || err?.body?.error?.status || 0);
  if (status === 404 || status === 410) return true;
  const msg = String(
    err?.body?.error?.code ||
    err?.body?.error?.message ||
    err?.message ||
    "",
  ).toLowerCase();
  return /not\s*found|missing|unknown\s*player|unknown\s*table|no local state/.test(msg);
}

async function recoverWhenServerStateMissing(err) {
  if (!isServerStateMissingError(err)) return false;
  const ok = window.confirm(t("serverStateGoneConfirm"));
  if (ok) {
    clearSession();
    setError("");
    await refreshLobby().catch(() => {});
    window.location.reload();
  } else {
    setError(t("serverStateGoneHint"));
  }
  return true;
}

function isAuthInvalidError(err) {
  const status = Number(err?.status || err?.body?.error?.status || 0);
  if (status !== 401 && status !== 403) return false;
  const msg = String(
    err?.body?.error?.code ||
    err?.body?.error?.message ||
    err?.message ||
    "",
  ).toLowerCase();
  return /player\s*key|playerkey|not\s*seated|forbidden|unauthorized|auth/.test(msg);
}

async function recoverWhenAuthInvalid(err) {
  if (!isAuthInvalidError(err)) return false;
  const ok = window.confirm(t("authInvalidConfirm"));
  if (ok) {
    clearSession();
    setError("");
  } else {
    setError(t("authInvalidHint"));
  }
  return true;
}

async function handleRequestError(err, options = {}) {
  const { showToast = true } = options;
  if (await recoverWhenServerStateMissing(err)) return;
  if (await recoverWhenAuthInvalid(err)) return;
  if (showToast) {
    showActionToast(err?.message || String(err));
  } else {
    setError(err?.message || String(err));
  }
}

function setError(message) {
  if (!message) {
    el.errorBox.classList.add("hidden");
    el.errorBox.textContent = "";
    return;
  }
  el.errorBox.classList.remove("hidden");
  el.errorBox.textContent = message;
}

function setConnection(text) {
  el.connectionState.textContent = text;
}

function showActionToast(message, durationMs = 2200) {
  if (!message || !el.actionToast) return;
  if (actionToastTimer) {
    clearTimeout(actionToastTimer);
    actionToastTimer = null;
  }
  el.actionToast.textContent = message;
  el.actionToast.classList.remove("hidden");
  actionToastTimer = setTimeout(() => {
    el.actionToast.classList.add("hidden");
    el.actionToast.textContent = "";
    actionToastTimer = null;
  }, durationMs);
}

async function renderFooterMeta() {
  if (!el.appMetaVersion) return;
  try {
    const { json } = await apiFetch("/ping", { method: "GET" });
    const ver = String(json?.ver || "").trim();
    if (!ver) {
      el.appMetaVersion.textContent = t("footerVersionUnavailable");
      return;
    }
    el.appMetaVersion.textContent = tf("footerVersion", { ver });
  } catch (_err) {
    el.appMetaVersion.textContent = t("footerVersionUnavailable");
  }
}

function revealDebugPanel() {
  if (!el.debugPanel || debugPanelUnlocked) return;
  debugPanelUnlocked = true;
  el.debugPanel.classList.remove("hidden");
  el.debugPanel.open = true;
}

function stopLobbyAutoRefresh() {
  if (!lobbyRefreshTimer) return;
  clearInterval(lobbyRefreshTimer);
  lobbyRefreshTimer = null;
}

async function refreshLobbyAuto() {
  if (state.session || lobbyRefreshInFlight) return;
  lobbyRefreshInFlight = true;
  try {
    await refreshLobby();
  } catch (err) {
    console.warn("[lobby-auto-refresh]", err);
  } finally {
    lobbyRefreshInFlight = false;
  }
}

function startLobbyAutoRefresh() {
  if (state.session || lobbyRefreshTimer) return;
  lobbyRefreshTimer = setInterval(() => {
    if (document.visibilityState !== "visible") return;
    refreshLobbyAuto();
  }, LOBBY_AUTO_REFRESH_MS);
}

function syncLobbyAutoRefresh() {
  if (state.session) {
    stopLobbyAutoRefresh();
  } else {
    startLobbyAutoRefresh();
  }
}

function normalizeEnumToken(raw) {
  return String(raw || "")
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "_")
    .replaceAll(/^_+|_+$/g, "");
}

function deriveSimpleTableState(status, phase) {
  const statusNorm = normalizeEnumToken(status);
  const phaseNorm = normalizeEnumToken(phase);
  const token = `${statusNorm}|${phaseNorm}`;
  if (/finished|finish|end|ended|closed|done|terminated/.test(token)) {
    return "finished";
  }
  if (/play|playing|running|in_game|tribute|exchange|action/.test(token)) {
    return "inGame";
  }
  return "waiting";
}

function formatTableStatusLabel(status, phase) {
  const simpleState = deriveSimpleTableState(status, phase);
  return t(`tableStateSimple.${simpleState}`);
}

function isSeatUnavailableError(err) {
  const status = Number(err?.status || err?.body?.error?.status || 0);
  if (status === 409) return true;
  const msg = String(
    err?.body?.error?.code ||
    err?.body?.error?.message ||
    err?.message ||
    "",
  ).toLowerCase();
  return /seat|occupied|taken|unavailable|already/.test(msg);
}

function normalizeJoinSeat(seat) {
  const raw = String(seat || "auto").trim().toUpperCase();
  if (SEATS.includes(raw)) return raw;
  return "auto";
}

function showPlayerNameModal(defaultName = "") {
  return new Promise((resolve) => {
    if (playerNameModalResolver) {
      playerNameModalResolver("");
      playerNameModalResolver = null;
    }
    playerNameModalResolver = resolve;
    el.playerNameModalInput.value = defaultName;
    el.playerNameModal.classList.remove("hidden");
    el.playerNameModal.setAttribute("aria-hidden", "false");
    window.requestAnimationFrame(() => {
      el.playerNameModalInput.focus();
      el.playerNameModalInput.select();
    });
  });
}

function closePlayerNameModal(result) {
  if (!playerNameModalResolver) return;
  const resolve = playerNameModalResolver;
  playerNameModalResolver = null;
  el.playerNameModal.classList.add("hidden");
  el.playerNameModal.setAttribute("aria-hidden", "true");
  resolve(result);
}

function showCreateTableModal(defaultName = "", defaultRank = "2") {
  return new Promise((resolve) => {
    if (createTableModalResolver) {
      createTableModalResolver(null);
      createTableModalResolver = null;
    }
    createTableModalResolver = resolve;
    el.createTableModalInput.value = defaultName;
    el.createTableModalRank.value = normalizeCreateRank(defaultRank);
    el.createTableModal.classList.remove("hidden");
    el.createTableModal.setAttribute("aria-hidden", "false");
    window.requestAnimationFrame(() => {
      el.createTableModalInput.focus();
      el.createTableModalInput.select();
    });
  });
}

function closeCreateTableModal(result) {
  if (!createTableModalResolver) return;
  const resolve = createTableModalResolver;
  createTableModalResolver = null;
  el.createTableModal.classList.add("hidden");
  el.createTableModal.setAttribute("aria-hidden", "true");
  resolve(result);
}

async function createTableFromModal() {
  const result = await showCreateTableModal("");
  if (result == null) return;
  const tableName = String(result.name || "").trim();
  const rank = normalizeCreateRank(result.rank);
  await createTable(tableName, rank);
}

function normalizeCreateRank(raw) {
  const normalized = String(raw || "")
    .trim()
    .toUpperCase();
  if (!normalized) return "2";
  return CREATE_RANK_OPTIONS.has(normalized) ? normalized : "";
}

async function ensurePlayerName() {
  const cached = String(state.preferredPlayerName || "").trim();
  if (cached) return cached;
  const fromModal = await showPlayerNameModal("");
  const name = String(fromModal || "").trim();
  if (!name) {
    showActionToast(t("errPlayerNameRequired"));
    return "";
  }
  savePreferredPlayerName(name);
  return name;
}

async function apiFetch(path, options = {}) {
  const response = await fetch(path, {
    headers: {
      "content-type": "application/json",
      ...(options.headers || {}),
    },
    ...options,
  });

  if (response.status === 204) {
    return { response, json: null };
  }

  const json = await response.json().catch(() => null);
  if (!response.ok) {
    const msg =
      json?.error?.message ||
      json?.message ||
      `HTTP ${response.status} for ${path}`;
    const err = new Error(msg);
    err.body = json;
    err.status = response.status;
    throw err;
  }
  return { response, json };
}

async function refreshLobby() {
  const { json } = await apiFetch("/api/v1/tables");
  state.tables = json.tables || [];
  renderTables();
}

async function createTable(name = "", rank = "2") {
  setError("");
  const tableName = String(name || "").trim();
  const rankToken = normalizeCreateRank(rank);
  if (!rankToken) {
    throw new Error(t("errRankInvalid"));
  }
  const body = { rank: rankToken };
  if (tableName) {
    body.name = tableName;
  }
  const { json } = await apiFetch("/api/v1/tables", {
    method: "POST",
    body: JSON.stringify(body),
  });
  await refreshLobby();
  return json.tableId || "";
}

const syncDriver = {
  buildSnapshotQuery(session) {
    if (!session) return new URLSearchParams();
    if (isPlayerSession(session)) {
      return new URLSearchParams({
        playerId: session.playerId,
        playerKey: session.playerKey,
      });
    }
    return new URLSearchParams();
  },
  buildNextstateQuery(session) {
    const params = new URLSearchParams({
      sinceSeq: String(session?.lastAppliedSeq || 0),
      timeoutMs: String(POLL_TIMEOUT_MS),
    });
    if (isPlayerSession(session)) {
      params.set("playerId", session.playerId);
      params.set("playerKey", session.playerKey);
    }
    return params;
  },
};

async function joinTable(options = {}) {
  setError("");
  const tableId = String(options.tableId || "").trim();
  if (!tableId) {
    showActionToast(t("errTableRequired"));
    return false;
  }
  const seat = normalizeJoinSeat(options.seat);
  let playerName = String(options.playerName || state.preferredPlayerName || "").trim();
  if (!playerName) {
    playerName = await ensurePlayerName();
    if (!playerName) return false;
  }
  savePreferredPlayerName(playerName);

  const { json } = await apiFetch(`/api/v1/tables/${encodeURIComponent(tableId)}/join`, {
    method: "POST",
    body: JSON.stringify({
      playerType: "human",
      playerName,
      seat,
    }),
  });

  state.session = createPlayerSession({
    tableId,
    playerId: json.playerId,
    playerKey: json.playerKey,
    playerName,
    lastAppliedSeq: Number(json.newSeq || 0),
  });
  saveSession();
  stopLobbyAutoRefresh();
  await bootstrapSnapshot();
  startPolling();
  return true;
}

async function spectateTable(tableId) {
  setError("");
  const nextTableId = String(tableId || "").trim();
  if (!nextTableId) {
    showActionToast(t("errTableRequired"));
    return false;
  }
  state.session = createObserverSession(nextTableId);
  saveSession();
  stopLobbyAutoRefresh();
  await bootstrapSnapshot();
  startPolling();
  return true;
}

async function joinTableFromLobby(tableId, seat = "auto") {
  const seatChoice = normalizeJoinSeat(seat);
  try {
    return await joinTable({ tableId, seat: seatChoice });
  } catch (err) {
    if (await recoverWhenServerStateMissing(err)) return false;
    if (await recoverWhenAuthInvalid(err)) return false;
    if (seatChoice !== "auto" && isSeatUnavailableError(err)) {
      const shouldAutoJoin = window.confirm(t("seatTakenAutoJoinConfirm"));
      if (shouldAutoJoin) {
        try {
          return await joinTable({ tableId, seat: "auto" });
        } catch (retryErr) {
          if (await recoverWhenServerStateMissing(retryErr)) return false;
          if (await recoverWhenAuthInvalid(retryErr)) return false;
          showActionToast(retryErr.message);
          return false;
        }
      }
      return false;
    }
    showActionToast(err.message);
    return false;
  }
}

async function spectateTableFromLobby(tableId) {
  try {
    return await spectateTable(tableId);
  } catch (err) {
    if (await recoverWhenServerStateMissing(err)) return false;
    showActionToast(err.message);
    return false;
  }
}

async function bootstrapSnapshot() {
  if (!state.session) return;
  const p = syncDriver.buildSnapshotQuery(state.session);
  const path = `/api/v1/tables/${encodeURIComponent(state.session.tableId)}/snapshot?${p.toString()}`;
  const { json } = await apiFetch(path);
  state.tableState = json;
  state.privateView = isPlayerSession(state.session) ? (json.private || null) : null;
  state.expect = json.expect || null;
  state.prompt = "";
  state.pendingReadySubmit = false;
  state.selectedHandIndexes.clear();
  state.lastDeltaPaths = [];
  state.pendingTributeGhost = null;
  state.session.lastAppliedSeq = Number(json.seq || 0);
  saveSession();
  render();
}

function canCurrentPlayerAct(kind) {
  if (!isPlayerSession(state.session) || !state.expect) return false;
  if (!state.expect.legalActions || !state.expect.legalActions.includes(kind)) {
    return false;
  }
  const actorIds = getExpectActorIds(state.expect);
  if (actorIds.length) {
    return actorIds.includes(state.session.playerId);
  }
  if (state.expect.kind === "ready" && state.tableState) {
    const mine = getSeatInfoByPlayerId(state.tableState, state.session.playerId);
    return Boolean(mine && !mine.info?.ready);
  }
  return false;
}

async function sendReady() {
  if (!isPlayerSession(state.session)) return;
  state.pendingReadySubmit = true;
  render();
  try {
    await apiFetch(`/api/v1/tables/${encodeURIComponent(state.session.tableId)}/ready`, {
      method: "POST",
      body: JSON.stringify({
        playerId: state.session.playerId,
        playerKey: state.session.playerKey,
        ready: true,
      }),
    });
  } catch (err) {
    state.pendingReadySubmit = false;
    render();
    throw err;
  }
}

async function sendAction(actionType, payload) {
  if (!isPlayerSession(state.session) || !state.tableState) return;
  await apiFetch(
    `/api/v1/tables/${encodeURIComponent(state.session.tableId)}/actions/${actionType}`,
    {
      method: "POST",
      body: JSON.stringify({
        playerId: state.session.playerId,
        playerKey: state.session.playerKey,
        seq: Number(state.tableState.seq || 0),
        ...payload,
      }),
    },
  );
}

function selectedCardsFromPrivate() {
  const hand = state.privateView?.handCards || [];
  return Array.from(state.selectedHandIndexes)
    .sort((a, b) => a - b)
    .map((idx) => hand[idx])
    .filter(Boolean);
}

function resolveSeatByPlayerId(table, playerId) {
  return getSeatInfoByPlayerId(table, playerId)?.seat || "";
}

function getExpectActorIds(expect) {
  if (!expect || typeof expect !== "object") return [];
  const ids = Array.isArray(expect.actorPlayerIds) ? expect.actorPlayerIds : [];
  return ids
    .map((id) => String(id || "").trim())
    .filter(Boolean);
}

function getPrimaryExpectActorId(expect) {
  const ids = getExpectActorIds(expect);
  return ids[0] || "";
}

function resolveTributeContext(actionType, actorPlayerId, actionCard, tableAfterApply) {
  const actorSeat = resolveSeatByPlayerId(tableAfterApply, actorPlayerId);
  const pairs = tableAfterApply?.hand?.tributePlan?.pairs || [];
  let matched = null;
  if (actionType === "tribute") {
    matched = pairs.find((p) => p?.payer === actorSeat && String(p?.paidCard || "").trim() === actionCard)
      || pairs.find((p) => p?.payer === actorSeat)
      || null;
  } else {
    matched = pairs.find(
      (p) => p?.receiver === actorSeat && String(p?.returnCard || "").trim() === actionCard,
    ) || pairs.find((p) => p?.receiver === actorSeat)
      || null;
  }

  const targetSeat = actionType === "tribute"
    ? String(matched?.receiver || "").trim()
    : String(matched?.payer || "").trim();
  return { targetSeat };
}

function deriveTributeGhostFromPlan(table, mySeat) {
  if (!table || !mySeat) return null;
  const pairs = table?.hand?.tributePlan?.pairs || [];
  const matched = pairs.find((p) => {
    const receiver = String(p?.receiver || "").trim();
    const paidCard = String(p?.paidCard || "").trim();
    const returnCard = String(p?.returnCard || "").trim();
    return receiver === mySeat && Boolean(paidCard) && !returnCard;
  });
  const card = String(matched?.paidCard || "").trim();
  if (!card) return null;
  return { card };
}

function shouldShowTributeGhost(table, mySeat) {
  const legalActions = state.expect?.legalActions || [];
  const fromPlan = deriveTributeGhostFromPlan(table, mySeat);
  if (fromPlan) return fromPlan;
  const ghost = state.pendingTributeGhost;
  if (!ghost || !ghost.card) return null;
  const inReturnPhase = state.expect?.kind === "exchange" || legalActions.includes("return_card");
  if (!inReturnPhase) return null;
  return ghost;
}

function consumeActionEvent(trigger, tableAfterApply) {
  if (!trigger || typeof trigger !== "object") return;
  const actionType = String(trigger.actionType || "").trim();
  if (actionType !== "tribute" && actionType !== "return_card") return;
  const actorPlayerId = String(trigger.actorPlayerId || "").trim();
  const card = String(trigger.payload?.card || "").trim();
  if (!card) return;

  const { targetSeat } = resolveTributeContext(
    actionType,
    actorPlayerId,
    card,
    tableAfterApply,
  );

  const mySeat = getSeatInfoByPlayerId(tableAfterApply, state.session?.playerId || "")?.seat || "";
  if (actionType === "tribute" && mySeat && targetSeat === mySeat) {
    state.pendingTributeGhost = {
      card: card,
    };
  } else if (actionType === "return_card" && mySeat && resolveSeatByPlayerId(tableAfterApply, actorPlayerId) === mySeat) {
    state.pendingTributeGhost = null;
  }
}

async function handleTransitionBody(body) {
  if (!state.session || !state.tableState) {
    throw new Error("no local state for transition apply");
  }
  const replayed = stateReplay.applyTransitionBody({
    tableState: state.tableState,
    lastAppliedSeq: state.session.lastAppliedSeq,
    body,
  });
  state.tableState = replayed.tableState;
  state.privateView = isPlayerSession(state.session) ? replayed.privateView : null;
  state.expect = replayed.expect;
  if (state.pendingReadySubmit && state.session?.playerId) {
    const mine = getSeatInfoByPlayerId(replayed.tableState, state.session.playerId);
    if (mine?.info?.ready || state.expect?.kind !== "ready") {
      state.pendingReadySubmit = false;
    }
  }
  state.prompt = replayed.prompt;
  state.lastDeltaPaths = replayed.lastDeltaPaths;
  state.session.lastAppliedSeq = replayed.lastAppliedSeq;
  state.selectedHandIndexes.clear();
  consumeActionEvent(replayed.trigger, replayed.tableState);
  saveSession();

  console.log("[table-transition]", {
    seq: body.seq,
    prevSeq: body.prevSeq,
    prompt: body.prompt || "",
    deltaPaths: state.lastDeltaPaths,
  });
}

async function pollLoop() {
  if (!state.session || state.polling) return;
  state.polling = true;
  state.stopPolling = false;
  setConnection(t("connPolling"));
  while (!state.stopPolling && state.session) {
    try {
      const qs = syncDriver.buildNextstateQuery(state.session);
      const path = `/api/v1/tables/${encodeURIComponent(state.session.tableId)}/nextstate?${qs.toString()}`;
      const { response, json } = await apiFetch(path, { method: "GET" });
      if (response.status === 204) {
        setConnection(t("connPollingHead"));
        continue;
      }
      await handleTransitionBody(json);
      setConnection(tf("connSeq", { seq: state.session.lastAppliedSeq }));
      render();
    } catch (err) {
      if (await recoverWhenServerStateMissing(err)) {
        continue;
      }
      if (await recoverWhenAuthInvalid(err)) {
        continue;
      }
      setConnection(t("connError"));
      setError(err.message || String(err));
      try {
        await bootstrapSnapshot();
      } catch (reErr) {
        if (await recoverWhenServerStateMissing(reErr)) {
          continue;
        }
        if (await recoverWhenAuthInvalid(reErr)) {
          continue;
        }
        setError(`poll error: ${err.message}; resync failed: ${reErr.message}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 700));
    }
  }
  state.polling = false;
  setConnection(t("connIdle"));
}

function startPolling() {
  if (!state.session) return;
  pollLoop();
}

function shouldShowTableScene() {
  return Boolean(state.session?.tableId);
}

function isPortraitPhoneTableMode() {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return false;
  }
  return window.matchMedia(FEED_MODE_MEDIA_QUERY).matches;
}

function shouldUseTrickFeedMode(table, mySeat) {
  return isPortraitPhoneTableMode();
}

function resolveViewCapabilities() {
  const observer = isObserverSession(state.session);
  return {
    isObserver: observer,
    showPrivateHand: !observer,
    canOperate: !observer,
  };
}

function viewportSizeKey() {
  const vv = window.visualViewport;
  const width = vv ? vv.width : window.innerWidth;
  const height = vv ? vv.height : window.innerHeight;
  // In portrait phone table mode, browser UI show/hide can jitter viewport height
  // during scroll. Ignore pure height drift to keep layout stable.
  if (isPortraitPhoneTableMode()) {
    return `${Math.round(width)}xh-stable`;
  }
  return `${Math.round(width)}x${Math.round(height)}`;
}

function computeLayoutRenderKey() {
  return `${shouldShowTableScene()}|${isPortraitPhoneTableMode()}|${viewportSizeKey()}`;
}

function scheduleLayoutRender() {
  const nextKey = computeLayoutRenderKey();
  if (nextKey === lastLayoutRenderKey) return;
  if (layoutRenderFrameId) return;
  layoutRenderFrameId = window.requestAnimationFrame(() => {
    layoutRenderFrameId = 0;
    const frameKey = computeLayoutRenderKey();
    if (frameKey === lastLayoutRenderKey) return;
    render();
  });
}

function scheduleLayoutRenderAfterSettle(delayMs = 180) {
  if (layoutSettleTimer) {
    clearTimeout(layoutSettleTimer);
  }
  layoutSettleTimer = setTimeout(() => {
    layoutSettleTimer = null;
    scheduleLayoutRender();
  }, delayMs);
}

function selectedHandIndexesSignature() {
  return Array.from(state.selectedHandIndexes)
    .sort((a, b) => a - b)
    .join(",");
}

function privateHandContentSignature(cards, tributeGhostCard) {
  return `${cards.join("|")}::${tributeGhostCard || ""}`;
}

function topPlaySignature(topPlay) {
  if (!topPlay) return "none";
  const cards = Array.isArray(topPlay.cards) ? topPlay.cards.join(",") : "";
  return `${topPlay.seat || ""}|${topPlay.combinationType || ""}|${cards}`;
}

function historyTailSignature(history, limit) {
  return history.slice(-limit).map((entry) => {
    const cards = Array.isArray(entry?.cards) ? entry.cards.join(",") : "";
    return `${entry?.seq || ""}|${entry?.seat || ""}|${entry?.actionType || ""}|${cards}`;
  }).join(";");
}

function latestTrickSignature(latestTrickBySeat, topPlaySeat, mySeat, actorSeat) {
  const parts = [`top:${topPlaySeat || ""}`, `my:${mySeat || ""}`, `actor:${actorSeat || ""}`];
  SEATS.forEach((seat) => {
    const entry = latestTrickBySeat.get(seat) || null;
    const cards = Array.isArray(entry?.cards) ? entry.cards.join(",") : "";
    parts.push(`${seat}:${entry?.actionType || ""}:${cards}`);
  });
  return parts.join("|");
}

function trickFeedSignature(table, history, narration, mySeat, isLeadTurn) {
  const trickHistory = sliceCurrentTrickHistory(history);
  const seatSig = SEATS.map((seat) => {
    const info = table?.seats?.[seat] || null;
    return `${seat}:${info?.playerId || ""}:${playerNameText(info)}`;
  }).join("|");
  const trickSig = trickHistory.map((entry) => {
    const cards = Array.isArray(entry?.cards) ? entry.cards.join(",") : "";
    return `${entry?.seq || ""}:${entry?.seat || ""}:${entry?.actionType || ""}:${cards}`;
  }).join(";");
  return `${seatSig}::${mySeat || ""}::${isLeadTurn ? "lead" : "follow"}::${trickSig}::${narration || ""}`;
}

function renderSceneVisibility() {
  const showTable = shouldShowTableScene();
  el.lobbyView.classList.toggle("hidden", showTable);
  el.tableView.classList.toggle("hidden", !showTable);
  syncLobbyAutoRefresh();
}

function getSeatInfoByPlayerId(table, playerId) {
  if (!table || !playerId) return null;
  for (const seat of SEATS) {
    const info = table.seats?.[seat];
    if (info?.playerId === playerId) {
      return { seat, info };
    }
  }
  return null;
}

function getMySeat(table) {
  if (isObserverSession(state.session)) return "S";
  return getSeatInfoByPlayerId(table, state.session?.playerId || "")?.seat || "S";
}

function toRelativeSeat(absSeat, mySeat) {
  const baseIdx = CLOCKWISE_SEATS.indexOf(absSeat);
  const myIdx = CLOCKWISE_SEATS.indexOf(mySeat);
  if (baseIdx < 0 || myIdx < 0) return absSeat;
  const delta = CLOCKWISE_SEATS.indexOf("S") - myIdx;
  return CLOCKWISE_SEATS[(baseIdx + delta + CLOCKWISE_SEATS.length) % CLOCKWISE_SEATS.length];
}

function relativeSeatToCss(relativeSeat) {
  if (relativeSeat === "N") return "north";
  if (relativeSeat === "E") return "east";
  if (relativeSeat === "W") return "west";
  return "south";
}

function playerNameText(info) {
  if (!info) return t("seatEmptyPlayer");
  return info.playerName || t("seatAnonymous");
}

function actorDisplayText(table) {
  const actorId = getPrimaryExpectActorId(state.expect);
  if (!actorId) return t("turnUnknown");
  const actorSeat = getSeatInfoByPlayerId(table, actorId);
  if (!actorSeat) return t("turnUnknown");
  const relativeSeat = toRelativeSeat(actorSeat.seat, getMySeat(table));
  return tf("turnAtSeat", {
    seat: relativeSeat,
    name: actorSeat.info.playerName || "-",
  });
}

function buildFinishRankBySeat(table) {
  const order = table?.hand?.finishingOrder;
  const map = new Map();
  if (!Array.isArray(order)) return map;
  order.forEach((seat, idx) => {
    if (SEATS.includes(seat) && !map.has(seat)) {
      map.set(seat, idx + 1);
    }
  });
  return map;
}

function finishRankText(rank) {
  if (rank === 1) return t("seatFinishRankFirst");
  if (rank === 2) return t("seatFinishRankSecond");
  if (rank === 3) return t("seatFinishRankThird");
  if (rank === 4) return t("seatFinishRankFourth");
  return "";
}

function buildLatestTrickBySeat(history) {
  const latest = new Map();
  for (let i = history.length - 1; i >= 0; i -= 1) {
    const item = history[i];
    if (!item || !SEATS.includes(item.seat)) continue;
    if (latest.has(item.seat)) continue;
    if (item.actionType === "pass") {
      latest.set(item.seat, { actionType: "pass", cards: [] });
    } else if (Array.isArray(item.cards) && item.cards.length) {
      latest.set(item.seat, { actionType: item.actionType || "play", cards: item.cards });
    }
    if (latest.size === SEATS.length) break;
  }
  return latest;
}

function teamForSeat(table, seat) {
  const teams = Array.isArray(table?.teams) ? table.teams : [];
  return teams.find((team) => Array.isArray(team?.seats) && team.seats.includes(seat)) || null;
}

function isHistoryPlayAction(entry) {
  return Boolean(entry && Array.isArray(entry.cards) && entry.cards.length);
}

function countConsecutivePassesBefore(history, index) {
  let count = 0;
  for (let i = index - 1; i >= 0; i -= 1) {
    if (history[i]?.actionType !== "pass") break;
    count += 1;
  }
  return count;
}

function findCurrentTrickStartIndex(history) {
  if (!Array.isArray(history) || !history.length) return 0;
  const startScan = Math.max(0, history.length - TRICK_LOOKBACK_LIMIT);
  for (let i = history.length - 1; i >= startScan; i -= 1) {
    const entry = history[i];
    if (!isHistoryPlayAction(entry)) continue;
    if (i === 0) return i;
    const passCount = countConsecutivePassesBefore(history, i);
    if (passCount >= TRICK_RESET_PASS_COUNT) {
      return i;
    }
  }
  return startScan;
}

function sliceCurrentTrickHistory(history) {
  if (!Array.isArray(history) || !history.length) return [];
  const start = findCurrentTrickStartIndex(history);
  return history.slice(start);
}

function isFriendlySeat(table, mySeat, seat) {
  const myTeam = teamForSeat(table, mySeat);
  if (!myTeam || !Array.isArray(myTeam.seats)) return seat === mySeat;
  return myTeam.seats.includes(seat);
}

function buildTrickFeedPlayerCard(table, seat, mySeat) {
  const info = table?.seats?.[seat] || null;
  const card = document.createElement("div");
  card.className = "trick-feed-player";
  if (seat === mySeat) {
    card.classList.add("is-self");
  }
  const name = document.createElement("div");
  name.className = "trick-feed-player-name";
  name.textContent = playerNameText(info);
  bindIdHovercardTrigger(name, "playerId", info?.playerId || "");
  card.appendChild(name);
  return card;
}

function renderTrickFeed(table, history, narration, mySeat, isLeadTurn) {
  el.trickFeed.innerHTML = "";
  const trickHistory = sliceCurrentTrickHistory(history);
  if (!trickHistory.length) {
    const empty = document.createElement("div");
    empty.className = "trick-feed-empty";
    empty.textContent = t("trickFeedEmpty");
    el.trickFeed.appendChild(empty);
  } else {
    trickHistory.forEach((entry) => {
      const seat = String(entry?.seat || "").trim();
      const friendly = isFriendlySeat(table, mySeat, seat);
      const isSelf = seat === mySeat;
      const line = document.createElement("div");
      line.className = `trick-feed-item ${friendly ? "is-friendly" : "is-opponent"}`;
      if (isSelf) {
        line.classList.add("is-self");
      }
      if (isLeadTurn) {
        line.classList.add("trick-feed-item-historical");
      }

      const bubble = document.createElement("div");
      bubble.className = "trick-feed-bubble";

      if (entry?.actionType === "pass") {
        const pass = document.createElement("div");
        pass.className = "trick-feed-pass";
        pass.textContent = t("pass");
        bubble.appendChild(pass);
      } else if (Array.isArray(entry?.cards) && entry.cards.length) {
        const cardsRow = document.createElement("div");
        cardsRow.className = "card-strip";
        entry.cards.forEach((card) => {
          cardsRow.appendChild(renderCardFace(card, { compact: true }));
        });
        bubble.appendChild(cardsRow);
      }

      line.appendChild(buildTrickFeedPlayerCard(table, seat, mySeat));
      line.appendChild(bubble);
      el.trickFeed.appendChild(line);
    });
  }

  if (narration) {
    const sys = document.createElement("div");
    sys.className = "trick-feed-system";
    sys.textContent = narration;
    el.trickFeed.appendChild(sys);
  }
  // Keep latest trick line visible after DOM writes and after layout settle.
  el.trickFeed.scrollTop = el.trickFeed.scrollHeight;
  window.requestAnimationFrame(() => {
    el.trickFeed.scrollTop = el.trickFeed.scrollHeight;
  });
}

function renderTopPlay(topPlay) {
  el.topPlay.innerHTML = "";
  if (!topPlay) {
    el.topPlay.textContent = t("topPlayNone");
    return;
  }

  const topPlaySummary = document.createElement("div");
  topPlaySummary.className = "mono";
  topPlaySummary.textContent = tf("topPlaySummary", {
    seat: topPlay.seat,
    combinationType: topPlay.combinationType,
    cards: (topPlay.cards || []).join(","),
  });
  el.topPlay.appendChild(topPlaySummary);

  const cardsRow = document.createElement("div");
  cardsRow.className = "card-strip";
  for (const card of topPlay.cards || []) {
    cardsRow.appendChild(renderCardFace(card, { compact: true }));
  }
  el.topPlay.appendChild(cardsRow);
}

function renderHistory(history) {
  el.history.innerHTML = "";
  for (const h of history.slice(-12)) {
    const line = document.createElement("div");
    line.className = "history-line";
    const meta = document.createElement("span");
    meta.className = "history-meta";
    meta.textContent =
      `${h.seq} ${h.seat} ${h.actionType}` +
      (Array.isArray(h.cards) ? ` [${h.cards.join(",")}]` : "");
    line.appendChild(meta);

    if (Array.isArray(h.cards) && h.cards.length) {
      const cardsRow = document.createElement("span");
      cardsRow.className = "card-strip";
      for (const card of h.cards) {
        cardsRow.appendChild(renderCardFace(card, { compact: true }));
      }
      line.appendChild(cardsRow);
    }
    el.history.appendChild(line);
  }
}

function rebuildPrivateHand(cards, tributeGhost) {
  clearHandGroupChainState();
  el.privateHand.innerHTML = "";
  renderCache.privateHandButtons = [];
  const rankGroups = groupContiguousHandCardsByRank(cards);
  if (tributeGhost) {
    const ghostNode = document.createElement("div");
    ghostNode.className = "card-rank-group";
    const ghostCard = document.createElement("button");
    ghostCard.type = "button";
    ghostCard.className = "card card-tribute-highlight card-ghost-floating";
    ghostCard.disabled = true;
    ghostCard.setAttribute("aria-label", tributeGhost.card);
    ghostCard.appendChild(renderCardFace(tributeGhost.card));
    ghostNode.appendChild(ghostCard);
    el.privateHand.appendChild(ghostNode);
  }

  rankGroups.forEach((group) => {
    const groupNode = document.createElement("div");
    groupNode.className = "card-rank-group";
    const groupIndices = group.map(({ idx: groupIdx }) => groupIdx);
    group.forEach(({ card, idx }) => {
      const c = document.createElement("button");
      c.type = "button";
      c.className = "card";
      c.dataset.handIdx = String(idx);
      c.setAttribute("aria-label", card);
      c.appendChild(renderCardFace(card));
      c.addEventListener("click", () => {
        onHandCardClick(idx, groupIndices);
      });
      renderCache.privateHandButtons[idx] = c;
      groupNode.appendChild(c);
    });
    el.privateHand.appendChild(groupNode);
  });
}

function syncPrivateHandSelection() {
  const selectedSig = selectedHandIndexesSignature();
  if (selectedSig === renderCache.privateHandSelectionSig) return;
  renderCache.privateHandButtons.forEach((node, idx) => {
    if (!node) return;
    node.classList.toggle("selected", state.selectedHandIndexes.has(idx));
  });
  renderCache.privateHandSelectionSig = selectedSig;
}

function buildSeatTags(table, seat, info) {
  const tags = [];
  const team = teamForSeat(table, seat);
  const teamLevel = String(team?.level || "").trim();
  const dealerSeat = String(table?.hand?.dealerSeat || "").trim();

  if (teamLevel) {
    tags.push({
      text: tf("tagLevelValue", { level: teamLevel }),
      variant: "seat-tag-level",
    });
  }
  if (info?.playerId && dealerSeat === seat) {
    tags.push({
      text: t("tagDealer"),
      variant: "seat-tag-info",
    });
  }
  return tags;
}

function buildSeatCard(table, seat, info, actorPlayerIds, mySeat, finishRankBySeat) {
  const relativeSeat = toRelativeSeat(seat, mySeat);
  const item = document.createElement("div");
  item.className = `seat-box seat-${relativeSeatToCss(relativeSeat)}`;
  const corner = document.createElement("div");
  corner.className = "seat-corner-marker";
  corner.textContent = seat;
  item.appendChild(corner);
  const isOccupied = Boolean(info?.playerId);
  if (!isOccupied) {
    item.classList.add("seat-box-empty");
    item.dataset.idleWatermark = t("stateIdle");
  }
  if (isOccupied && seat === mySeat) {
    item.classList.add("is-self");
  }
  if (isOccupied && info.playerId && actorPlayerIds.includes(info.playerId)) {
    item.classList.add("current-turn");
  }
  if (!isOccupied) return item;

  const title = document.createElement("div");
  title.className = "seat-title";
  title.textContent = playerNameText(info);
  bindIdHovercardTrigger(title, "playerId", info?.playerId || "");
  const isAway = String(info?.presence || "").toLowerCase() === "away";
  const dot = document.createElement("span");
  dot.className = `seat-ready-dot ${isAway ? "away" : info.ready ? "ready" : ""}`;
  title.appendChild(dot);
  item.appendChild(title);

  const meta = document.createElement("div");
  meta.className = "seat-meta";
  const finishRankLabel = finishRankText(finishRankBySeat.get(seat));
  const isInGame = deriveSimpleTableState(table?.status, table?.phase) === "inGame";
  const remaining = Number(info.remainingCount);
  const shouldShowRemaining = isInGame && Number.isFinite(remaining) && remaining <= 10;
  meta.textContent = shouldShowRemaining
    ? tf("seatMetaRemaining", { remaining: String(remaining) })
    : "";
  item.appendChild(meta);

  const tags = buildSeatTags(table, seat, info);
  if (isAway) {
    tags.push({
      text: t("stateAway"),
      variant: "seat-tag-away",
    });
  }
  if (tags.length) {
    const tagsRow = document.createElement("div");
    tagsRow.className = "seat-tags";
    tags.forEach((tag) => {
      const chip = document.createElement("span");
      chip.className = `seat-tag ${tag.variant || ""}`.trim();
      chip.textContent = tag.text;
      tagsRow.appendChild(chip);
    });
    item.appendChild(tagsRow);
  }

  if (finishRankLabel) {
    const finishTag = document.createElement("div");
    finishTag.className = "seat-finish-rank";
    finishTag.textContent = finishRankLabel;
    item.appendChild(finishTag);
  }
  return item;
}

function renderPortraitSeatOverview(table, mySeat, finishRankBySeat) {
  el.portraitSeatGrid.innerHTML = "";
  for (const seat of SEATS) {
    const info = table.seats?.[seat] || null;
    const card = buildSeatCard(
      table,
      seat,
      info,
      getExpectActorIds(state.expect),
      mySeat,
      finishRankBySeat,
    );
    card.classList.add(`portrait-abs-${String(seat).toLowerCase()}`);
    el.portraitSeatGrid.appendChild(card);
  }
}

function renderTrickLayer(mySeat, latestTrickBySeat, topPlaySeat, isLeadTurn, actorSeat) {
  el.trickBySeat.innerHTML = "";
  for (const seat of SEATS) {
    const relativeSeat = toRelativeSeat(seat, mySeat);
    const group = document.createElement("div");
    group.className = `trick-group trick-${relativeSeatToCss(relativeSeat)}`;
    if (topPlaySeat && seat === topPlaySeat) {
      group.classList.add("trick-group-top");
    }
    const seatAction = latestTrickBySeat.get(seat) || null;
    const cards = seatAction?.actionType === "pass" ? [] : (seatAction?.cards || []);
    const isPass = seatAction?.actionType === "pass";
    const isActorResidual = Boolean(actorSeat) && seat === actorSeat && (isPass || cards.length > 0);
    if (isLeadTurn && (isPass || cards.length > 0)) {
      group.classList.add("trick-group-historical");
    }
    if (isActorResidual) {
      group.classList.add("trick-group-historical");
    }
    if (isPass) {
      group.classList.add("trick-group-pass");
      const passOverlay = document.createElement("div");
      passOverlay.className = "trick-pass-overlay";
      passOverlay.textContent = t("passOverlay");
      group.appendChild(passOverlay);
    } else if (!cards.length) {
      group.classList.add("trick-group-empty");
      const placeholder = document.createElement("div");
      placeholder.className = "card-face card-face-compact card-face-fallback";
      placeholder.textContent = "";
      group.appendChild(placeholder);
    } else {
      cards.forEach((card) => {
        group.appendChild(renderCardFace(card, { compact: true }));
      });
    }
    el.trickBySeat.appendChild(group);
  }
}

function resolveNarrationText(rawNarration) {
  const raw = String(rawNarration || "").trim();
  if (!raw) return "";
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      const lang = window.i18n?.lang === "zh-CN" ? "zh" : "en";
      const preferred = String(parsed[lang] || "").trim();
      if (preferred) return preferred;
      const fallback = String(parsed.zh || parsed.en || "").trim();
      if (fallback) return fallback;
    }
  } catch (_err) {}
  return raw;
}

function debugBoolText(value) {
  return value ? t("debugBoolTrue") : t("debugBoolFalse");
}

function debugSeatPresenceText(info) {
  if (!info?.playerId) return t("stateIdle");
  const presence = String(info?.presence || "").trim();
  if (!presence) {
    return info?.ready ? t("stateReady") : t("stateWaiting");
  }
  const normalized = presence.toLowerCase();
  if (normalized === "away") return t("stateAway");
  if (normalized === "ready") return t("stateReady");
  return presence;
}

function renderPlayersDetail(table, finishRankBySeat) {
  if (!el.tablePlayersDetail) return;
  el.tablePlayersDetail.innerHTML = "";
  for (const seat of SEATS) {
    const info = table?.seats?.[seat] || null;
    const isOccupied = Boolean(info?.playerId);
    const remainingNum = Number(info?.remainingCount);
    const remainingText = isOccupied && Number.isFinite(remainingNum) ? String(remainingNum) : "-";
    const finishRankLabel = finishRankText(finishRankBySeat.get(seat));
    const line = document.createElement("div");
    line.className = "debug-player-line mono";
    line.textContent = tf("debugPlayerLine", {
      seat,
      playerId: isOccupied ? String(info.playerId) : "-",
      playerName: isOccupied ? playerNameText(info) : t("seatEmptyPlayer"),
      playerType: isOccupied ? String(info?.playerType || "-") : "-",
      playerModel: isOccupied ? String(info?.playerModel || "-") : "-",
      ready: isOccupied ? debugBoolText(Boolean(info?.ready)) : "-",
      presence: debugSeatPresenceText(info),
      remaining: remainingText,
      rank: finishRankLabel || "-",
    });
    el.tablePlayersDetail.appendChild(line);
  }
}

function formatDebugJsonScalar(value) {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (typeof value === "string") return `"${value}"`;
  return String(value);
}

function createDebugJsonLine(label, value, className = "", useRawText = false) {
  const line = document.createElement("div");
  line.className = "debug-json-line";
  if (label !== null && label !== undefined) {
    const keyEl = document.createElement("span");
    keyEl.className = "debug-json-key";
    keyEl.textContent = `${label}:`;
    line.appendChild(keyEl);
  }
  const valueEl = document.createElement("span");
  valueEl.className = `debug-json-value ${className}`.trim();
  valueEl.textContent = useRawText ? String(value) : formatDebugJsonScalar(value);
  line.appendChild(valueEl);
  return line;
}

function createDebugJsonNode(label, value, depth = 0) {
  if (value === null || value === undefined || typeof value !== "object") {
    const valueClass = value === null ? "is-null" : typeof value === "string" ? "is-string" : "";
    return createDebugJsonLine(label, value, valueClass);
  }

  const isArray = Array.isArray(value);
  const entries = isArray
    ? value.map((item, index) => [String(index), item])
    : Object.entries(value);
  if (!entries.length) {
    return createDebugJsonLine(label, isArray ? "[]" : "{}", "is-empty", true);
  }

  const details = document.createElement("details");
  details.className = "debug-json-node";
  details.open = depth === 0;
  const summary = document.createElement("summary");
  summary.className = "debug-json-summary";
  const summaryLabel = document.createElement("span");
  summaryLabel.className = "debug-json-key";
  summaryLabel.textContent = String(label ?? "value");
  summary.appendChild(summaryLabel);
  const summaryMeta = document.createElement("span");
  summaryMeta.className = "debug-json-meta";
  summaryMeta.textContent = isArray ? `[${entries.length}]` : `{${entries.length}}`;
  summary.appendChild(summaryMeta);
  details.appendChild(summary);

  const children = document.createElement("div");
  children.className = "debug-json-children";
  entries.forEach(([key, item]) => {
    children.appendChild(createDebugJsonNode(key, item, depth + 1));
  });
  details.appendChild(children);
  return details;
}

function renderLatestStateJson() {
  if (!el.latestStateJsonTree) return;
  el.latestStateJsonTree.innerHTML = "";
  const latestState = {
    tableState: state.tableState,
    privateView: state.privateView,
    expect: state.expect,
    prompt: state.prompt,
    session: state.session,
  };
  const hasAnyData = Object.values(latestState).some((value) => {
    if (value === null || value === undefined) return false;
    if (typeof value === "string") return value.length > 0;
    return true;
  });
  if (!hasAnyData) {
    el.latestStateJsonTree.textContent = t("debugJsonEmpty");
    return;
  }
  Object.entries(latestState).forEach(([key, value]) => {
    el.latestStateJsonTree.appendChild(createDebugJsonNode(key, value));
  });
}

function tableTitleText(tableState, tableItem) {
  const name = String(tableState?.name || tableItem?.name || "").trim();
  if (name) return name;
  const id = String(
    tableState?.tableId
    || tableItem?.state?.tableId
    || tableItem?.tableId
    || "",
  ).trim();
  return id || "-";
}

function cardSymbolToSvgId(cardSymbol) {
  const value = String(cardSymbol || "").trim();
  if (!value) return null;
  if (value === "🃏R") return "joker_red";
  if (value === "🃏b") return "joker_black";

  const suitMap = {
    "♠": "spade",
    "♥": "heart",
    "♦": "diamond",
    "♣": "club",
  };
  const rankMap = {
    A: "1",
    K: "king",
    Q: "queen",
    J: "jack",
    "10": "10",
    "9": "9",
    "8": "8",
    "7": "7",
    "6": "6",
    "5": "5",
    "4": "4",
    "3": "3",
    "2": "2",
  };

  const suit = suitMap[value[0]];
  if (!suit) return null;
  const rank = rankMap[value.slice(1)];
  if (!rank) return null;
  return `${suit}_${rank}`;
}

function cardRankKey(cardSymbol) {
  const symbol = String(cardSymbol || "").trim();
  if (!symbol) return "";
  if (symbol.startsWith("🃏")) return symbol;
  return symbol.slice(1) || symbol;
}

function groupContiguousHandCardsByRank(cards) {
  const groups = [];
  let prevRank = null;
  cards.forEach((card, idx) => {
    const rank = cardRankKey(card);
    if (!groups.length || rank !== prevRank) {
      groups.push([]);
    }
    groups[groups.length - 1].push({ card, idx });
    prevRank = rank;
  });
  return groups;
}

function renderCardFace(cardSymbol, options = {}) {
  const { compact = false } = options;
  const node = document.createElement("span");
  node.className = compact ? "card-face card-face-compact" : "card-face";

  const symbol = String(cardSymbol || "").trim();
  const cardId = cardSymbolToSvgId(symbol);
  if (!cardId) {
    node.classList.add("card-face-fallback");
    node.textContent = symbol || "?";
    return node;
  }

  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "card-svg");
  svg.setAttribute("viewBox", SVG_CARD_VIEWBOX);
  svg.setAttribute("aria-hidden", "true");

  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `${SVG_CARDS_SPRITE_PATH}#${cardId}`);
  svg.appendChild(use);
  node.appendChild(svg);
  return node;
}

function renderTables() {
  el.tablesList.innerHTML = "";
  for (const item of state.tables) {
    const node = document.createElement("div");
    node.className = "table-card";
    const tableState = item.state || {};
    const tableSimpleState = deriveSimpleTableState(tableState.status, tableState.phase);

    const head = document.createElement("div");
    head.className = "table-card-head";
    const tableId = document.createElement("div");
    tableId.className = "table-id";
    const tableDisplayTitle = tableTitleText(tableState, item);
    tableId.textContent = tableDisplayTitle;
    bindIdHovercardTrigger(tableId, "tableId", tableState.tableId || item.tableId || "");
    const badge = document.createElement("div");
    badge.className = "table-badge";
    const badgeText = tf("tableCardBadge", {
      status: formatTableStatusLabel(tableState.status, tableState.phase),
    });
    badge.textContent = badgeText;
    badge.title = badgeText;
    head.appendChild(tableId);
    head.appendChild(badge);
    node.appendChild(head);

    const seatMap = document.createElement("div");
    seatMap.className = "table-seat-map";
    const tableTeams = Array.isArray(tableState.teams) ? tableState.teams : [];
    const initialRank =
      String(tableTeams[0]?.level || "").trim() ||
      String(tableTeams[1]?.level || "").trim() ||
      "2";
    const shouldShowInitialRank = initialRank && initialRank !== "2";
    const layout = [
      "void",
      "N",
      "void",
      "W",
      "void",
      "E",
      "void",
      "S",
      "void",
    ];
    const seats = item.state?.seats || {};
    layout.forEach((pos, index) => {
      const cell = document.createElement("div");
      if (pos === "void") {
        if (index === 4 && shouldShowInitialRank) {
          cell.className = "table-seat-center-rank";
          cell.textContent = tf("tableInitialRank", { rank: initialRank });
        }
        seatMap.appendChild(cell);
        return;
      }
      const player = seats[pos] || null;
      const occupied = Boolean(player?.playerId);
      cell.className = `table-seat-cell ${occupied ? "occupied" : "empty"}`.trim();
      if (!occupied) {
        const seatLabel = document.createElement("div");
        seatLabel.textContent = pos;
        const idle = document.createElement("div");
        idle.className = "seat-status seat-status-idle";
        idle.textContent = t("stateIdle");
        cell.appendChild(seatLabel);
        cell.appendChild(idle);
        cell.addEventListener("click", (ev) => {
          ev.stopPropagation();
          if (tableSimpleState === "waiting") {
            joinTableFromLobby(tableState.tableId || "", pos);
          } else if (tableSimpleState === "inGame") {
            spectateTableFromLobby(tableState.tableId || "");
          } else {
            showActionToast(t("tableStateSimple.finished"));
          }
        });
      } else {
        const seatLabel = document.createElement("div");
        seatLabel.textContent = pos;
        const name = document.createElement("div");
        name.className = "seat-name";
        const displaySeatName = player.playerName || "-";
        name.textContent = displaySeatName;
        bindIdHovercardTrigger(name, "playerId", player.playerId || "");
        const status = document.createElement("div");
        const isAway = String(player?.presence || "").toLowerCase() === "away";
        const ready = Boolean(player.ready);
        status.className = `seat-status ${
          isAway ? "seat-status-away" : ready ? "seat-status-ready" : "seat-status-waiting"
        }`;
        const statusText = isAway ? t("stateAway") : ready ? t("stateReady") : t("stateWaiting");
        status.textContent = statusText;
        status.title = statusText;
        cell.appendChild(seatLabel);
        cell.appendChild(name);
        cell.appendChild(status);
        cell.addEventListener("click", (ev) => {
          ev.stopPropagation();
          showActionToast(t("errSeatOccupied"));
        });
      }
      seatMap.appendChild(cell);
    });
    node.appendChild(seatMap);

    const actions = document.createElement("div");
    actions.className = "table-card-actions";
    const actionBtn = document.createElement("button");
    actionBtn.type = "button";
    if (tableSimpleState === "inGame") {
      actionBtn.textContent = t("spectate");
      actionBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        spectateTableFromLobby(tableState.tableId || "");
      });
    } else if (tableSimpleState === "waiting") {
      actionBtn.textContent = t("quickJoin");
      actionBtn.classList.add("table-card-quick-join");
      actionBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        joinTableFromLobby(tableState.tableId || "", "auto");
      });
    } else {
      actionBtn.disabled = true;
      actionBtn.textContent = t("tableStateSimple.finished");
    }
    const spacer = document.createElement("div");
    spacer.className = "muted";
    spacer.textContent = "";
    actions.appendChild(spacer);
    actions.appendChild(actionBtn);
    node.appendChild(actions);

    el.tablesList.appendChild(node);
  }

  const createCard = document.createElement("button");
  createCard.type = "button";
  createCard.className = "table-card table-card-create";
  createCard.innerHTML = `
    <div class="plus-mark">+</div>
    <div class="create-label">${t("createTableCard")}</div>
    <div class="muted">${t("createTableHint")}</div>
  `;
  createCard.addEventListener("click", () => {
    createTableFromModal().catch((err) => showActionToast(err.message));
  });
  if (!state.tables.length) {
    createCard.classList.add("only-card");
  }
  el.tablesList.appendChild(createCard);

  el.tablesEmptyHint.classList.toggle("hidden", state.tables.length > 0);
  syncIdHovercardAnchor();
}

function render() {
  renderSceneVisibility();
  const s = state.session;
  el.sessionInfo.textContent = s
    ? tf(isObserverSession(s) ? "sessionSummaryObserver" : "sessionSummary", s)
    : t("noSessionYet");
  el.promptInfo.textContent = state.prompt || "";
  const legalActions = state.expect?.legalActions || [];
  const actorIds = getExpectActorIds(state.expect);
  el.expectInfo.textContent = state.expect ? tf("expectSummary", {
    kind: state.expect.kind,
    actor: actorIds.length ? actorIds.join(",") : "-",
    legal: legalActions.join(","),
  }) : "";
  el.tableLegalActions.textContent = legalActions.length
    ? tf("tableLegalActionsSummary", { legal: legalActions.join(" / ") })
    : t("tableLegalActionsNone");

  const table = state.tableState;
  const finishRankBySeat = table ? buildFinishRankBySeat(table) : new Map();
  renderPlayersDetail(table, finishRankBySeat);
  renderLatestStateJson();
  if (!table) {
    clearTableRenderCache();
    el.portraitSeatOverview.classList.add("hidden");
    el.portraitSeatGrid.innerHTML = "";
    el.tableView.classList.remove("is-portrait-layout");
    el.tableStage.classList.remove("is-feed-mode");
    el.trickFeedWrap.classList.add("hidden");
    el.trickFeed.innerHTML = "";
    el.readyFlowRow.classList.add("hidden");
    el.tableNarration.classList.add("hidden");
    el.tableNarration.textContent = "";
    el.tableMeta.textContent = t("noTableSelected");
    el.tableInPlayTitle.textContent = "";
    el.tableInPlayTitle.removeAttribute("title");
    el.tableInPlayTitle.classList.add("hidden");
    el.tableInPlayTitle.setAttribute("aria-hidden", "true");
    el.portraitTableInPlayTitle.textContent = "";
    el.portraitTableInPlayTitle.removeAttribute("title");
    el.portraitTableInPlayTitle.classList.add("hidden");
    el.portraitTableInPlayTitle.setAttribute("aria-hidden", "true");
    hideIdHovercard();
    el.tableTurnInfo.textContent = "";
    el.seatGrid.innerHTML = "";
    el.privateHand.innerHTML = "";
    el.topPlay.textContent = t("topPlayNone");
    el.trickBySeat.innerHTML = "";
    el.history.innerHTML = "";
    el.readyCta.classList.add("hidden");
    el.tributeRow.classList.add("hidden");
    el.returnRow.classList.add("hidden");
    el.playBtn.classList.add("hidden");
    el.passBtn.classList.add("hidden");
    lastLayoutRenderKey = computeLayoutRenderKey();
    return;
  }

  el.tableMeta.textContent = tf("tableMeta", {
    tableId: table.tableId,
    status: formatTableStatusLabel(table.status, table.phase),
  });
  const inPlayTitle = tableTitleText(table, null);
  el.tableInPlayTitle.textContent = inPlayTitle;
  el.portraitTableInPlayTitle.textContent = inPlayTitle;
  bindIdHovercardTrigger(el.tableInPlayTitle, "tableId", table.tableId || "");
  bindIdHovercardTrigger(el.portraitTableInPlayTitle, "tableId", table.tableId || "");
  const mySeat = getMySeat(table);
  const portraitMode = isPortraitPhoneTableMode();
  const feedMode = shouldShowTableScene() && shouldUseTrickFeedMode(table, mySeat);
  if (portraitMode) {
    el.tableInPlayTitle.classList.add("hidden");
    el.tableInPlayTitle.setAttribute("aria-hidden", "true");
    el.portraitTableInPlayTitle.classList.remove("hidden");
    el.portraitTableInPlayTitle.setAttribute("aria-hidden", "false");
  } else {
    el.portraitTableInPlayTitle.classList.add("hidden");
    el.portraitTableInPlayTitle.setAttribute("aria-hidden", "true");
    el.tableInPlayTitle.classList.remove("hidden");
    el.tableInPlayTitle.setAttribute("aria-hidden", "false");
  }
  el.portraitSeatOverview.classList.toggle("hidden", !portraitMode);
  el.tableView.classList.toggle("is-portrait-layout", portraitMode);
  el.tableStage.classList.toggle("is-feed-mode", feedMode);
  el.trickFeedWrap.classList.toggle("hidden", !feedMode);
  const narration = resolveNarrationText(table.narration);
  if (narration && !feedMode) {
    el.tableNarration.textContent = narration;
    el.tableNarration.classList.remove("hidden");
  } else {
    el.tableNarration.textContent = "";
    el.tableNarration.classList.add("hidden");
  }
  el.tableTurnInfo.textContent = actorDisplayText(table);
  if (portraitMode) {
    renderPortraitSeatOverview(table, mySeat, finishRankBySeat);
  } else {
    el.portraitSeatGrid.innerHTML = "";
  }
  el.seatGrid.innerHTML = "";
  for (const seat of SEATS) {
    el.seatGrid.appendChild(buildSeatCard(
      table,
      seat,
      table.seats?.[seat] || null,
      actorIds,
      mySeat,
      finishRankBySeat,
    ));
  }

  const topPlay = table.hand?.topPlay;
  const topPlayCards = topPlay?.cards || [];
  const hasTopPlayCards = Array.isArray(topPlayCards) && topPlayCards.length > 0;
  const isLeadTurn = !topPlay;
  const nextTopPlaySig = topPlaySignature(topPlay);
  if (nextTopPlaySig !== renderCache.topPlaySig) {
    renderTopPlay(topPlay);
    renderCache.topPlaySig = nextTopPlaySig;
  }

  const history = table.hand?.history || [];
  // Main table trick layer should only reflect current trick actions.
  // This clears stale cards from other seats right after a new lead play.
  const latestTrick = buildLatestTrickBySeat(sliceCurrentTrickHistory(history));
  const actorSeat = getSeatInfoByPlayerId(table, getPrimaryExpectActorId(state.expect))?.seat || null;
  const nextTrickLayerSig = latestTrickSignature(latestTrick, topPlay?.seat || null, mySeat, actorSeat);
  if (nextTrickLayerSig !== renderCache.trickLayerSig) {
    renderTrickLayer(mySeat, latestTrick, topPlay?.seat || null, isLeadTurn, actorSeat);
    renderCache.trickLayerSig = nextTrickLayerSig;
  }
  if (feedMode) {
    const nextTrickFeedSig = trickFeedSignature(table, history, narration, mySeat, isLeadTurn);
    if (nextTrickFeedSig !== renderCache.trickFeedSig) {
      renderTrickFeed(table, history, narration, mySeat, isLeadTurn);
      renderCache.trickFeedSig = nextTrickFeedSig;
    }
  } else {
    if (renderCache.trickFeedSig !== null) {
      el.trickFeed.innerHTML = "";
      renderCache.trickFeedSig = null;
    }
  }

  const nextHistorySig = historyTailSignature(history, 12);
  if (nextHistorySig !== renderCache.historySig) {
    renderHistory(history);
    renderCache.historySig = nextHistorySig;
  }

  const caps = resolveViewCapabilities();
  const isObserver = caps.isObserver;
  const cards = isObserver ? [] : (state.privateView?.handCards || []);
  const tributeGhost = shouldShowTributeGhost(table, mySeat);
  const nextPrivateHandContentSig = privateHandContentSignature(cards, tributeGhost?.card || "");
  if (nextPrivateHandContentSig !== renderCache.privateHandContentSig) {
    rebuildPrivateHand(cards, tributeGhost);
    renderCache.privateHandContentSig = nextPrivateHandContentSig;
    renderCache.privateHandSelectionSig = null;
  }
  syncPrivateHandSelection();
  el.privateHandArea?.classList.toggle("hidden", !caps.showPrivateHand);

  const canReady = canCurrentPlayerAct("ready");
  const legalActionsSet = new Set(legalActions);
  const readyStageOnly = state.expect?.kind === "ready"
    || (legalActionsSet.has("ready")
      && !legalActionsSet.has("play")
      && !legalActionsSet.has("pass")
      && !legalActionsSet.has("tribute")
      && !legalActionsSet.has("return_card")
      && !legalActionsSet.has("exchange"));
  const readyAlready = Boolean(table.seats?.[mySeat]?.ready);
  const showReadyCta = canReady && readyStageOnly && !state.pendingReadySubmit && !readyAlready;
  el.readyBtn.disabled = !canReady || state.pendingReadySubmit;
  el.readyFlowBtn.disabled = !canReady || state.pendingReadySubmit;
  const showReadyFlow = showReadyCta;
  el.readyCta.classList.add("hidden");
  el.readyFlowRow.classList.toggle("hidden", !showReadyFlow);
  const canTribute = canCurrentPlayerAct("tribute");
  const canReturnCard = canCurrentPlayerAct("return_card");
  const canPlay = canCurrentPlayerAct("play");
  const canPass = canCurrentPlayerAct("pass") && hasTopPlayCards;

  el.playBtn.disabled = !canPlay;
  el.passBtn.disabled = !canPass;
  el.tributeBtn.disabled = !canTribute;
  el.returnCardBtn.disabled = !canReturnCard;

  el.playBtn.classList.toggle("hidden", !caps.canOperate || !canPlay);
  el.passBtn.classList.toggle("hidden", !caps.canOperate || !canPass);
  el.tributeRow.classList.toggle("hidden", !caps.canOperate || !canTribute);
  el.returnRow.classList.toggle("hidden", !caps.canOperate || !canReturnCard);
  lastLayoutRenderKey = computeLayoutRenderKey();
  syncIdHovercardAnchor();
}

async function init() {
  el.privateHand.addEventListener("dblclick", (ev) => {
    if (ev.target !== el.privateHand) return;
    ev.preventDefault();
    if (!state.selectedHandIndexes.size) return;
    state.selectedHandIndexes.clear();
    syncPrivateHandSelection();
  });
  el.appMetaVersion?.addEventListener("dblclick", (ev) => {
    ev.preventDefault();
    if (!shouldShowTableScene()) return;
    revealDebugPanel();
  });

  document.addEventListener("i18n:changed", () => {
    t = window.i18n && window.i18n.t ? window.i18n.t : (key) => key;
    tf = window.i18n && window.i18n.tf
      ? window.i18n.tf
      : (key, vars) => {
          let msg = t(key);
          if (!vars) return msg;
          Object.keys(vars).forEach((k) => {
            msg = msg.replaceAll(`{${k}}`, String(vars[k]));
          });
          return msg;
        };
    clearTableRenderCache();
    renderTables();
    render();
    if (state.session && state.polling) {
      setConnection(tf("connSeq", { seq: state.session.lastAppliedSeq || 0 }));
    } else {
      setConnection(t("connIdle"));
    }
    renderFooterMeta();
  });

  el.refreshTablesBtn.addEventListener("click", () => {
    refreshLobby().catch((err) => showActionToast(err.message));
  });

  el.playerNameConfirmBtn.addEventListener("click", () => {
    closePlayerNameModal(el.playerNameModalInput.value);
  });
  el.playerNameCancelBtn.addEventListener("click", () => {
    closePlayerNameModal("");
  });
  el.playerNameModal.addEventListener("click", (ev) => {
    if (ev.target === el.playerNameModal) {
      closePlayerNameModal("");
    }
  });
  el.playerNameModalInput.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      closePlayerNameModal(el.playerNameModalInput.value);
      return;
    }
    if (ev.key === "Escape") {
      ev.preventDefault();
      closePlayerNameModal("");
    }
  });

  el.createTableConfirmBtn.addEventListener("click", () => {
    closeCreateTableModal({
      name: el.createTableModalInput.value,
      rank: el.createTableModalRank.value,
    });
  });
  el.createTableCancelBtn.addEventListener("click", () => {
    closeCreateTableModal(null);
  });
  el.createTableModal.addEventListener("click", (ev) => {
    if (ev.target === el.createTableModal) {
      closeCreateTableModal(null);
    }
  });
  el.createTableModalInput.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      closeCreateTableModal({
        name: el.createTableModalInput.value,
        rank: el.createTableModalRank.value,
      });
      return;
    }
    if (ev.key === "Escape") {
      ev.preventDefault();
      closeCreateTableModal(null);
    }
  });

  el.readyBtn.addEventListener("click", () => {
    sendReady().catch((err) => {
      handleRequestError(err);
    });
  });
  el.readyFlowBtn.addEventListener("click", () => {
    sendReady().catch((err) => {
      handleRequestError(err);
    });
  });

  el.tributeBtn.addEventListener("click", () => {
    const cards = selectedCardsFromPrivate();
    if (cards.length !== 1) {
      showActionToast(t("errSelectSingleTributeCard"));
      return;
    }
    sendAction("tribute", { card: cards[0] }).catch((err) => {
      handleRequestError(err);
    });
  });

  el.returnCardBtn.addEventListener("click", () => {
    const cards = selectedCardsFromPrivate();
    if (cards.length !== 1) {
      showActionToast(t("errSelectSingleReturnCard"));
      return;
    }
    sendAction("return_card", { card: cards[0] }).catch((err) => {
      handleRequestError(err);
    });
  });

  el.passBtn.addEventListener("click", () => {
    sendAction("pass", {}).catch((err) => {
      handleRequestError(err);
    });
  });

  el.playBtn.addEventListener("click", () => {
    const cards = selectedCardsFromPrivate();
    if (!cards.length) {
      showActionToast(t("errSelectCards"));
      return;
    }
    sendAction("play", { cards, declaredWildMapping: null }).catch((err) => {
      handleRequestError(err);
    });
  });

  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape" && idHovercardRoot && !idHovercardRoot.classList.contains("hidden")) {
      hideIdHovercard();
      return;
    }
    if (ev.key === "Escape" && playerNameModalResolver) {
      closePlayerNameModal("");
      return;
    }
    if (ev.key === "Escape" && createTableModalResolver) {
      closeCreateTableModal(null);
      return;
    }
    if (ev.key !== "Escape") return;
    if (!state.selectedHandIndexes.size) return;
    state.selectedHandIndexes.clear();
    syncPrivateHandSelection();
  });

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && !state.session) {
      refreshLobbyAuto();
    }
  });
  document.addEventListener("click", (ev) => {
    if (!idHovercardRoot || idHovercardRoot.classList.contains("hidden")) return;
    const target = ev.target;
    if (!(target instanceof Element)) return;
    if (idHovercardRoot.contains(target)) return;
    if (target.closest(".id-hover-trigger")) return;
    hideIdHovercard();
  });

  window.addEventListener("resize", () => {
    scheduleLayoutRender();
    syncIdHovercardAnchor();
  });
  window.addEventListener("scroll", syncIdHovercardAnchor, true);
  window.addEventListener("orientationchange", () => {
    scheduleLayoutRender();
    scheduleLayoutRenderAfterSettle();
    syncIdHovercardAnchor();
  });
  if (window.visualViewport) {
    window.visualViewport.addEventListener("resize", () => {
      scheduleLayoutRender();
      syncIdHovercardAnchor();
    });
  }

  await refreshLobby().catch((err) => setError(err.message));
  startLobbyAutoRefresh();
  if (state.session) {
    await bootstrapSnapshot().catch(async (err) => {
      if (await recoverWhenServerStateMissing(err)) return;
      if (await recoverWhenAuthInvalid(err)) return;
      setError(err.message);
    });
    startPolling();
  }
  render();
  await renderFooterMeta();
}

init();
