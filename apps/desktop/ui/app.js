const invoke = window.__TAURI__?.core?.invoke ?? (async () => { throw new Error("Tauri API unavailable"); });
const listen = window.__TAURI__?.event?.listen ?? (async () => () => {});

let snapshot = null;
let activeContactId = null;
let appSurface = "chat";
let pendingOnboardAvatar = null;
let pendingProfileAvatar = null;
let profileRemoveAvatar = false;
let pendingAttachments = [];
let sendInFlight = false;
let editingMessageId = null;
const avatarCache = new Map();
let refreshTimer = null;
let loadingOlder = false;
let hasMoreMessages = true;
const INITIAL_MSG_LIMIT = 30;
const pendingMediaByRow = new WeakMap();
const IS_LINUX = /linux/i.test(navigator.userAgent || "");

const mediaLoadObserver = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    const row = entry.target;
    mediaLoadObserver.unobserve(row);
    const bubble = row.querySelector(".bubble");
    const message = pendingMediaByRow.get(row);
    if (!bubble || !message || bubble.dataset.mediaLoaded) continue;
    bubble.dataset.mediaLoaded = "1";
    loadMessageMedia(message, bubble);
  }
}, { root: null, rootMargin: "240px 0px" });

function $(id) { return document.getElementById(id); }

function show(el) { el.classList.remove("hidden"); }
function hide(el) { el.classList.add("hidden"); }

function openModal(id) { $(id).showModal(); }
function closeModal(id) { $(id).close(); }
window.closeModal = closeModal;

function initials(name) {
  return (name || "?").split(/\s+/).filter(Boolean).map(w => w[0]).join("").slice(0, 2).toUpperCase();
}

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, c => ({ "&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;" }[c]));
}

function formatTime(iso) {
  try {
    return new Date(iso).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  } catch { return ""; }
}

function setAvatarEl(el, name, dataUrl) {
  if (!el) return;
  el.innerHTML = "";
  el.classList.remove("has-image");
  if (dataUrl) {
    const img = document.createElement("img");
    img.src = dataUrl;
    img.alt = name || "";
    el.appendChild(img);
    el.classList.add("has-image");
  } else {
    el.textContent = initials(name);
  }
}

async function replaceMessageAttachment(messageId) {
  const input = document.createElement("input");
  input.type = "file";
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    try {
      const dataBase64 = await readFileAsBase64(file);
      const updated = await invoke("replace_message_attachment", {
        contactId: activeContactId,
        messageId,
        attachment: {
          name: file.name,
          mime: file.type || "application/octet-stream",
          dataBase64,
        },
      });
      removeMessageRow(messageId);
      appendMessage(updated);
      debouncedRefresh();
    } catch (err) {
      alert("Не удалось заменить файл: " + err);
    }
  };
  input.click();
}

function readFileAsBase64(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result || "");
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error || new Error("read failed"));
    reader.readAsDataURL(file);
  });
}

function readImageFile(file) {
  return new Promise((resolve, reject) => {
    if (!file || !file.type.startsWith("image/")) {
      reject(new Error("Выберите изображение"));
      return;
    }
    if (file.size > 512 * 1024) {
      reject(new Error("Файл больше 512 KB"));
      return;
    }
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(new Error("Не удалось прочитать файл"));
    reader.readAsDataURL(file);
  });
}

function previewAvatar(imgEl, fallbackEl, name, dataUrl) {
  if (dataUrl) {
    imgEl.src = dataUrl;
    show(imgEl);
    hide(fallbackEl);
  } else {
    imgEl.removeAttribute("src");
    hide(imgEl);
    show(fallbackEl);
    fallbackEl.textContent = initials(name) || "🐕";
  }
}

function updateProfileFooter() {
  const profile = snapshot?.profile;
  if (!profile) return;
  $("profile-name").textContent = profile.display_name;
  $("profile-id").textContent = `@${profile.user_id}`;
  setAvatarEl($("profile-avatar"), profile.display_name, profile.avatar_data_url);
}

function msgContactId(m) {
  return m?.contact_id ?? m?.contactId ?? "";
}

function msgDirection(m) {
  return m?.direction ?? "";
}

function msgDeletedAt(m) {
  return m?.deleted_at ?? m?.deletedAt ?? null;
}

function msgKind(m) {
  return m?.kind ?? "text";
}

function isOutgoingMessage(m, row) {
  if (row?.classList.contains("out")) return true;
  if (row?.classList.contains("in")) return false;
  return msgDirection(m) === "out";
}

function isMessageDeleted(m) {
  return msgKind(m) === "deleted" || !!msgDeletedAt(m);
}

function pendingStatusLabel() {
  return "ожидает прочтения";
}

function isPendingStatus(status) {
  return status === "queued_local" || status === "queued_mailbox";
}

function isChatConnecting(contactId) {
  if (!contactId || !snapshot) return false;
  if (snapshot.connected_contact_id === contactId) return false;
  return (
    snapshot.connecting_contact_id === contactId ||
    snapshot.wanted_contact_id === contactId
  );
}

function contactConnectionLabel(contactId) {
  if (!snapshot) return "";
  if (snapshot.connected_contact_id === contactId) return "прямое соединение";
  if (isChatConnecting(contactId)) return "подключение WebRTC…";
  if (snapshot.contact_presence?.[contactId]) return "в приложении";
  return `@${contactId}`;
}

function updateComposePlaceholder() {
  const input = $("message-input");
  if (!input) return;
  input.placeholder = pendingAttachments.length
    ? "Подпись к файлу (необязательно)…"
    : "Напишите сообщение…";
}

function setAppSurface(surface) {
  appSurface = surface;
}

async function markContactRead(contactId) {
  if (!contactId) return;
  try {
    await invoke("mark_contact_read", { contactId });
  } catch (err) {
    console.warn("mark_contact_read failed:", err);
  }
  if (snapshot?.unread_by_contact) {
    snapshot.unread_by_contact[contactId] = 0;
  }
  renderContacts();
  korkiNotify?.updateDocumentTitle?.({ snapshot, activeContactId, appSurface, messagesEl: $("messages") });
}

function messagePreviewForContact(contactId) {
  const preview = snapshot?.chat_previews?.[contactId];
  if (!preview) return contactConnectionLabel(contactId);
  const prefix = preview.direction === "out" ? "Вы: " : "";
  return prefix + (preview.preview || "");
}

function sortedContacts() {
  if (!snapshot?.contacts) return [];
  return [...snapshot.contacts].sort((a, b) => {
    const ta = snapshot.chat_previews?.[a.user_id]?.created_at ?? "";
    const tb = snapshot.chat_previews?.[b.user_id]?.created_at ?? "";
    if (ta !== tb) return tb.localeCompare(ta);
    return a.display_name.localeCompare(b.display_name, "ru");
  });
}

function unreadCount(contactId) {
  return snapshot?.unread_by_contact?.[contactId] ?? 0;
}

function showNewMessagesPill() {
  const pill = $("new-messages-pill");
  if (pill) show(pill);
}

function hideNewMessagesPill() {
  const pill = $("new-messages-pill");
  if (pill) hide(pill);
}

async function scrollChatToBottom(markRead = true) {
  const box = $("messages");
  if (!box) return;
  box.scrollTop = box.scrollHeight;
  hideNewMessagesPill();
  if (markRead && activeContactId) await markContactRead(activeContactId);
}

function handleIncomingMessage(msg) {
  const result = korkiNotify.onInboundMessage(msg);
  const cid = msgContactId(msg);

  if (result?.append && cid === activeContactId) {
    appendMessage(msg, result.scroll !== false);
    if (result.showPill) showNewMessagesPill();
    else if (result.scroll !== false) scrollChatToBottom(true);
    refreshChatStatus(true);
    return;
  }

  if (cid === activeContactId) {
    refreshChatStatus(true);
  } else {
    debouncedRefresh();
  }
}

function debouncedRefresh() {
  if (refreshTimer) clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => refresh(), 180);
}

async function ensureContactAvatar(contactId) {
  if (avatarCache.has(contactId)) return avatarCache.get(contactId);
  try {
    const url = await invoke("get_contact_avatar", { contactId });
    avatarCache.set(contactId, url ?? null);
    return url ?? null;
  } catch {
    avatarCache.set(contactId, null);
    return null;
  }
}

function updateOutboxBadge() {
  const badge = $("outbox-badge");
  const count = snapshot?.outbox_count ?? 0;
  if (count > 0) {
    badge.textContent = `${count} ожидает прочтения`;
    show(badge);
  } else {
    hide(badge);
  }
}

async function refresh() {
  try {
    snapshot = await invoke("get_snapshot");
  } catch (err) {
    console.error("get_snapshot failed:", err);
    await showProfileRecovery(err);
    return;
  }
  hide($("onboard-error"));
  if (!snapshot.has_identity) {
    await showOnboardingForNewProfile();
    return;
  }
  hide($("onboarding"));
  show($("app"));
  updateProfileFooter();
  updateOutboxBadge();
  korkiNotify?.updateDocumentTitle?.({ snapshot, activeContactId, appSurface, messagesEl: $("messages") });
  renderInvitations();
  renderContacts();
  updateConnectButton();
  if (activeContactId) {
    await refreshChatStatus();
    const c = snapshot.contacts.find(x => x.user_id === activeContactId);
    if (c) {
      const avatar = await ensureContactAvatar(c.user_id);
      setAvatarEl($("chat-avatar"), c.display_name, avatar);
    }
  }
}

async function showOnboardingForNewProfile() {
  show($("onboarding"));
  hide($("app"));
  show($("btn-restore-identity"));
  try {
    const status = await invoke("get_profile_status");
    const onDisk = status?.identity_on_disk ?? status?.identityOnDisk;
    const loaded = status?.identity_loaded ?? status?.identityLoaded;
    if (onDisk) {
      const uid = status?.user_id ?? status?.userId;
      $("onboard-error").textContent = uid
        ? `Найден сохранённый профиль @${uid}. Вход…`
        : "Найден сохранённый профиль на этом устройстве. Вход…";
      show($("onboard-error"));
      if (!loaded) {
        await restoreExistingProfile();
      }
    } else {
      hide($("onboard-error"));
    }
  } catch (error) {
    console.warn("get_profile_status failed:", error);
  }
}

async function showProfileRecovery(err) {
  show($("onboarding"));
  hide($("app"));
  show($("btn-restore-identity"));
  const errEl = $("onboard-error");
  errEl.textContent = `Не удалось загрузить приложение: ${err}. Пробуем войти в существующий профиль…`;
  show(errEl);
  try {
    const status = await invoke("get_profile_status");
    const onDisk = status?.identity_on_disk ?? status?.identityOnDisk;
    const loaded = status?.identity_loaded ?? status?.identityLoaded;
    if (onDisk && !loaded) {
      await restoreExistingProfile();
    }
  } catch (error) {
    console.warn("get_profile_status failed:", error);
  }
}

async function restoreExistingProfile() {
  try {
    await invoke("restore_identity");
    hide($("onboard-error"));
    await refresh();
  } catch (e) {
    $("onboard-error").textContent = `Не удалось войти: ${e}`;
    show($("onboard-error"));
  }
}

function renderInvitations() {
  const panel = $("invitations-panel");
  const invites = snapshot?.pending_invitations ?? [];
  if (!invites.length) {
    hide(panel);
    panel.innerHTML = "";
    return;
  }
  show(panel);
  panel.innerHTML = `<div class="invitations-title">Приглашения</div>`;
  for (const inv of invites) {
    const row = document.createElement("div");
    row.className = "invitation-row";
    row.innerHTML = `
      <div class="invitation-meta">
        <div class="invitation-name">${escapeHtml(inv.display_name)}</div>
        <div class="invitation-id">@${escapeHtml(inv.from_user_id)}</div>
      </div>
      <div class="invitation-actions">
        <button type="button" class="btn-accent-sm btn-accept" data-id="${escapeHtml(inv.from_user_id)}">Принять</button>
        <button type="button" class="btn-ghost btn-decline" data-id="${escapeHtml(inv.from_user_id)}">×</button>
      </div>`;
    row.querySelector(".btn-accept").onclick = async () => {
      try {
        await invoke("accept_invitation", { fromUserId: inv.from_user_id });
        await refresh();
      } catch (e) {
        alert("Не удалось принять: " + e);
      }
    };
    row.querySelector(".btn-decline").onclick = async () => {
      await invoke("decline_invitation", { fromUserId: inv.from_user_id });
      await refresh();
    };
    panel.appendChild(row);
  }
}

function updateConnectButton() {
  const btn = $("btn-connect");
  if (!btn) return;
  if (snapshot.firebase_configured) {
    btn.classList.add("hidden");
    return;
  }
  btn.classList.remove("hidden");
  btn.textContent = "SDP";
  btn.title = "Ручной обмен SDP";
}

async function setWantedContact(contactId) {
  await invoke("set_wanted_contact", { contactId: contactId ?? null });
}

function contactPreview(c) {
  return messagePreviewForContact(c.user_id);
}

function setChatOpen(open) {
  $("orbit-workspace")?.classList.toggle("chat-open", open);
}

function renderContacts() {
  const list = $("contact-list");
  list.innerHTML = "";
  const q = ($("search-contacts").value || "").toLowerCase();
  for (const c of sortedContacts()) {
    if (q && !c.display_name.toLowerCase().includes(q) && !c.user_id.toLowerCase().includes(q)) continue;
    const unread = unreadCount(c.user_id);
    const previewMeta = snapshot?.chat_previews?.[c.user_id];
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "contact-item" + (c.user_id === activeContactId ? " active" : "") + (unread > 0 ? " has-unread" : "");
    const avatarWrap = document.createElement("div");
    avatarWrap.className = "avatar-wrap";
    const avatar = document.createElement("div");
    avatar.className = "avatar";
    setAvatarEl(avatar, c.display_name, avatarCache.get(c.user_id) ?? null);
    ensureContactAvatar(c.user_id).then((url) => {
      if (activeContactId === c.user_id || snapshot?.contacts?.some(x => x.user_id === c.user_id)) {
        setAvatarEl(avatar, c.display_name, url);
      }
    });
    avatarWrap.appendChild(avatar);
    if (snapshot.contact_presence?.[c.user_id]) {
      const dot = document.createElement("span");
      dot.className = "online-dot";
      dot.title = snapshot.connected_contact_id === c.user_id
        ? "Прямое соединение"
        : "Приложение открыто";
      avatarWrap.appendChild(dot);
    }
    btn.appendChild(avatarWrap);
    const meta = document.createElement("div");
    meta.className = "contact-meta";
    const timeLabel = previewMeta?.created_at ? formatTime(previewMeta.created_at) : "";
    const badgeHtml = unread > 0
      ? `<span class="contact-unread-badge" aria-label="${unread} непрочитанных">${unread > 99 ? "99+" : unread}</span>`
      : "";
    meta.innerHTML = `
      <div class="contact-row-top">
        <div class="contact-name">${escapeHtml(c.display_name)}</div>
        ${timeLabel ? `<span class="contact-time">${escapeHtml(timeLabel)}</span>` : ""}
      </div>
      <div class="contact-preview-row" style="display:flex;align-items:center;justify-content:space-between;gap:0.35rem;min-width:0">
        <div class="contact-preview${unread > 0 ? " unread" : ""}">${escapeHtml(contactPreview(c))}</div>
        ${badgeHtml}
      </div>`;
    btn.appendChild(meta);
    btn.onclick = () => selectContact(c.user_id, c.display_name);
    li.appendChild(btn);
    list.appendChild(li);
  }
}

async function selectContact(id, name) {
  korkiNotify.dismissGroup(id);
  activeContactId = id;
  hideNewMessagesPill();
  setAppSurface("chat");
  hasMoreMessages = true;
  pendingAttachments = [];
  renderAttachPreview();
  renderContacts();
  hide($("empty-state"));
  show($("chat-view"));
  setChatOpen(true);
  $("chat-title").textContent = name;
  setAvatarEl($("chat-avatar"), name, avatarCache.get(id) ?? null);
  ensureContactAvatar(id).then((url) => {
    if (activeContactId === id) setAvatarEl($("chat-avatar"), name, url);
  });

  const wanted = setWantedContact(id);
  const loaded = loadMessages();
  await Promise.all([wanted, loaded]);
  await scrollChatToBottom(true);
  await refreshChatStatus(false);

  if (snapshot?.firebase_configured) {
    invoke("sync_mailbox", { contactId: id })
      .then((incoming) => {
        for (const m of incoming) appendMessage(m);
      })
      .catch(() => {});
  }
}

async function refreshChatStatus(skipContactsRender = false) {
  snapshot = await invoke("get_snapshot");
  const peerOnline = snapshot.contact_presence?.[activeContactId] === true;
  const connected = snapshot.connected_contact_id === activeContactId;
  const connecting = isChatConnecting(activeContactId);

  const onlineDot = $("chat-online-dot");
  if (peerOnline) {
    show(onlineDot);
    onlineDot.title = connected ? "Прямое соединение" : "Приложение открыто";
  } else {
    hide(onlineDot);
  }

  const statusEl = $("chat-status");
  if (connected) {
    statusEl.textContent = "Прямое соединение WebRTC";
  } else if (connecting) {
    statusEl.textContent = peerOnline
      ? "Подключение WebRTC… · mailbox уже работает"
      : "Ожидание собеседника";
  } else if (peerOnline) {
    statusEl.textContent = "В приложении · mailbox · WebRTC не активен";
  } else {
    statusEl.textContent = snapshot.firebase_configured
      ? "Offline · доставка через mailbox"
      : "Не подключено";
  }

  const pill = $("e2e-pill");
  const pillText = $("e2e-pill-text");
  pill.classList.remove("is-live", "is-connecting");
  if (connected) {
    pill.classList.add("is-live");
    pillText.textContent = "E2E · прямое соединение";
  } else if (connecting) {
    pill.classList.add("is-connecting");
    pillText.textContent = "E2E · подключение WebRTC";
  } else {
    pillText.textContent = peerOnline ? "E2E · mailbox" : "Защищено E2E";
  }

  updateConnectButton();
  updateOutboxBadge();
  updateProfileFooter();
  if (!skipContactsRender) renderContacts();
}

async function loadMessages() {
  if (!activeContactId) return;
  hasMoreMessages = true;
  const box = $("messages");
  box.innerHTML = "";
  box.classList.add("is-loading");
  try {
    const msgs = await invoke("get_messages_page", {
      contactId: activeContactId,
      beforeCreatedAt: null,
      limit: INITIAL_MSG_LIMIT,
    });
    hasMoreMessages = msgs.length >= INITIAL_MSG_LIMIT;
    for (const m of msgs) appendMessage(m, false);
    box.scrollTop = box.scrollHeight;
  } finally {
    box.classList.remove("is-loading");
  }
}

async function loadOlderMessages() {
  if (!activeContactId || loadingOlder || !hasMoreMessages) return;
  const box = $("messages");
  const first = box.querySelector(".msg-row");
  if (!first) return;
  const before = first.dataset.createdAt;
  if (!before) return;
  loadingOlder = true;
  try {
    const prevHeight = box.scrollHeight;
    const msgs = await invoke("get_messages_page", {
      contactId: activeContactId,
      beforeCreatedAt: before,
      limit: INITIAL_MSG_LIMIT,
    });
    hasMoreMessages = msgs.length >= INITIAL_MSG_LIMIT;
    for (const m of msgs) appendMessage(m, false, true);
    box.scrollTop = box.scrollHeight - prevHeight;
  } finally {
    loadingOlder = false;
  }
}

async function syncActiveChatMessages() {
  if (!activeContactId) return;
  const msgs = await invoke("get_messages_page", {
    contactId: activeContactId,
    beforeCreatedAt: null,
    limit: 40,
  });
  const box = $("messages");
  const existing = new Set([...box.querySelectorAll(".msg-row")].map(r => r.dataset.msgId));
  for (const m of msgs) {
    if (existing.has(m.id)) {
      updateMessageRecord(m);
    } else {
      appendMessage(m);
    }
  }
}

function updateMessageStatus(msgId, status) {
  const row = $("messages").querySelector(`.msg-row[data-msg-id="${msgId}"]`);
  if (!row) return;
  const pending = isPendingStatus(status);
  const timeEl = row.querySelector(".msg-time");
  if (!timeEl) return;
  const base = timeEl.textContent.split(" · ")[0];
  timeEl.textContent = pending ? `${base} · ${pendingStatusLabel()}` : base;
}

function applyDeletedAppearance(row, m) {
  row.classList.add("msg-deleted");
  const bubble = row.querySelector(".bubble");
  if (!bubble) return;
  bubble.querySelector(".bubble-media")?.remove();
  bubble.querySelector(".bubble-file")?.remove();
  bubble.querySelector(".bubble-caption")?.remove();
  let textEl = bubble.querySelector(".bubble-text");
  if (!textEl) {
    textEl = document.createElement("div");
    textEl.className = "bubble-text";
    bubble.appendChild(textEl);
  }
  textEl.textContent = m.body || "Сообщение удалено";
  bubble.querySelector(".msg-edited")?.remove();
}

function updateMessageRecord(m) {
  const row = $("messages").querySelector(`.msg-row[data-msg-id="${m.id}"]`);
  if (!row) return false;
  if (m.kind === "deleted" || m.deleted_at) {
    applyDeletedAppearance(row, m);
    updateMessageStatus(m.id, m.status);
    return true;
  }
  row.classList.remove("msg-deleted");
  const bubble = row.querySelector(".bubble");
  if (!bubble) return true;
  const kind = m.kind || "text";
  if (kind === "text") {
    let textEl = bubble.querySelector(".bubble-text");
    if (!textEl) {
      textEl = document.createElement("div");
      textEl.className = "bubble-text";
      bubble.appendChild(textEl);
    }
    textEl.textContent = m.body;
  } else {
    const cap = bubble.querySelector(".bubble-caption");
    const caption = mediaCaptionText(m);
    if (caption) {
      if (cap) cap.textContent = caption;
      else {
        const capEl = document.createElement("div");
        capEl.className = "bubble-caption";
        capEl.textContent = caption;
        bubble.prepend(capEl);
      }
    } else if (cap) {
      cap.remove();
    }
  }
  let edited = bubble.querySelector(".msg-edited");
  if (m.edited_at) {
    if (!edited) {
      edited = document.createElement("span");
      edited.className = "msg-edited";
      edited.textContent = "изменено";
      bubble.appendChild(edited);
    }
  } else if (edited) {
    edited.remove();
  }
  updateMessageStatus(m.id, m.status);
  return true;
}

function removeMessageRow(msgId) {
  $("messages").querySelector(`.msg-row[data-msg-id="${msgId}"]`)?.remove();
}

let messageContextMenu = null;

function hideMessageContextMenu() {
  messageContextMenu?.classList.add("hidden");
}

function ensureMessageContextMenu() {
  if (messageContextMenu) return messageContextMenu;
  messageContextMenu = document.createElement("div");
  messageContextMenu.id = "message-context-menu";
  messageContextMenu.className = "message-context-menu hidden";
  document.body.appendChild(messageContextMenu);
  document.addEventListener("click", hideMessageContextMenu);
  document.addEventListener("contextmenu", (e) => {
    if (!messageContextMenu?.contains(e.target)) hideMessageContextMenu();
  });
  return messageContextMenu;
}

function showMessageContextMenu(x, y, m, row) {
  const menu = ensureMessageContextMenu();
  menu.innerHTML = "";
  const outgoing = isOutgoingMessage(m, row);
  const deleted = isMessageDeleted(m);
  const kind = msgKind(m);
  const canEditCaption = outgoing && (kind === "image" || kind === "file") && !deleted;
  if (canEditCaption) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = "Изменить подпись";
    btn.onclick = async (ev) => {
      ev.stopPropagation();
      hideMessageContextMenu();
      const next = prompt("Подпись к файлу", mediaCaptionText(m) || "");
      if (next === null) return;
      try {
        const updated = await invoke("edit_message_caption", {
          contactId: activeContactId,
          messageId: m.id,
          caption: next.trim() || null,
        });
        updateMessageRecord(updated);
      } catch (err) {
        alert("Не удалось изменить подпись: " + err);
      }
    };
    menu.appendChild(btn);
  }
  const canReplace = outgoing && (kind === "image" || kind === "file") && !deleted;
  if (canReplace) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = "Заменить файл";
    btn.onclick = (ev) => {
      ev.stopPropagation();
      hideMessageContextMenu();
      replaceMessageAttachment(m.id);
    };
    menu.appendChild(btn);
  }
  const canEdit = outgoing && kind === "text" && !deleted;
  const canDeleteForAll = outgoing && !deleted;
  if (canEdit) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = "Редактировать";
    btn.onclick = (ev) => {
      ev.stopPropagation();
      hideMessageContextMenu();
      startEditMessage(m);
    };
    menu.appendChild(btn);
  }
  if (canDeleteForAll) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = "Удалить у всех";
    btn.onclick = async (ev) => {
      ev.stopPropagation();
      hideMessageContextMenu();
      if (!confirm("Удалить сообщение у всех?")) return;
      try {
        const updated = await invoke("delete_message", {
          contactId: activeContactId,
          messageId: m.id,
        });
        updateMessageRecord(updated);
      } catch (err) {
        alert("Не удалось удалить: " + err);
      }
    };
    menu.appendChild(btn);
  }
  const hideBtn = document.createElement("button");
  hideBtn.type = "button";
  hideBtn.textContent = "Удалить у меня";
  hideBtn.onclick = async (ev) => {
    ev.stopPropagation();
    hideMessageContextMenu();
    try {
      await invoke("hide_message_for_me", { messageId: m.id });
      removeMessageRow(m.id);
    } catch (err) {
      alert("Не удалось скрыть: " + err);
    }
  };
  menu.appendChild(hideBtn);
  if (!menu.children.length) return;
  menu.classList.remove("hidden");
  menu.style.left = `${Math.min(x, window.innerWidth - 200)}px`;
  menu.style.top = `${Math.min(y, window.innerHeight - 120)}px`;
}

function startEditMessage(m) {
  editingMessageId = m.id;
  const input = $("message-input");
  input.value = m.body;
  input.placeholder = "Редактирование… Enter — сохранить, Esc — отмена";
  input.focus();
}

function cancelEditMessage() {
  editingMessageId = null;
  const input = $("message-input");
  input.value = "";
  input.placeholder = "Напишите сообщение…";
}

function attachMessageContextMenu(row, m) {
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    showMessageContextMenu(e.clientX, e.clientY, m, row);
  });
}

function formatDateLabel(iso) {
  try {
    const d = new Date(iso);
    const today = new Date();
    const sameDay =
      d.getDate() === today.getDate() &&
      d.getMonth() === today.getMonth() &&
      d.getFullYear() === today.getFullYear();
    if (sameDay) return "Сегодня";
    return d.toLocaleDateString("ru-RU", { day: "numeric", month: "long" });
  } catch {
    return "";
  }
}

function ensureDateSeparator(box, iso) {
  const label = formatDateLabel(iso);
  if (!label) return;
  const key = `date-${label}`;
  if (box.querySelector(`[data-date-key="${key}"]`)) return;
  const sep = document.createElement("div");
  sep.className = "msg-date";
  sep.dataset.dateKey = key;
  sep.textContent = label;
  box.appendChild(sep);
}

function fileIconSvg() {
  return `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>`;
}

function zoomIconSvg() {
  return `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3M11 8v6M8 11h6"/></svg>`;
}

function mediaCaptionText(m) {
  const body = (m.body || "").trim();
  const name = (m.attachment_name || "").trim();
  if (!body) return null;
  if (body === name) return null;
  if (name && body.split(", ").every(part => name.includes(part))) return null;
  return body;
}

function createMediaThumb(messageId, att, index) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "media-thumb-btn";
  btn.setAttribute("aria-label", `Открыть ${att.name || "изображение"}`);
  const img = document.createElement("img");
  img.className = "media-thumb";
  img.src = `data:${att.mime};base64,${att.data_base64}`;
  img.alt = att.name || "";
  img.loading = "lazy";
  const hint = document.createElement("span");
  hint.className = "media-thumb-hint";
  hint.innerHTML = zoomIconSvg();
  btn.append(img, hint);
  btn.onclick = (e) => {
    e.stopPropagation();
    openMediaViewer(messageId, index);
  };
  return btn;
}

function createFileChip(messageId, att, m) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "bubble-file media-file-btn";
  btn.innerHTML = `${fileIconSvg()}<span>${escapeHtml(att.name || m.attachment_name || "Файл")}</span>`;
  btn.onclick = () => openMediaViewer(messageId, 0);
  return btn;
}

const mediaViewerState = {
  messageId: null,
  index: 0,
  total: 0,
};

async function openMediaViewer(messageId, index = 0) {
  const viewer = $("media-viewer");
  if (!viewer) return;
  let total = 1;
  try {
    total = await invoke("get_attachment_count", { messageId });
  } catch {
    total = 1;
  }
  mediaViewerState.messageId = messageId;
  mediaViewerState.index = Math.min(index, Math.max(total - 1, 0));
  mediaViewerState.total = Math.max(total, 1);
  viewer.classList.remove("hidden");
  viewer.setAttribute("aria-hidden", "false");
  document.body.classList.add("media-viewer-open");
  await renderMediaViewerSlide();
}

function closeMediaViewer() {
  const viewer = $("media-viewer");
  if (!viewer) return;
  viewer.classList.add("hidden");
  viewer.setAttribute("aria-hidden", "true");
  document.body.classList.remove("media-viewer-open");
  const img = $("media-viewer-img");
  if (img) {
    img.src = "";
    img.classList.add("hidden");
  }
  mediaViewerState.messageId = null;
}

async function renderMediaViewerSlide() {
  const { messageId, index, total } = mediaViewerState;
  if (!messageId) return;

  const title = $("media-viewer-title");
  const counter = $("media-viewer-counter");
  const prev = $("media-viewer-prev");
  const next = $("media-viewer-next");
  const img = $("media-viewer-img");
  const filePanel = $("media-viewer-file");
  const loading = $("media-viewer-loading");

  img.classList.add("hidden");
  filePanel.classList.add("hidden");
  loading.classList.remove("hidden");

  if (total > 1) {
    counter.textContent = `${index + 1} / ${total}`;
    counter.classList.remove("hidden");
    prev.classList.toggle("hidden", index <= 0);
    next.classList.toggle("hidden", index >= total - 1);
  } else {
    counter.classList.add("hidden");
    prev.classList.add("hidden");
    next.classList.add("hidden");
  }

  try {
    const att = await invoke("read_attachment", { messageId, index });
    title.textContent = att.name || "Медиа";
    loading.classList.add("hidden");
    if (att.mime?.startsWith("image/")) {
      img.src = `data:${att.mime};base64,${att.data_base64}`;
      img.alt = att.name || "";
      img.classList.remove("hidden");
    } else {
      $("media-viewer-file-name").textContent = att.name || "Файл";
      filePanel.classList.remove("hidden");
    }
  } catch (err) {
    loading.textContent = "Не удалось загрузить";
    console.warn("media viewer load failed", err);
  }
}

function stepMediaViewer(delta) {
  const nextIndex = mediaViewerState.index + delta;
  if (nextIndex < 0 || nextIndex >= mediaViewerState.total) return;
  mediaViewerState.index = nextIndex;
  renderMediaViewerSlide();
}

async function loadMessageMedia(m, container) {
  const kind = m.kind || "text";
  if (kind === "text") return;

  if (kind === "image" || kind === "file") {
    try {
      const att = await invoke("read_attachment", { messageId: m.id, index: 0 });
      if (kind === "image" && att.mime?.startsWith("image/")) {
        const wrap = document.createElement("div");
        wrap.className = "bubble-media is-single";
        wrap.appendChild(createMediaThumb(m.id, att, 0));
        container.prepend(wrap);
      } else {
        container.prepend(createFileChip(m.id, att, m));
      }
    } catch (err) {
      console.warn("attachment load failed", err);
    }
    return;
  }

  if (kind === "album") {
    const grid = document.createElement("div");
    grid.className = "bubble-media is-album";
    let count = 0;
    try {
      const total = await invoke("get_attachment_count", { messageId: m.id });
      for (let i = 0; i < total; i++) {
        const att = await invoke("read_attachment", { messageId: m.id, index: i });
        if (att.mime?.startsWith("image/")) {
          grid.appendChild(createMediaThumb(m.id, att, i));
          count++;
        }
      }
    } catch (err) {
      console.warn("album load failed", err);
    }
    if (count) container.prepend(grid);
  }
}

function appendMessage(m, scroll = true, prepend = false) {
  const box = $("messages");
  if (box.querySelector(`.msg-row[data-msg-id="${m.id}"]`)) {
    updateMessageRecord(m);
    return;
  }
  if (m.kind === "deleted" || m.deleted_at) {
    ensureDateSeparator(box, m.created_at);
    const row = document.createElement("div");
    row.className = `msg-row ${m.direction === "out" ? "out" : "in"} msg-deleted`;
    row.dataset.msgId = m.id;
    row.dataset.createdAt = m.created_at;
    const bubble = document.createElement("div");
    bubble.className = "bubble";
    const inner = document.createElement("div");
    inner.appendChild(bubble);
    const timeEl = document.createElement("div");
    timeEl.className = "msg-time";
    timeEl.textContent = formatTime(m.created_at);
    inner.appendChild(timeEl);
    row.appendChild(inner);
    applyDeletedAppearance(row, m);
    attachMessageContextMenu(row, m);
    if (prepend) box.prepend(row);
    else box.appendChild(row);
    if (scroll) box.scrollTop = box.scrollHeight;
    return;
  }
  ensureDateSeparator(box, m.created_at);
  const row = document.createElement("div");
  row.className = `msg-row ${m.direction === "out" ? "out" : "in"}`;
  if (m.direction === "out") row.classList.add("msg-enter-out");
  row.dataset.msgId = m.id;
  row.dataset.createdAt = m.created_at;
  const pending = isPendingStatus(m.status);
  const bubble = document.createElement("div");
  bubble.className = "bubble";
  const kind = m.kind || "text";
  if (kind !== "text") {
    const caption = mediaCaptionText(m);
    if (caption) {
      const cap = document.createElement("div");
      cap.className = "bubble-caption";
      cap.textContent = caption;
      bubble.appendChild(cap);
    }
  }
  const inner = document.createElement("div");
  inner.appendChild(bubble);
  const timeEl = document.createElement("div");
  timeEl.className = "msg-time";
  timeEl.textContent = `${formatTime(m.created_at)}${pending ? ` · ${pendingStatusLabel()}` : ""}`;
  inner.appendChild(timeEl);
  row.appendChild(inner);
  if (prepend) {
    const firstRow = box.querySelector(".msg-row");
    if (firstRow) box.insertBefore(row, firstRow);
    else box.appendChild(row);
  } else {
    box.appendChild(row);
  }
  if (kind !== "text") {
    row.classList.add("msg-has-media");
    pendingMediaByRow.set(row, m);
    mediaLoadObserver.observe(row);
  } else {
    const text = document.createElement("div");
    text.className = "bubble-text";
    text.textContent = m.body;
    bubble.appendChild(text);
    if (m.edited_at) {
      const edited = document.createElement("span");
      edited.className = "msg-edited";
      edited.textContent = "изменено";
      bubble.appendChild(edited);
    }
  }
  attachMessageContextMenu(row, m);
  if (scroll) box.scrollTop = box.scrollHeight;
}

$("btn-onboard-avatar").onclick = () => $("input-onboard-avatar").click();
$("input-onboard-avatar").onchange = async (e) => {
  const file = e.target.files?.[0];
  if (!file) return;
  try {
    pendingOnboardAvatar = await readImageFile(file);
    previewAvatar($("onboard-avatar-img"), $("onboard-avatar-fallback"), $("input-display-name").value, pendingOnboardAvatar);
  } catch (err) {
    alert(err.message || err);
  }
  e.target.value = "";
};

$("btn-create-identity").onclick = async () => {
  const userId = $("input-user-id").value.trim();
  const name = $("input-display-name").value.trim();
  if (!userId || !name) return alert("Заполните поля");
  try {
    await invoke("create_identity", { userId, displayName: name });
    if (pendingOnboardAvatar) {
      await invoke("update_profile", { avatarDataUrl: pendingOnboardAvatar });
      pendingOnboardAvatar = null;
    }
    hide($("onboard-error"));
    await refresh();
  } catch (e) {
    $("onboard-error").textContent = String(e);
    show($("onboard-error"));
    try {
      const status = await invoke("get_profile_status");
      if (status?.identity_on_disk ?? status?.identityOnDisk) show($("btn-restore-identity"));
    } catch { /* ignore */ }
  }
};

$("btn-restore-identity").onclick = () => restoreExistingProfile();

$("search-contacts").oninput = renderContacts;
$("btn-add-contact").onclick = () => {
  $("input-contact-id").value = "";
  $("input-bundle").value = "";
  switchAddContactTab("id");
  openModal("modal-add-contact");
};

function switchAddContactTab(mode) {
  const byId = mode === "id";
  $("tab-add-by-id").classList.toggle("active", byId);
  $("tab-add-by-bundle").classList.toggle("active", !byId);
  $("panel-add-by-id").classList.toggle("hidden", !byId);
  $("panel-add-by-bundle").classList.toggle("hidden", byId);
}

$("tab-add-by-id").onclick = () => switchAddContactTab("id");
$("tab-add-by-bundle").onclick = () => switchAddContactTab("bundle");

$("btn-add-by-id").onclick = async () => {
  const userId = $("input-contact-id").value.trim();
  if (!userId) return alert("Введите ID");
  try {
    await invoke("add_contact_by_id", { userId });
    closeModal("modal-add-contact");
    await refresh();
  } catch (e) {
    alert("Не удалось добавить: " + e);
  }
};

$("input-contact-id").onkeydown = (e) => {
  if (e.key === "Enter") $("btn-add-by-id").click();
};

$("btn-save-contact").onclick = async () => {
  const json = $("input-bundle").value.trim();
  if (!json) return;
  await invoke("add_contact", { bundleJson: json });
  $("input-bundle").value = "";
  closeModal("modal-add-contact");
  await refresh();
};

$("btn-show-qr").onclick = async () => {
  const qr = await invoke("get_bundle_qr");
  const container = $("qr-container");
  if (qr.startsWith("data:")) {
    container.innerHTML = `<img src="${qr}" alt="QR" />`;
  } else {
    container.textContent = qr;
  }
  openModal("modal-qr");
};

$("btn-open-profile").onclick = () => {
  const p = snapshot?.profile;
  if (!p) return;
  pendingProfileAvatar = null;
  profileRemoveAvatar = false;
  $("input-profile-name").value = p.display_name;
  $("input-profile-user-id").value = p.user_id;
  previewAvatar($("profile-edit-img"), $("profile-edit-fallback"), p.display_name, p.avatar_data_url);
  setAppSurface("profile");
  openModal("modal-profile");
};

$("btn-profile-avatar").onclick = () => $("input-profile-avatar").click();
$("input-profile-avatar").onchange = async (e) => {
  const file = e.target.files?.[0];
  if (!file) return;
  try {
    pendingProfileAvatar = await readImageFile(file);
    profileRemoveAvatar = false;
    previewAvatar(
      $("profile-edit-img"),
      $("profile-edit-fallback"),
      $("input-profile-name").value,
      pendingProfileAvatar
    );
  } catch (err) {
    alert(err.message || err);
  }
  e.target.value = "";
};

$("btn-remove-avatar").onclick = () => {
  pendingProfileAvatar = null;
  profileRemoveAvatar = true;
  previewAvatar($("profile-edit-img"), $("profile-edit-fallback"), $("input-profile-name").value, null);
};

$("btn-save-profile").onclick = async () => {
  const name = $("input-profile-name").value.trim();
  if (!name) return alert("Введите никнейм");
  try {
    await invoke("update_profile", {
      displayName: name,
      avatarDataUrl: pendingProfileAvatar,
      removeAvatar: profileRemoveAvatar,
    });
    closeModal("modal-profile");
    await refresh();
  } catch (e) {
    alert("Ошибка: " + e);
  }
};

$("btn-connect").onclick = async () => {
  if (!activeContactId || !snapshot || snapshot.firebase_configured) return;
  try {
    const result = await invoke("connect_offer", { contactId: activeContactId });
    $("input-offer-sdp").value = result.offer_sdp;
    $("input-answer-sdp").value = "";
    openModal("modal-connect");
  } catch (e) {
    alert("Ошибка: " + e);
  }
};

$("btn-copy-offer").onclick = () => navigator.clipboard.writeText($("input-offer-sdp").value);

$("btn-finish-connect").onclick = async () => {
  const answer = $("input-answer-sdp").value.trim();
  if (!answer || !activeContactId) return;
  try {
    await invoke("connect_finish", { contactId: activeContactId, answerSdp: answer });
    closeModal("modal-connect");
    await refresh();
    const c = snapshot.contacts.find(x => x.user_id === activeContactId);
    if (c) await selectContact(c.user_id, c.display_name, c.avatar_data_url);
  } catch (e) {
    alert("Ошибка: " + e);
  }
};

$("btn-answer-connect").onclick = async () => {
  const offer = $("input-incoming-offer").value.trim();
  if (!offer || !activeContactId) return;
  try {
    const result = await invoke("connect_answer", { contactId: activeContactId, offerSdp: offer });
    $("input-answer-sdp").value = result.answer_sdp;
    alert("Answer создан — отправьте его собеседнику.");
    closeModal("modal-connect");
    await refresh();
    const c = snapshot.contacts.find(x => x.user_id === activeContactId);
    if (c) await selectContact(c.user_id, c.display_name, c.avatar_data_url);
  } catch (e) {
    alert("Ошибка: " + e);
  }
};

$("btn-safety").onclick = async () => {
  if (!activeContactId) return;
  const num = await invoke("get_safety_number", { contactId: activeContactId });
  $("safety-number").textContent = num;
  openModal("modal-safety");
};

async function readFileAsBase64(file) {
  const maxBytes = 20 * 1024 * 1024;
  if (file.size > maxBytes) {
    throw new Error(`«${file.name || "файл"}» больше 20 МБ`);
  }
  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary);
}

function insertTextAtCursor(text) {
  const input = $("message-input");
  if (!input || !text) return;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  input.value = input.value.slice(0, start) + text + input.value.slice(end);
  const pos = start + text.length;
  input.selectionStart = pos;
  input.selectionEnd = pos;
  input.focus();
}

function filesFromPasteEvent(e) {
  const files = [];
  const items = [...(e.clipboardData?.items || [])];
  for (const item of items) {
    if (item.kind === "file") {
      const file = item.getAsFile();
      if (file) files.push(file);
      continue;
    }
    if (item.type?.startsWith("image/")) {
      const file = item.getAsFile();
      if (file) files.push(file);
    }
  }
  if (!files.length && e.clipboardData?.files?.length) {
    files.push(...e.clipboardData.files);
  }
  return files.filter((file) => file && file.size > 0);
}

function attachmentDtoToFile(item) {
  if (!item?.dataBase64) return null;
  const binary = atob(item.dataBase64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new File([bytes], item.name || "clipboard.bin", {
    type: item.mime || "application/octet-stream",
  });
}

async function readNativeClipboardFiles() {
  try {
    const items = await invoke("read_clipboard_attachments");
    if (!Array.isArray(items) || !items.length) return [];
    return items.map(attachmentDtoToFile).filter(Boolean);
  } catch (err) {
    console.error("read_clipboard_attachments failed:", err);
    return [];
  }
}

async function handleComposePaste(e) {
  if (!activeContactId) return;

  const domFiles = filesFromPasteEvent(e);
  if (domFiles.length) {
    e.preventDefault();
    await addAttachmentsFromFiles(domFiles);
    return;
  }

  if (!IS_LINUX) return;

  e.preventDefault();
  const nativeFiles = await readNativeClipboardFiles();
  if (nativeFiles.length) {
    await addAttachmentsFromFiles(nativeFiles);
    return;
  }

  const text =
    e.clipboardData?.getData("text/plain") ||
    (await invoke("read_clipboard_text").catch(() => null));
  insertTextAtCursor(text || "");
}

function pasteFileName(file, index) {
  if (file.name) return file.name;
  const ext = (file.type || "application/octet-stream").split("/")[1] || "bin";
  return `clipboard-${Date.now()}-${index}.${ext}`;
}

async function addAttachmentsFromFiles(files) {
  let added = false;
  for (const file of files) {
    if (!file) continue;
    if (pendingAttachments.length >= 10) {
      alert("Максимум 10 файлов за раз");
      break;
    }
    try {
      const dataBase64 = await readFileAsBase64(file);
      const previewUrl = (file.type || "").startsWith("image/") ? URL.createObjectURL(file) : null;
      pendingAttachments.push({
        name: pasteFileName(file, pendingAttachments.length),
        mime: file.type || "application/octet-stream",
        dataBase64,
        previewUrl,
      });
      added = true;
    } catch (err) {
      alert(err.message || err);
    }
  }
  if (added) {
    renderAttachPreview();
    $("message-input")?.focus();
  }
  return added;
}

function setSendInFlight(active) {
  sendInFlight = active;
  const btn = $("btn-send");
  const compose = document.querySelector(".compose-inner");
  const bar = document.querySelector(".compose-bar");
  btn?.classList.toggle("is-sending", active);
  btn?.setAttribute("aria-busy", active ? "true" : "false");
  compose?.classList.toggle("is-sending", active);
  bar?.classList.toggle("is-sending", active);
}

function revokeAttachmentPreview(item) {
  if (item?.previewUrl) URL.revokeObjectURL(item.previewUrl);
}

function clearPendingAttachments() {
  for (const item of pendingAttachments) revokeAttachmentPreview(item);
  pendingAttachments = [];
}

function renderAttachPreview() {
  const panel = $("attach-preview");
  if (!pendingAttachments.length) {
    hide(panel);
    panel.classList.remove("is-visible");
    panel.innerHTML = "";
    updateComposePlaceholder();
    return;
  }
  show(panel);
  panel.classList.add("is-visible");
  panel.innerHTML = "";
  updateComposePlaceholder();
  pendingAttachments.forEach((item, index) => {
    const chip = document.createElement("div");
    chip.className = "attach-chip";
    chip.style.animationDelay = `${index * 40}ms`;
    if (item.previewUrl) {
      const img = document.createElement("img");
      img.src = item.previewUrl;
      img.alt = item.name;
      chip.appendChild(img);
    }
    const label = document.createElement("span");
    label.textContent = item.name;
    chip.appendChild(label);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "×";
    remove.onclick = () => {
      revokeAttachmentPreview(pendingAttachments[index]);
      pendingAttachments.splice(index, 1);
      renderAttachPreview();
    };
    chip.appendChild(remove);
    panel.appendChild(chip);
  });
}

$("btn-attach").onclick = () => $("input-attach").click();
$("input-attach").onchange = async (e) => {
  const files = [...(e.target.files || [])];
  e.target.value = "";
  await addAttachmentsFromFiles(files);
};

$("message-input").addEventListener("paste", handleComposePaste);

$("messages").addEventListener("scroll", () => {
  const box = $("messages");
  if (box.scrollTop < 80) loadOlderMessages();
  if (korkiNotify.isAtBottom(box)) {
    hideNewMessagesPill();
    if (activeContactId) void markContactRead(activeContactId);
  }
});

$("new-messages-pill")?.addEventListener("click", () => { void scrollChatToBottom(true); });

$("btn-send").onclick = sendCurrentMessage;
$("message-input").onkeydown = (e) => {
  if (e.key === "Escape" && editingMessageId) {
    e.preventDefault();
    cancelEditMessage();
    return;
  }
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendCurrentMessage();
  }
};

async function sendCurrentMessage() {
  const text = $("message-input").value.trim();
  if (!activeContactId || sendInFlight) return;
  if (editingMessageId) {
    if (!text) return alert("Введите текст");
    setSendInFlight(true);
    try {
      const msg = await invoke("edit_message", {
        contactId: activeContactId,
        messageId: editingMessageId,
        text,
      });
      updateMessageRecord(msg);
      cancelEditMessage();
      debouncedRefresh();
    } catch (e) {
      alert("Не удалось изменить: " + e);
    } finally {
      setSendInFlight(false);
    }
    return;
  }
  if (!text && !pendingAttachments.length) return;
  setSendInFlight(true);
  try {
    let msg;
    if (pendingAttachments.length) {
      msg = await invoke("send_attachments", {
        contactId: activeContactId,
        attachments: pendingAttachments.map(item => ({
          name: item.name,
          mime: item.mime,
          dataBase64: item.dataBase64,
        })),
        caption: text || null,
      });
      clearPendingAttachments();
      renderAttachPreview();
    } else {
      msg = await invoke("send_message", { contactId: activeContactId, text });
    }
    $("message-input").value = "";
    appendMessage(msg);
    debouncedRefresh();
  } catch (e) {
    alert("Не удалось отправить: " + e);
  } finally {
    setSendInFlight(false);
  }
}

$("btn-back-contacts").onclick = () => {
  activeContactId = null;
  hideNewMessagesPill();
  setAppSurface("chat");
  setChatOpen(false);
  hide($("chat-view"));
  show($("empty-state"));
  setWantedContact(null);
  renderContacts();
};

async function openSettingsModal() {
  closeModal("modal-profile");
  setAppSurface("settings");
  snapshot = await invoke("get_snapshot");
  $("input-firebase-url").value = snapshot.firebase_database_url_override ?? "";
  $("input-firebase-url").placeholder = snapshot.firebase_database_url;
  $("input-firebase-token").value = "";
  const hint = $("firebase-default-hint");
  hint.textContent = snapshot.firebase_uses_default_url
    ? `Сейчас: встроенный URL (${snapshot.firebase_database_url})`
    : "Сейчас: свой URL. Очистите поле, чтобы вернуть встроенный.";
  openModal("modal-settings");
}

$("btn-open-settings").onclick = openSettingsModal;
$("btn-open-settings-chip").onclick = openSettingsModal;

$("btn-check-updates").onclick = async () => {
  try {
    const msg = await invoke("check_for_updates_manual");
    alert(msg);
  } catch (err) {
    alert(String(err));
  }
};

$("btn-save-settings").onclick = async () => {
  await invoke("save_config", {
    config: {
      firebase_database_url: $("input-firebase-url").value.trim() || null,
      firebase_auth_token: $("input-firebase-token").value.trim() || null,
    },
  });
  closeModal("modal-settings");
  setAppSurface(activeContactId ? "chat" : "chat");
  await refresh();
};

$("modal-settings")?.addEventListener("close", () => {
  if (!activeContactId) setAppSurface("chat");
  else setAppSurface("chat");
});
$("modal-profile")?.addEventListener("close", () => setAppSurface(activeContactId ? "chat" : "chat"));
$("modal-profile")?.addEventListener("show", () => setAppSurface("profile"));
$("modal-settings")?.addEventListener("show", () => setAppSurface("settings"));

listen("message-received", (e) => {
  handleIncomingMessage(e.payload);
});
listen("message-updated", (e) => {
  const msg = e.payload;
  if (!msg) return;
  if (msgContactId(msg) === activeContactId) {
    if (!updateMessageRecord(msg)) appendMessage(msg, false);
    refreshChatStatus(true);
  }
  debouncedRefresh();
});
listen("message-deleted", (e) => {
  const msg = e.payload;
  if (!msg) return;
  if (msgContactId(msg) === activeContactId) {
    if (!updateMessageRecord(msg)) appendMessage(msg, false);
    refreshChatStatus(true);
  }
  debouncedRefresh();
});
listen("message-hidden", (e) => {
  const id = e.payload?.id;
  if (id) removeMessageRow(id);
});
listen("messages-updated", () => {
  debouncedRefresh();
  if (!activeContactId) return;
  syncActiveChatMessages();
  refreshChatStatus(true);
});
listen("message-sent", (e) => {
  const msg = e.payload;
  if (msg) updateMessageStatus(msg.id, msg.status);
  debouncedRefresh();
});
listen("message-status-updated", (e) => {
  const { id, status } = e.payload ?? {};
  if (id && status) updateMessageStatus(id, status);
  if (activeContactId) refreshChatStatus(true);
});
listen("contacts-updated", async () => {
  if (activeContactId) {
    await refreshChatStatus(false);
    await syncActiveChatMessages();
  }
  debouncedRefresh();
});

$("media-viewer-close")?.addEventListener("click", closeMediaViewer);
$("media-viewer-backdrop")?.addEventListener("click", closeMediaViewer);
$("media-viewer-prev")?.addEventListener("click", () => stepMediaViewer(-1));
$("media-viewer-next")?.addEventListener("click", () => stepMediaViewer(1));
document.addEventListener("keydown", (e) => {
  if ($("media-viewer")?.classList.contains("hidden")) return;
  if (e.key === "Escape") closeMediaViewer();
  if (e.key === "ArrowLeft") stepMediaViewer(-1);
  if (e.key === "ArrowRight") stepMediaViewer(1);
});

korkiNotify.configure({
  stackEl: $("notification-stack"),
  getState: () => ({
    snapshot,
    activeContactId,
    appSurface,
    messagesEl: $("messages"),
    announceInbound: (name, preview) => {
      const live = $("sr-live");
      if (live) live.textContent = `Новое сообщение от ${name}: ${preview}`;
    },
  }),
  onOpenChat: (id, name) => {
    const c = snapshot?.contacts?.find((x) => x.user_id === id);
    selectContact(id, c?.display_name || name);
  },
  onMarkRead: markContactRead,
  onContactsRefresh: () => debouncedRefresh(),
});

refresh();
