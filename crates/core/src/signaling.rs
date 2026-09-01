use anyhow::{Context, Result};
use corgigram_crypto::PreKeyBundle;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Deserialize)]
pub struct SignalingSdp {
    pub sdp: String,
    pub from: String,
    pub ts: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MailboxEntry {
    pub ciphertext: String,
    pub from: String,
    pub ts: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AvatarEntry {
    pub ciphertext: String,
    pub mime: String,
    pub ts: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DirectoryEntry {
    pub bundle: PreKeyBundle,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectPing {
    pub from: String,
    pub ts: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeliveryAck {
    pub from: String,
    pub ts: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PresenceEntry {
    pub online: bool,
    pub ts: i64,
}

impl PresenceEntry {
    pub fn is_online(&self) -> bool {
        if !self.online {
            return false;
        }
        let now = chrono::Utc::now().timestamp_millis();
        now - self.ts < 90_000
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MailboxPing {
    pub from: String,
    pub msg_id: String,
    pub ts: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InvitationEntry {
    pub from: String,
    pub display_name: String,
    pub ts: i64,
}

#[derive(Clone)]
pub struct FirebaseSignaling {
    client: Client,
    base_url: String,
    auth_token: Option<String>,
}

impl FirebaseSignaling {
    pub fn new(database_url: &str, auth_token: Option<String>) -> Self {
        let base_url = database_url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            base_url,
            auth_token,
        }
    }

    fn url(&self, path: &str) -> String {
        let mut url = format!("{}/{}.json", self.base_url, path.trim_start_matches('/'));
        if let Some(token) = &self.auth_token {
            url.push_str(&format!("?auth={token}"));
        }
        url
    }

    pub async fn publish_offer(&self, target_user_id: &str, from_user_id: &str, offer_sdp: &str) -> Result<()> {
        self.client
            .put(self.url(&format!("signaling/{target_user_id}/offer")))
            .json(&json!({
                "sdp": offer_sdp,
                "from": from_user_id,
                "ts": chrono::Utc::now().timestamp()
            }))
            .send()
            .await
            .context("firebase publish offer")?
            .error_for_status()
            .context("firebase publish offer status")?;
        Ok(())
    }

    pub async fn fetch_offer(&self, user_id: &str) -> Result<Option<SignalingSdp>> {
        self.fetch_json(&self.url(&format!("signaling/{user_id}/offer"))).await
    }

    pub async fn wait_offer(&self, user_id: &str, timeout_secs: u64) -> Result<SignalingSdp> {
        for _ in 0..timeout_secs * 2 {
            if let Some(offer) = self.fetch_offer(user_id).await? {
                return Ok(offer);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        anyhow::bail!("timed out waiting for firebase offer")
    }

    pub async fn publish_answer(&self, target_user_id: &str, from_user_id: &str, answer_sdp: &str) -> Result<()> {
        self.client
            .put(self.url(&format!("signaling/{target_user_id}/answer")))
            .json(&json!({
                "sdp": answer_sdp,
                "from": from_user_id,
                "ts": chrono::Utc::now().timestamp()
            }))
            .send()
            .await
            .context("firebase publish answer")?
            .error_for_status()
            .context("firebase publish answer status")?;
        Ok(())
    }

    pub async fn fetch_answer(&self, user_id: &str, from_user_id: &str) -> Result<Option<SignalingSdp>> {
        if let Some(answer) = self.fetch_json::<SignalingSdp>(&self.url(&format!("signaling/{user_id}/answer"))).await? {
            if answer.from == from_user_id {
                return Ok(Some(answer));
            }
        }
        Ok(None)
    }

    pub async fn wait_answer(&self, user_id: &str, from_user_id: &str, timeout_secs: u64) -> Result<String> {
        for _ in 0..timeout_secs * 2 {
            if let Some(answer) = self.fetch_answer(user_id, from_user_id).await? {
                return Ok(answer.sdp);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        anyhow::bail!("timed out waiting for firebase answer")
    }

    pub async fn clear_signaling(&self, user_id: &str) -> Result<()> {
        let _ = self.client.delete(self.url(&format!("signaling/{user_id}/offer"))).send().await;
        let _ = self
            .client
            .delete(self.url(&format!("signaling/{user_id}/answer")))
            .send()
            .await;
        let _ = self
            .client
            .delete(self.url(&format!("signaling/{user_id}/ice")))
            .send()
            .await;
        Ok(())
    }

    pub async fn publish_ice_candidate(
        &self,
        target_user_id: &str,
        from_user_id: &str,
        candidate_id: &str,
        candidate_json: &str,
    ) -> Result<()> {
        self.client
            .put(self.url(&format!(
                "signaling/{target_user_id}/ice/{from_user_id}/{candidate_id}"
            )))
            .json(&json!({
                "c": candidate_json,
                "ts": chrono::Utc::now().timestamp()
            }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn list_ice_candidates(
        &self,
        user_id: &str,
        from_user_id: &str,
    ) -> Result<Vec<(String, String)>> {
        let url = self.url(&format!("signaling/{user_id}/ice/{from_user_id}"));
        let mut last_err = None;
        for attempt in 0..3 {
            match self.client.get(&url).send().await {
                Ok(resp) => {
                    if resp.status().as_u16() == 404 {
                        return Ok(vec![]);
                    }
                    let value: serde_json::Value = resp.error_for_status()?.json().await?;
                    let Some(obj) = value.as_object() else {
                        return Ok(vec![]);
                    };
                    let mut out = Vec::new();
                    for (id, entry) in obj {
                        if let Some(c) = entry.get("c").and_then(|v| v.as_str()) {
                            out.push((id.clone(), c.to_string()));
                        }
                    }
                    return Ok(out);
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < 2 {
                        tokio::time::sleep(std::time::Duration::from_millis(300 * (attempt + 1)))
                            .await;
                    }
                }
            }
        }
        Err(last_err.unwrap().into())
    }

    pub async fn publish_mailbox(
        &self,
        recipient_id: &str,
        msg_id: &str,
        from_user_id: &str,
        ciphertext: &[u8],
    ) -> Result<()> {
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ciphertext);
        self.client
            .put(self.url(&format!("mailboxes/{recipient_id}/{msg_id}")))
            .json(&json!({
                "ciphertext": encoded,
                "from": from_user_id,
                "ts": chrono::Utc::now().timestamp()
            }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn list_mailbox(&self, user_id: &str) -> Result<Vec<(String, MailboxEntry)>> {
        let url = self.url(&format!("mailboxes/{user_id}"));
        let resp = self.client.get(&url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        let Some(obj) = value.as_object() else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for (id, entry) in obj {
            if let Ok(parsed) = serde_json::from_value::<MailboxEntry>(entry.clone()) {
                out.push((id.clone(), parsed));
            }
        }
        out.sort_by_key(|(_, e)| e.ts);
        Ok(out)
    }

    pub async fn delete_mailbox(&self, user_id: &str, msg_id: &str) -> Result<()> {
        let _ = self
            .client
            .delete(self.url(&format!("mailboxes/{user_id}/{msg_id}")))
            .send()
            .await;
        Ok(())
    }

    /// Lightweight ping so the recipient's client knows to pull mailbox immediately.
    pub async fn publish_mailbox_ping(
        &self,
        recipient_id: &str,
        from_user_id: &str,
        msg_id: &str,
    ) -> Result<()> {
        self.client
            .put(self.url(&format!("mailbox_pings/{recipient_id}/{from_user_id}")))
            .json(&json!({
                "from": from_user_id,
                "msg_id": msg_id,
                "ts": chrono::Utc::now().timestamp_millis()
            }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn list_mailbox_pings(&self, user_id: &str) -> Result<Vec<(String, MailboxPing)>> {
        let url = self.url(&format!("mailbox_pings/{user_id}"));
        let resp = self.client.get(&url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        let Some(obj) = value.as_object() else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for (from_id, entry) in obj {
            if let Ok(parsed) = serde_json::from_value::<MailboxPing>(entry.clone()) {
                out.push((from_id.clone(), parsed));
            }
        }
        out.sort_by_key(|(_, p)| p.ts);
        Ok(out)
    }

    pub async fn publish_delivery_ack(
        &self,
        sender_id: &str,
        msg_id: &str,
        from_user_id: &str,
    ) -> Result<()> {
        self.client
            .put(self.url(&format!("delivery_acks/{sender_id}/{msg_id}")))
            .json(&json!({
                "from": from_user_id,
                "ts": chrono::Utc::now().timestamp_millis()
            }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn list_delivery_acks(&self, user_id: &str) -> Result<Vec<(String, DeliveryAck)>> {
        let url = self.url(&format!("delivery_acks/{user_id}"));
        let resp = self.client.get(&url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        let Some(obj) = value.as_object() else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for (id, entry) in obj {
            if let Ok(parsed) = serde_json::from_value::<DeliveryAck>(entry.clone()) {
                out.push((id.clone(), parsed));
            }
        }
        out.sort_by_key(|(_, ack)| ack.ts);
        Ok(out)
    }

    pub async fn delete_delivery_ack(&self, user_id: &str, msg_id: &str) -> Result<()> {
        let _ = self
            .client
            .delete(self.url(&format!("delivery_acks/{user_id}/{msg_id}")))
            .send()
            .await;
        Ok(())
    }

    pub async fn delete_mailbox_ping(&self, recipient_id: &str, from_user_id: &str) -> Result<()> {
        let _ = self
            .client
            .delete(self.url(&format!("mailbox_pings/{recipient_id}/{from_user_id}")))
            .send()
            .await;
        Ok(())
    }

    pub async fn publish_presence(&self, user_id: &str, online: bool) -> Result<()> {
        self.client
            .put(self.url(&format!("presence/{user_id}")))
            .json(&json!({
                "online": online,
                "ts": chrono::Utc::now().timestamp_millis()
            }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn fetch_presence(&self, user_id: &str) -> Result<Option<PresenceEntry>> {
        self.fetch_json(&self.url(&format!("presence/{user_id}"))).await
    }

    pub async fn publish_connect_ping(&self, recipient_id: &str, from_user_id: &str) -> Result<()> {
        self.client
            .put(self.url(&format!("connect_pings/{recipient_id}/{from_user_id}")))
            .json(&json!({
                "from": from_user_id,
                "ts": chrono::Utc::now().timestamp_millis()
            }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn list_connect_pings(&self, user_id: &str) -> Result<Vec<(String, ConnectPing)>> {
        let url = self.url(&format!("connect_pings/{user_id}"));
        let resp = self.client.get(&url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        let Some(obj) = value.as_object() else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for (from_id, entry) in obj {
            if let Ok(parsed) = serde_json::from_value::<ConnectPing>(entry.clone()) {
                out.push((from_id.clone(), parsed));
            }
        }
        out.sort_by_key(|(_, p)| p.ts);
        Ok(out)
    }

    pub async fn delete_connect_ping(&self, recipient_id: &str, from_user_id: &str) -> Result<()> {
        let _ = self
            .client
            .delete(self.url(&format!("connect_pings/{recipient_id}/{from_user_id}")))
            .send()
            .await;
        Ok(())
    }

    /// Public pre-key bundle for contact discovery by user ID (no secret keys).
    pub async fn publish_directory(&self, user_id: &str, bundle: &PreKeyBundle) -> Result<()> {
        self.client
            .put(self.url(&format!("directory/{user_id}")))
            .json(&json!({
                "bundle": bundle,
                "updated_at": chrono::Utc::now().timestamp()
            }))
            .send()
            .await
            .context("firebase publish directory")?
            .error_for_status()
            .context("firebase publish directory status")?;
        Ok(())
    }

    pub async fn fetch_directory(&self, user_id: &str) -> Result<Option<PreKeyBundle>> {
        let entry = self
            .fetch_json::<DirectoryEntry>(&self.url(&format!("directory/{user_id}")))
            .await?;
        Ok(entry.map(|e| e.bundle))
    }

    pub async fn publish_invitation(
        &self,
        recipient_id: &str,
        from_user_id: &str,
        display_name: &str,
    ) -> Result<()> {
        self.client
            .put(self.url(&format!("invitations/{recipient_id}/{from_user_id}")))
            .json(&json!({
                "from": from_user_id,
                "display_name": display_name,
                "ts": chrono::Utc::now().timestamp()
            }))
            .send()
            .await
            .context("firebase publish invitation")?
            .error_for_status()
            .context("firebase publish invitation status")?;
        Ok(())
    }

    pub async fn list_invitations(&self, user_id: &str) -> Result<Vec<(String, InvitationEntry)>> {
        let url = self.url(&format!("invitations/{user_id}"));
        let resp = self.client.get(&url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        let Some(obj) = value.as_object() else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for (id, entry) in obj {
            if let Ok(parsed) = serde_json::from_value::<InvitationEntry>(entry.clone()) {
                out.push((id.clone(), parsed));
            }
        }
        out.sort_by_key(|(_, e)| e.ts);
        Ok(out)
    }

    pub async fn delete_invitation(&self, recipient_id: &str, from_user_id: &str) -> Result<()> {
        let _ = self
            .client
            .delete(self.url(&format!("invitations/{recipient_id}/{from_user_id}")))
            .send()
            .await;
        Ok(())
    }

    /// E2E ciphertext: only `viewer_id` can decrypt with their identity + owner's bundle.
    pub async fn publish_avatar(
        &self,
        owner_id: &str,
        viewer_id: &str,
        ciphertext: &[u8],
        mime: &str,
    ) -> Result<()> {
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ciphertext);
        self.client
            .put(self.url(&format!("avatars/{owner_id}/{viewer_id}")))
            .json(&json!({
                "ciphertext": encoded,
                "mime": mime,
                "ts": chrono::Utc::now().timestamp()
            }))
            .send()
            .await
            .context("firebase publish avatar")?
            .error_for_status()
            .context("firebase publish avatar status")?;
        Ok(())
    }

    pub async fn fetch_avatar(&self, owner_id: &str, viewer_id: &str) -> Result<Option<AvatarEntry>> {
        self.fetch_json(&self.url(&format!("avatars/{owner_id}/{viewer_id}")))
            .await
    }

    pub async fn delete_avatar(&self, owner_id: &str, viewer_id: &str) -> Result<()> {
        let _ = self
            .client
            .delete(self.url(&format!("avatars/{owner_id}/{viewer_id}")))
            .send()
            .await;
        Ok(())
    }

    /// Viewer asks owner to publish an encrypted avatar copy on next sync.
    pub async fn request_avatar(&self, owner_id: &str, viewer_id: &str) -> Result<()> {
        self.client
            .put(self.url(&format!("avatar_wants/{owner_id}/{viewer_id}")))
            .json(&json!({ "ts": chrono::Utc::now().timestamp() }))
            .send()
            .await
            .context("firebase request avatar")?
            .error_for_status()
            .context("firebase request avatar status")?;
        Ok(())
    }

    pub async fn list_avatar_wants(&self, owner_id: &str) -> Result<Vec<String>> {
        let url = self.url(&format!("avatar_wants/{owner_id}"));
        let resp = self.client.get(&url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        let Some(obj) = value.as_object() else {
            return Ok(vec![]);
        };
        Ok(obj.keys().cloned().collect())
    }

    async fn fetch_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<Option<T>> {
        let resp = self.client.get(url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let value: serde_json::Value = resp.error_for_status()?.json().await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(value)?))
    }
}
