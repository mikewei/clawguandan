(function () {
  function toPointerSegments(pointer) {
    if (!pointer.startsWith("/")) {
      throw new Error(`path must start with /: ${pointer}`);
    }
    return pointer
      .slice(1)
      .split("/")
      .map((seg) => seg.replaceAll("~1", "/").replaceAll("~0", "~"));
  }

  function applyReplace(root, pointer, value) {
    const segs = toPointerSegments(pointer);
    let cur = root;
    for (let i = 0; i < segs.length - 1; i += 1) {
      const key = segs[i];
      if (cur == null || typeof cur !== "object" || !(key in cur)) {
        throw new Error(`replace: missing key ${key} for ${pointer}`);
      }
      cur = cur[key];
    }
    const leaf = segs[segs.length - 1];
    if (cur == null || typeof cur !== "object" || !(leaf in cur)) {
      throw new Error(`replace: missing leaf ${leaf} for ${pointer}`);
    }
    cur[leaf] = value;
  }

  function applyAdd(root, pointer, value) {
    if (!pointer.endsWith("/-")) {
      throw new Error(`unsupported add path (must end with /-): ${pointer}`);
    }
    const base = pointer.slice(0, -2);
    const segs = toPointerSegments(base);
    let cur = root;
    for (const seg of segs) {
      if (cur == null || typeof cur !== "object" || !(seg in cur)) {
        throw new Error(`add: missing key ${seg} for ${pointer}`);
      }
      cur = cur[seg];
    }
    if (!Array.isArray(cur)) {
      throw new Error(`add: parent is not array for ${pointer}`);
    }
    cur.push(value);
  }

  function applyDeltaOpsInPlace(tableState, ops) {
    for (const op of ops) {
      if (!op || typeof op !== "object") {
        throw new Error(`invalid op: ${JSON.stringify(op)}`);
      }
      const kind = op.op;
      const path = op.path;
      if (kind !== "replace" && kind !== "add") {
        throw new Error(`unsupported op kind: ${kind}`);
      }
      if (kind === "replace") {
        applyReplace(tableState, path, op.value);
      } else {
        applyAdd(tableState, path, op.value);
      }
    }
  }

  function summarizeDeltaPaths(ops) {
    return (ops || []).map((x) => x.path).slice(0, 8);
  }

  function applyTransitionBody(input) {
    const { tableState, lastAppliedSeq, body } = input || {};
    if (!tableState) {
      throw new Error("no local state for transition apply");
    }
    const expectedPrev = Number(lastAppliedSeq || 0);
    if (Number(body?.prevSeq) !== expectedPrev) {
      throw new Error(`seq gap: transition.prevSeq=${body?.prevSeq}, local=${expectedPrev}`);
    }
    const nextTableState = structuredClone(tableState);
    applyDeltaOpsInPlace(nextTableState, body?.delta?.ops || []);
    return {
      tableState: nextTableState,
      privateView: body?.private || null,
      expect: body?.expect || nextTableState.expect || null,
      prompt: body?.prompt || "",
      lastDeltaPaths: summarizeDeltaPaths(body?.delta?.ops || []),
      lastAppliedSeq: Number(body?.seq || 0),
      trigger: body?.delta?.event?.trigger || null,
    };
  }

  window.stateReplay = {
    applyDeltaOpsInPlace,
    summarizeDeltaPaths,
    applyTransitionBody,
  };
})();
