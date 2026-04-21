(function () {
  const messages = {
    en: {
      appTitle: "Claw Guandan",
      appBrowserTitle: "Claw Guandan",
      connIdle: "Idle",
      connPolling: "Polling",
      connPollingHead: "Polling (head)",
      connError: "Error",
      connSeq: "Seq {seq}",
      lobby: "Lobby",
      joinTable: "Join table",
      tables: "Tables",
      refreshTables: "Manual refresh",
      createTable: "Create table",
      createTableCard: "Create new table",
      createTableHint: "Tap to create",
      createTableDialogTitle: "Create new table",
      createTableDialogHint: "Name is optional. Leave blank to use tableId.",
      createTableRankLabel: "Start rank",
      phTableNameOptional: "Table name (optional)",
      phTableId: "tableId",
      phPlayerName: "playerName",
      auto: "auto",
      joinAsHuman: "Join as human",
      currentSession: "Current session",
      noSessionYet: "No session yet",
      ready: "Ready",
      resyncSnapshot: "Re-sync snapshot",
      clearSession: "Clear session",
      leaveTable: "Leave table",
      tableView: "Table view",
      noTableSelected: "No table selected",
      roundCards: "Round cards",
      trickFeedTitle: "Play area",
      trickFeedEmpty: "Waiting for trick actions...",
      tableHistory: "Table history",
      debugInfo: "Detailed state (debug)",
      action: "Action",
      pass: "Pass",
      passOverlay: "PASS",
      phTributeCard: "tribute card (e.g. ♠A)",
      submitTribute: "Tribute",
      phReturnCard: "return card (e.g. ♦10)",
      submitReturnCard: "Submit return card",
      privateHandHint: "Hand area",
      playSelectedCards: "Play selected cards",
      errTableAndPlayerRequired: "tableId and playerName are required",
      errTributeRequired: "tribute card is required",
      errSelectSingleTributeCard: "select exactly one card to tribute",
      errReturnRequired: "return card is required",
      errSelectSingleReturnCard: "select exactly one card to return",
      errSelectCards: "select at least one card",
      errTableRequired: "tableId is required",
      errSeatOccupied: "This seat is occupied. Choose an empty seat.",
      errPlayerNameRequired: "Please enter player name",
      errRankInvalid: "Invalid rank. Allowed values: 2-10, J, Q, K, A.",
      seatTakenAutoJoinConfirm:
        "This seat is no longer available. Try auto-join this table instead?",
      noTables: "No tables",
      quickJoin: "Quick join",
      tableCardBadge: "{status}",
      tableCardSeq: "",
      tableInitialRank: "Initial rank {rank}",
      "tableStateSimple.waiting": "Waiting",
      "tableStateSimple.inGame": "In game",
      "tableStateSimple.finished": "Finished",
      seatOpen: "{seat}: idle",
      stateIdle: "Idle",
      stateReady: "Ready",
      stateWaiting: "Waiting",
      stateAway: "Away",
      seatEmptyPlayer: "Empty seat",
      seatAnonymous: "Player",
      seatEmptyShort: "{seat} (empty)",
      seatLabelWithName: "{seat} {name}",
      seatWaiting: "Waiting for player",
      tagDeclarer: "Call",
      tagDealer: "D",
      tagTurn: "Turn",
      tagLevelValue: "Lv {level}",
      seatMetaRemaining: "cards left: {remaining}",
      seatFinishRankFirst: "1st out",
      seatFinishRankSecond: "2nd out",
      seatFinishRankThird: "3rd out",
      seatFinishRankFourth: "4th out",
      turnUnknown: "Current turn: waiting",
      turnAtSeat: "Current turn: {seat} {name}",
      tableLegalActionsSummary: "Available actions: {legal}",
      tableLegalActionsNone: "Available actions: none",
      roundCardsEmptySeat: "No cards this round",
      seatEmpty: "{seat}: (empty)",
      seatSummary: "{seat}: {name} [{type}] ready={ready} remaining={remaining}",
      tableRowSummary:
        "{tableId} status={status} phase={phase} seq={seq} | {summary}",
      sessionSummary:
        "tableId={tableId}, playerId={playerId}, playerName={playerName}, lastAppliedSeq={lastAppliedSeq}",
      expectSummary: "expect.kind={kind}, actor={actor}, legal={legal}",
      tableMeta: "Table {tableId} | {status}",
      topPlaySummary:
        "topPlay: seat={seat} type={combinationType} cards={cards}",
      topPlayNone: "topPlay: none",
      serverStateGoneConfirm:
        "The server no longer has your local game state. Clear local session and return to lobby?",
      serverStateGoneHint:
        "Server state is missing. You can retry or clear local session from the debug panel.",
      authInvalidConfirm:
        "Authentication expired or is invalid. Clear local session and return to lobby to re-join?",
      authInvalidHint:
        "Authentication failed. Re-join the table to get a new player key (for example after server restart).",
      playerNameDialogTitle: "Enter player name",
      playerNameDialogHint: "Enter a name before joining a table.",
      confirm: "Confirm",
      cancel: "Cancel",
    },
    "zh-CN": {
      appTitle: "钳子掼蛋",
      appBrowserTitle: "钳子掼蛋",
      connIdle: "空闲",
      connPolling: "轮询中",
      connPollingHead: "轮询中（无新状态）",
      connError: "错误",
      connSeq: "序列 {seq}",
      lobby: "大厅",
      joinTable: "加入牌桌",
      tables: "牌桌列表",
      refreshTables: "手动刷新",
      createTable: "创建牌桌",
      createTableCard: "创建新牌桌",
      createTableHint: "点击即可创建",
      createTableDialogTitle: "创建新牌桌",
      createTableDialogHint: "牌桌名可选。留空确认则使用 tableId。",
      createTableRankLabel: "起始级别",
      phTableNameOptional: "牌桌名称（可选）",
      phTableId: "牌桌ID",
      phPlayerName: "玩家名",
      auto: "自动",
      joinAsHuman: "以人类玩家加入",
      currentSession: "当前会话",
      noSessionYet: "暂无会话",
      ready: "准备",
      resyncSnapshot: "重新同步快照",
      clearSession: "清除会话",
      leaveTable: "离开牌桌",
      tableView: "牌桌视图",
      noTableSelected: "尚未选择牌桌",
      roundCards: "本轮出牌",
      trickFeedTitle: "出牌区",
      trickFeedEmpty: "等待本墩出牌...",
      tableHistory: "对局历史",
      debugInfo: "详细状态（调试）",
      action: "操作",
      pass: "过牌",
      passOverlay: "过牌",
      phTributeCard: "进贡牌（例如 ♠A）",
      submitTribute: "进贡",
      phReturnCard: "还贡牌（例如 ♦10）",
      submitReturnCard: "提交还贡",
      privateHandHint: "手牌区",
      playSelectedCards: "打出所选牌",
      errTableAndPlayerRequired: "tableId 和 playerName 为必填项",
      errTributeRequired: "请输入进贡牌",
      errSelectSingleTributeCard: "请选择且仅选择一张牌用于进贡",
      errReturnRequired: "请输入还贡牌",
      errSelectSingleReturnCard: "请选择且仅选择一张牌用于还贡",
      errSelectCards: "请至少选择一张牌",
      errTableRequired: "tableId 为必填项",
      errSeatOccupied: "该座位已有人，请选择空位",
      errPlayerNameRequired: "请输入玩家名",
      errRankInvalid: "无效级别。可选值：2-10、J、Q、K、A。",
      seatTakenAutoJoinConfirm: "该座位已不可用，是否改为自动加入该牌桌？",
      noTables: "暂无牌桌",
      quickJoin: "快速加入",
      tableCardBadge: "{status}",
      tableCardSeq: "",
      tableInitialRank: "初始级别 {rank}",
      "tableStateSimple.waiting": "等待中",
      "tableStateSimple.inGame": "游戏中",
      "tableStateSimple.finished": "游戏结束",
      seatOpen: "{seat}: 空闲",
      stateIdle: "空闲",
      stateReady: "已准备",
      stateWaiting: "等待中",
      stateAway: "离开",
      seatEmptyPlayer: "空位",
      seatAnonymous: "玩家",
      seatEmptyShort: "{seat}（空）",
      seatLabelWithName: "{seat} {name}",
      seatWaiting: "等待玩家入座",
      tagDeclarer: "主叫",
      tagDealer: "庄",
      tagTurn: "出",
      tagLevelValue: "{level}级",
      seatMetaRemaining: "余牌: {remaining}",
      seatFinishRankFirst: "头游",
      seatFinishRankSecond: "二游",
      seatFinishRankThird: "三游",
      seatFinishRankFourth: "末游",
      turnUnknown: "当前轮次：等待中",
      turnAtSeat: "当前轮到：{seat} {name}",
      tableLegalActionsSummary: "可执行操作：{legal}",
      tableLegalActionsNone: "可执行操作：暂无",
      roundCardsEmptySeat: "本轮未出牌",
      seatEmpty: "{seat}:（空）",
      seatSummary: "{seat}: {name} [{type}] 准备={ready} 余牌={remaining}",
      tableRowSummary:
        "{tableId} 状态={status} 阶段={phase} 序列={seq} | {summary}",
      sessionSummary:
        "tableId={tableId}, playerId={playerId}, playerName={playerName}, lastAppliedSeq={lastAppliedSeq}",
      expectSummary: "expect.kind={kind}, actor={actor}, legal={legal}",
      tableMeta: "牌桌 {tableId} | {status}",
      topPlaySummary:
        "顶牌: seat={seat} 牌型={combinationType} 牌={cards}",
      topPlayNone: "顶牌: 无",
      serverStateGoneConfirm:
        "服务端已没有你的本地对局状态。是否清空本地会话并返回大厅？",
      serverStateGoneHint:
        "服务端状态缺失。你可以重试，或在调试面板中清空本地会话。",
      authInvalidConfirm:
        "身份认证已失效或无效。是否清空本地会话并返回大厅重新加入？",
      authInvalidHint:
        "认证失败。请重新加入牌桌以获取新的 playerKey（例如服务端重启后）。",
      playerNameDialogTitle: "输入玩家名",
      playerNameDialogHint: "加入牌桌前请先输入玩家名。",
      confirm: "确认",
      cancel: "取消",
    },
  };

  const STORAGE_KEY = "clawguandan-lang";

  function detectLang() {
    return "en";
  }

  let currentLang = (() => {
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved === "en" || saved === "zh-CN") return saved;
    } catch (_err) {}
    return detectLang();
  })();

  let dict = messages[currentLang] || messages.en;

  function t(key) {
    return dict[key] != null ? dict[key] : messages.en[key] != null ? messages.en[key] : key;
  }

  function tf(key, vars) {
    let msg = t(key);
    if (!vars) return msg;
    Object.keys(vars).forEach((k) => {
      msg = msg.replaceAll(`{${k}}`, String(vars[k]));
    });
    return msg;
  }

  function applyTranslations() {
    document.documentElement.lang = currentLang === "zh-CN" ? "zh-CN" : "en";
    const titleEl = document.querySelector("title");
    if (titleEl) titleEl.textContent = t("appBrowserTitle");

    document.querySelectorAll("[data-i18n]").forEach((node) => {
      const key = node.getAttribute("data-i18n");
      const val = t(key);
      if (node.getAttribute("data-i18n-placeholder")) {
        node.placeholder = val;
      } else {
        node.textContent = val;
      }
    });

    document.querySelectorAll(".lang-btn").forEach((btn) => {
      const lang = btn.getAttribute("data-lang");
      btn.classList.toggle("active", lang === currentLang);
    });
  }

  function setLang(lang) {
    if (lang !== "en" && lang !== "zh-CN") return;
    currentLang = lang;
    dict = messages[currentLang] || messages.en;
    try {
      localStorage.setItem(STORAGE_KEY, lang);
    } catch (_err) {}
    window.i18n.lang = currentLang;
    applyTranslations();
    try {
      document.dispatchEvent(new CustomEvent("i18n:changed"));
    } catch (_err) {}
  }

  window.i18n = { t, tf, lang: currentLang, setLang };

  document.querySelectorAll(".lang-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      setLang(btn.getAttribute("data-lang"));
    });
  });

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", applyTranslations);
  } else {
    applyTranslations();
  }
})();
