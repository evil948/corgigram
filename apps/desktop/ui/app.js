const invoke = window.__TAURI__?.core?.invoke ?? (async () => { throw new Error("Tauri API unavailable"); });
const listen = window.__TAURI__?.event?.listen ?? (async () => () => {});

let snapshot = null;
let activeContactId = null;
let pendingOnboardAvatar = null;
let pendingProfileAvatar = null;
let profileRemoveAvatar = false;
let pendingAttachments = [];
const avatarCache = new Map();
let refreshTimer = null;
let loadingOlder = false;
let hasMoreMessages = true;

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
    badge.textContent = `${count} в очереди`;
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
    show($("onboarding"));
    hide($("app"));
    return;
  }
  if (!snapshot.has_identity) {
    show($("onboarding"));
    hide($("app"));
    return;
  }
  hide($("onboarding"));
  show($("app"));
  updateProfileFooter();
  updateOutboxBadge();
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
  const online = snapshot.contact_presence?.[c.user_id];
  if (online) return "в сети";
  return `@${c.user_id}`;
}

function setChatOpen(open) {
  $("orbit-workspace")?.classList.toggle("chat-open", open);
}

function renderContacts() {
  const list = $("contact-list");
  list.innerHTML = "";
  const q = ($("search-contacts").value || "").toLowerCase();
  for (const c of snapshot.contacts) {
    if (q && !c.display_name.toLowerCase().includes(q) && !c.user_id.toLowerCase().includes(q)) continue;
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "contact-item" + (c.user_id === activeContactId ? " active" : "");
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
      dot.title = "В сети";
      avatarWrap.appendChild(dot);
    }
    btn.appendChild(avatarWrap);
    const meta = document.createElement("div");
    meta.className = "contact-meta";
    meta.innerHTML = `
      <div class="contact-row-top">
        <div class="contact-name">${escapeHtml(c.display_name)}</div>
      </div>
      <div class="contact-preview">${escapeHtml(contactPreview(c))}</div>`;
    btn.appendChild(meta);
    btn.onclick = () => selectContact(c.user_id, c.display_name);
    li.appendChild(btn);
    list.appendChild(li);
  }
}

async function selectContact(id, name) {
  activeContactId = id;
  hasMoreMessages = true;
  pendingAttachments = [];
  renderAttachPreview();
  renderContacts();
  hide($("empty-state"));
  show($("chat-view"));
  setChatOpen(true);
  $("chat-title").textContent = name;
  const avatar = await ensureContactAvatar(id);
  setAvatarEl($("chat-avatar"), name, avatar);
  await setWantedContact(id);
  await refreshChatStatus();
  await loadMessages();
  if (snapshot.firebase_configured) {
    const incoming = await invoke("sync_mailbox", { contactId: id });
    for (const m of incoming) appendMessage(m);
  }
}

async function refreshChatStatus() {
  snapshot = await invoke("get_snapshot");
  const peerOnline = snapshot.contact_presence?.[activeContactId] === true;
  const connected = snapshot.connected_contact_id === activeContactId;
  const connecting = snapshot.connecting_contact_id === activeContactId;

  const onlineDot = $("chat-online-dot");
  if (peerOnline && connected) {
    show(onlineDot);
  } else {
    hide(onlineDot);
  }

  const statusEl = $("chat-status");
  if (connected) {
    statusEl.textContent = "Прямое соединение · в сети";
  } else if (connecting) {
    statusEl.textContent = peerOnline ? "Подключение…" : "Ожидание собеседника";
  } else {
    statusEl.textContent = snapshot.firebase_configured
      ? "Offline mailbox · доставка при появлении в сети"
      : "Не подключено";
  }

  const pill = $("e2e-pill");
  const pillText = $("e2e-pill-text");
  pill.classList.remove("is-live", "is-connecting");
  if (connected) {
    pill.classList.add("is-live");
    pillText.textContent = "Защищено E2E · онлайн";
  } else if (connecting) {
    pill.classList.add("is-connecting");
    pillText.textContent = "Защищено E2E · подключение";
  } else {
    pillText.textContent = "Защищено E2E";
  }

  updateConnectButton();
  updateOutboxBadge();
  updateProfileFooter();
  renderContacts();
}

async function loadMessages() {
  if (!activeContactId) return;
  hasMoreMessages = true;
  const msgs = await invoke("get_messages_page", { contactId: activeContactId, beforeCreatedAt: null, limit: 50 });
  const box = $("messages");
  box.innerHTML = "";
  hasMoreMessages = msgs.length >= 50;
  for (const m of msgs) appendMessage(m, false);
  box.scrollTop = box.scrollHeight;
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
      limit: 50,
    });
    hasMoreMessages = msgs.length >= 50;
    for (const m of msgs) appendMessage(m, false, true);
    box.scrollTop = box.scrollHeight - prevHeight;
  } finally {
    loadingOlder = false;
  }
}

async function syncActiveChatMessages() {
  if (!activeContactId) return;
  const msgs = await invoke("get_messages", { contactId: activeContactId });
  const box = $("messages");
  const existing = new Set([...box.querySelectorAll(".msg-row")].map(r => r.dataset.msgId));
  for (const m of msgs) {
    if (existing.has(m.id)) {
      updateMessageStatus(m.id, m.status);
    } else {
      appendMessage(m);
    }
  }
}

function updateMessageStatus(msgId, status) {
  const row = $("messages").querySelector(`.msg-row[data-msg-id="${msgId}"]`);
  if (!row) return;
  const pending = status === "pending" || status === "queued_local";
  const timeEl = row.querySelector(".msg-time");
  if (!timeEl) return;
  const base = timeEl.textContent.split(" · ")[0];
  timeEl.textContent = pending ? `${base} · в очереди` : base;
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

async function renderMessageMedia(m, container) {
  const kind = m.kind || "text";
  if (kind === "text") return;

  if (kind === "image" || kind === "file") {
    try {
      const att = await invoke("read_attachment", { messageId: m.id, index: 0 });
      if (kind === "image" && att.mime?.startsWith("image/")) {
        const wrap = document.createElement("div");
        wrap.className = "bubble-media";
        const img = document.createElement("img");
        img.src = `data:${att.mime};base64,${att.data_base64}`;
        img.alt = att.name || "изображение";
        wrap.appendChild(img);
        container.prepend(wrap);
      } else {
        const file = document.createElement("div");
        file.className = "bubble-file";
        file.innerHTML = `${fileIconSvg()}<span>${escapeHtml(att.name || m.attachment_name || "Файл")}</span>`;
        container.prepend(file);
      }
    } catch (err) {
      console.warn("attachment load failed", err);
    }
    return;
  }

  if (kind === "album") {
    const grid = document.createElement("div");
    grid.className = "bubble-media";
    for (let i = 0; i < 4; i++) {
      try {
        const att = await invoke("read_attachment", { messageId: m.id, index: i });
        if (att.mime?.startsWith("image/")) {
          const img = document.createElement("img");
          img.src = `data:${att.mime};base64,${att.data_base64}`;
          img.alt = att.name || `фото ${i + 1}`;
          grid.appendChild(img);
        }
      } catch {
        break;
      }
    }
    if (grid.childElementCount) container.prepend(grid);
  }
}

function appendMessage(m, scroll = true, prepend = false) {
  const box = $("messages");
  if (box.querySelector(`.msg-row[data-msg-id="${m.id}"]`)) {
    updateMessageStatus(m.id, m.status);
    return;
  }
  ensureDateSeparator(box, m.created_at);
  const row = document.createElement("div");
  row.className = `msg-row ${m.direction === "out" ? "out" : "in"}`;
  row.dataset.msgId = m.id;
  row.dataset.createdAt = m.created_at;
  const pending = m.status === "pending" || m.status === "queued_local";
  const bubble = document.createElement("div");
  bubble.className = "bubble";
  const caption = (m.kind && m.kind !== "text") ? m.body : escapeHtml(m.body);
  bubble.innerHTML = `<div class="bubble-caption">${caption}</div>`;
  const inner = document.createElement("div");
  inner.appendChild(bubble);
  const timeEl = document.createElement("div");
  timeEl.className = "msg-time";
  timeEl.textContent = `${formatTime(m.created_at)}${pending ? " · в очереди" : ""}`;
  inner.appendChild(timeEl);
  row.appendChild(inner);
  if (prepend) {
    const firstRow = box.querySelector(".msg-row");
    if (firstRow) box.insertBefore(row, firstRow);
    else box.appendChild(row);
  } else {
    box.appendChild(row);
  }
  renderMessageMedia(m, bubble).then(() => {
    if (scroll) box.scrollTop = box.scrollHeight;
  });
  if (scroll && (m.kind || "text") === "text") box.scrollTop = box.scrollHeight;
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
  await invoke("create_identity", { userId, displayName: name });
  if (pendingOnboardAvatar) {
    await invoke("update_profile", { avatarDataUrl: pendingOnboardAvatar });
    pendingOnboardAvatar = null;
  }
  await refresh();
};

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
  if (file.size > 4 * 1024 * 1024) {
    throw new Error(`«${file.name}» больше 4 МБ`);
  }
  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary);
}

function renderAttachPreview() {
  const panel = $("attach-preview");
  if (!pendingAttachments.length) {
    hide(panel);
    panel.innerHTML = "";
    return;
  }
  show(panel);
  panel.innerHTML = "";
  pendingAttachments.forEach((item, index) => {
    const chip = document.createElement("div");
    chip.className = "attach-chip";
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
  for (const file of files) {
    if (pendingAttachments.length >= 10) {
      alert("Максимум 10 файлов за раз");
      break;
    }
    try {
      const dataBase64 = await readFileAsBase64(file);
      const previewUrl = file.type.startsWith("image/") ? URL.createObjectURL(file) : null;
      pendingAttachments.push({
        name: file.name,
        mime: file.type || "application/octet-stream",
        dataBase64,
        previewUrl,
      });
    } catch (err) {
      alert(err.message || err);
    }
  }
  renderAttachPreview();
};

$("messages").addEventListener("scroll", () => {
  const box = $("messages");
  if (box.scrollTop < 80) loadOlderMessages();
});

$("btn-send").onclick = sendCurrentMessage;
$("message-input").onkeydown = (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendCurrentMessage();
  }
};

async function sendCurrentMessage() {
  const text = $("message-input").value.trim();
  if (!activeContactId) return;
  if (!text && !pendingAttachments.length) return;
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
      pendingAttachments = [];
      renderAttachPreview();
    } else {
      msg = await invoke("send_message", { contactId: activeContactId, text });
    }
    $("message-input").value = "";
    appendMessage(msg);
    debouncedRefresh();
  } catch (e) {
    alert("Не удалось отправить: " + e);
  }
}

$("btn-back-contacts").onclick = () => {
  activeContactId = null;
  setChatOpen(false);
  hide($("chat-view"));
  show($("empty-state"));
  setWantedContact(null);
  renderContacts();
};

$("btn-open-settings").onclick = async () => {
  closeModal("modal-profile");
  snapshot = await invoke("get_snapshot");
  $("input-firebase-url").value = snapshot.firebase_database_url_override ?? "";
  $("input-firebase-url").placeholder = snapshot.firebase_database_url;
  $("input-firebase-token").value = "";
  const hint = $("firebase-default-hint");
  hint.textContent = snapshot.firebase_uses_default_url
    ? `Сейчас: встроенный URL (${snapshot.firebase_database_url})`
    : "Сейчас: свой URL. Очистите поле, чтобы вернуть встроенный.";
  openModal("modal-settings");
};

$("btn-save-settings").onclick = async () => {
  await invoke("save_config", {
    config: {
      firebase_database_url: $("input-firebase-url").value.trim() || null,
      firebase_auth_token: $("input-firebase-token").value.trim() || null,
    },
  });
  closeModal("modal-settings");
  await refresh();
};

listen("message-received", (e) => {
  if (e.payload.contact_id === activeContactId) appendMessage(e.payload);
});
listen("message-sent", async () => {
  await refresh();
  await syncActiveChatMessages();
});
listen("contacts-updated", async () => {
  debouncedRefresh();
  if (activeContactId) {
    await refreshChatStatus();
    await syncActiveChatMessages();
  }
});

refresh();
