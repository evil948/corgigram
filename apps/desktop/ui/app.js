const invoke = window.__TAURI__?.core?.invoke ?? (async () => { throw new Error("Tauri API unavailable"); });
const listen = window.__TAURI__?.event?.listen ?? (async () => () => {});

let snapshot = null;
let activeContactId = null;
let pendingOnboardAvatar = null;
let pendingProfileAvatar = null;
let profileRemoveAvatar = false;

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
  renderContacts();
  updateConnectButton();
  if (activeContactId) {
    await refreshChatStatus();
    const c = snapshot.contacts.find(x => x.user_id === activeContactId);
    if (c) setAvatarEl($("chat-avatar"), c.display_name, c.avatar_data_url);
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
  btn.textContent = "Подключиться (SDP)";
  btn.title = "Ручной обмен SDP";
}

async function setWantedContact(contactId) {
  await invoke("set_wanted_contact", { contactId: contactId ?? null });
}

function renderContacts() {
  const list = $("contact-list");
  list.innerHTML = "";
  const q = ($("search-contacts").value || "").toLowerCase();
  for (const c of snapshot.contacts) {
    if (q && !c.display_name.toLowerCase().includes(q) && !c.user_id.toLowerCase().includes(q)) continue;
    const li = document.createElement("li");
    li.className = "contact-item" + (c.user_id === activeContactId ? " active" : "");
    const avatar = document.createElement("div");
    avatar.className = "avatar";
    setAvatarEl(avatar, c.display_name, c.avatar_data_url);
    li.appendChild(avatar);
    const meta = document.createElement("div");
    meta.className = "contact-meta";
    meta.innerHTML = `
      <div class="contact-name">${escapeHtml(c.display_name)}</div>
      <div class="contact-preview">@${escapeHtml(c.user_id)}</div>`;
    li.appendChild(meta);
    li.onclick = () => selectContact(c.user_id, c.display_name, c.avatar_data_url);
    list.appendChild(li);
  }
}

async function selectContact(id, name, avatarUrl = null) {
  activeContactId = id;
  renderContacts();
  hide($("empty-state"));
  show($("chat-view"));
  $("chat-title").textContent = name;
  setAvatarEl($("chat-avatar"), name, avatarUrl);
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
  const connected = snapshot.connected_contact_id === activeContactId;
  const connecting = snapshot.connecting_contact_id === activeContactId;
  $("chat-status").innerHTML = connected
    ? `<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2z"/></svg> Защищено E2E · онлайн`
    : connecting
      ? `<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2z"/></svg> Защищено E2E · подключение…`
      : `<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2z"/></svg> Защищено E2E · ${snapshot.firebase_configured ? "offline mailbox" : "не подключено"}`;
  updateConnectButton();
  updateOutboxBadge();
  updateProfileFooter();
}

async function loadMessages() {
  if (!activeContactId) return;
  const msgs = await invoke("get_messages", { contactId: activeContactId });
  const box = $("messages");
  box.innerHTML = "";
  for (const m of msgs) appendMessage(m, false);
  box.scrollTop = box.scrollHeight;
}

function appendMessage(m, scroll = true) {
  const box = $("messages");
  const row = document.createElement("div");
  row.className = `msg-row ${m.direction === "out" ? "out" : "in"}`;
  const pending = m.status === "pending" || m.status === "queued_local";
  row.innerHTML = `
    <div>
      <div class="bubble">${escapeHtml(m.body)}</div>
      <div class="msg-time">${formatTime(m.created_at)}${pending ? " · ⏳" : ""}</div>
    </div>`;
  box.appendChild(row);
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

$("btn-send").onclick = sendCurrentMessage;
$("message-input").onkeydown = (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendCurrentMessage();
  }
};

async function sendCurrentMessage() {
  const text = $("message-input").value.trim();
  if (!text || !activeContactId) return;
  try {
    const msg = await invoke("send_message", { contactId: activeContactId, text });
    $("message-input").value = "";
    appendMessage(msg);
    await refresh();
  } catch (e) {
    alert("Не удалось отправить: " + e);
  }
}

$("btn-settings").onclick = async () => {
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
listen("message-sent", async () => { await refresh(); });
listen("contacts-updated", async () => {
  await refresh();
  if (activeContactId) await refreshChatStatus();
});

refresh();
