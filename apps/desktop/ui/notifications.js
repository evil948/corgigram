/**
 * korki messenger notification hub — in-app stack + optional OS notifications.
 * Idempotent by message ID; groups rapid messages per contact.
 */
(function initKorkiNotifications(global) {
  const NOTIFY_GROUP_MS = 3200;
  const STACK_MAX = 4;
  const SCROLL_BOTTOM_THRESHOLD = 80;

  const seenMessageIds = new Set();
  const groups = new Map();
  let stackEl = null;
  let windowFocused = true;
  let documentVisible = true;
  let getState = () => ({});
  let onOpenChat = () => {};
  let onMarkRead = () => {};
  let onContactsRefresh = () => {};

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    }[c]));
  }

  function previewText(msg) {
    const kind = msg.kind || "text";
    if (kind === "image") return "Фото";
    if (kind === "album") return "Альбом";
    if (kind === "file") {
      const name = msg.attachment_name || msg.attachmentName;
      return name ? `Файл · ${name}` : "Файл";
    }
    const body = (msg.body || "").trim();
    return body.length > 72 ? `${body.slice(0, 72)}…` : body;
  }

  function isAtBottom(box) {
    if (!box) return true;
    return box.scrollHeight - box.scrollTop - box.clientHeight < SCROLL_BOTTOM_THRESHOLD;
  }

  function shouldSuppressPopup(msg, state) {
    const cid = msg.contact_id ?? msg.contactId;
    if (msg.direction === "out") return true;
    if (state.appSurface !== "chat") return false;
    if (cid !== state.activeContactId) return false;
    if (!state.messagesEl) return false;
    return isAtBottom(state.messagesEl) && windowFocused && documentVisible;
  }

  function totalUnread(state) {
    const map = state.snapshot?.unread_by_contact ?? {};
    return Object.values(map).reduce((a, b) => a + (b || 0), 0);
  }

  function updateDocumentTitle(state) {
    const n = totalUnread(state);
    document.title = n > 0 ? `(${n}) korki` : "korki";
  }

  function renderStack() {
    if (!stackEl) return;
    stackEl.innerHTML = "";
    const items = [...groups.values()]
      .sort((a, b) => b.lastAt - a.lastAt)
      .slice(0, STACK_MAX);
    for (const g of items) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "notify-card";
      card.dataset.contactId = g.contactId;
      const countLabel = g.count > 1 ? `<span class="notify-count">${g.count}</span>` : "";
      const time = g.lastMsg?.created_at ? formatTime(g.lastMsg.created_at) : "";
      card.innerHTML = `
        <div class="notify-card-top">
          <span class="notify-sender">${escapeHtml(g.displayName)}</span>
          ${countLabel}
          <span class="notify-time">${escapeHtml(time)}</span>
        </div>
        <div class="notify-preview">${escapeHtml(g.preview)}</div>`;
      card.onclick = () => {
        dismissGroup(g.contactId);
        onOpenChat(g.contactId, g.displayName);
      };
      stackEl.appendChild(card);
    }
    stackEl.classList.toggle("hidden", items.length === 0);
  }

  function formatTime(iso) {
    try {
      return new Date(iso).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
    } catch {
      return "";
    }
  }

  function dismissGroup(contactId) {
    const g = groups.get(contactId);
    if (g?.timer) clearTimeout(g.timer);
    groups.delete(contactId);
    renderStack();
  }

  function dismissAll() {
    for (const g of groups.values()) {
      if (g.timer) clearTimeout(g.timer);
    }
    groups.clear();
    renderStack();
  }

  function enqueueInApp(contactId, displayName, msg) {
    const existing = groups.get(contactId);
    const preview = previewText(msg);
    const now = Date.now();
    if (existing) {
      existing.count += 1;
      existing.lastMsg = msg;
      existing.preview = existing.count > 1
        ? `${existing.count} новых сообщений`
        : preview;
      existing.lastAt = now;
      if (existing.timer) clearTimeout(existing.timer);
      existing.timer = setTimeout(() => dismissGroup(contactId), NOTIFY_GROUP_MS);
    } else {
      groups.set(contactId, {
        contactId,
        displayName,
        count: 1,
        preview,
        lastMsg: msg,
        lastAt: now,
        timer: setTimeout(() => dismissGroup(contactId), NOTIFY_GROUP_MS),
      });
    }
    renderStack();
  }

  function maybeOsNotify(contactId, displayName, msg, count) {
    if (!global.Notification || Notification.permission !== "granted") return;
    if (windowFocused && documentVisible) return;
    const title = count > 1 ? `${displayName} (${count})` : displayName;
    try {
      const n = new Notification(title, {
        body: count > 1 ? `${count} новых сообщений` : previewText(msg),
        tag: `korki-${contactId}`,
        silent: false,
      });
      n.onclick = () => {
        global.focus();
        dismissGroup(contactId);
        onOpenChat(contactId, displayName);
        n.close();
      };
    } catch {
      /* WebView may block */
    }
  }

  async function ensureOsPermission() {
    if (!global.Notification) return;
    if (Notification.permission === "default") {
      try {
        await Notification.requestPermission();
      } catch {
        /* ignore */
      }
    }
  }

  function onInboundMessage(msg) {
    if (!msg?.id || msg.direction === "out") return;
    if (seenMessageIds.has(msg.id)) return;
    seenMessageIds.add(msg.id);
    if (seenMessageIds.size > 5000) {
      const drop = [...seenMessageIds].slice(0, 1000);
      drop.forEach((id) => seenMessageIds.delete(id));
    }

    const state = getState();
    const cid = msg.contact_id ?? msg.contactId;
    const contact = state.snapshot?.contacts?.find((c) => c.user_id === cid);
    const displayName = contact?.display_name || cid;

    updateDocumentTitle(state);

    if (shouldSuppressPopup(msg, state)) {
      onMarkRead(cid);
      return { handled: "active-visible", append: true, scroll: true };
    }

    if (state.appSurface === "chat" && cid === state.activeContactId) {
      onContactsRefresh();
      return { handled: "active-scrolled", append: true, scroll: false, showPill: true };
    }

    enqueueInApp(cid, displayName, msg);
    const g = groups.get(cid);
    maybeOsNotify(cid, displayName, msg, g?.count ?? 1);
    onContactsRefresh();

    if (state.announceInbound) {
      state.announceInbound(displayName, previewText(msg));
    }

    return { handled: "notified", append: false };
  }

  function configure(opts) {
    stackEl = opts.stackEl;
    getState = opts.getState;
    onOpenChat = opts.onOpenChat;
    onMarkRead = opts.onMarkRead;
    onContactsRefresh = opts.onContactsRefresh;
    global.addEventListener("focus", () => { windowFocused = true; });
    global.addEventListener("blur", () => { windowFocused = false; });
    document.addEventListener("visibilitychange", () => {
      documentVisible = !document.hidden;
      if (documentVisible) dismissAll();
    });
    ensureOsPermission();
  }

  global.korkiNotify = {
    configure,
    onInboundMessage,
    dismissAll,
    dismissGroup,
    previewText,
    isAtBottom,
    updateDocumentTitle,
    seenMessageIds,
  };
})(window);
