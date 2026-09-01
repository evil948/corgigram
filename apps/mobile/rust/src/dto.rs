use corgigram_core::AppConfig;
use corgigram_storage::{ContactRecord, MessageRecord};
use flutter_rust_bridge::frb;

#[frb(non_final)]
pub struct ProfileDto {
    pub user_id: String,
    pub display_name: String,
    pub bundle_json: String,
    pub safety_hint: String,
    pub avatar_data_url: Option<String>,
}

#[frb(non_final)]
pub struct ContactDto {
    pub user_id: String,
    pub display_name: String,
    pub bundle_json: String,
    pub created_at: String,
    pub avatar_data_url: Option<String>,
}

#[frb(non_final)]
pub struct MessageDto {
    pub id: String,
    pub contact_id: String,
    pub direction: String,
    pub body: String,
    pub status: String,
    pub created_at: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_mime: Option<String>,
}

fn default_kind() -> String {
    "text".into()
}

#[frb(non_final)]
pub struct SnapshotDto {
    pub has_identity: bool,
    pub profile: Option<ProfileDto>,
    pub contacts: Vec<ContactDto>,
    pub connected_contact_id: Option<String>,
    pub connecting_contact_id: Option<String>,
    pub firebase_configured: bool,
    pub firebase_database_url: String,
    pub firebase_database_url_override: Option<String>,
    pub firebase_uses_default_url: bool,
    pub outbox_count: i32,
    #[serde(default)]
    pub pending_invitations: Vec<InvitationDto>,
    #[serde(default)]
    pub contact_presence: std::collections::HashMap<String, bool>,
}

#[frb(non_final)]
pub struct InvitationDto {
    pub from_user_id: String,
    pub display_name: String,
}

#[frb(non_final)]
pub struct ConnectAutoDto {
    pub contact_id: String,
    pub connected: bool,
}

#[frb(non_final)]
pub struct PushPayloadDto {
    pub kind: String,
    pub sender_id: String,
}

impl From<corgigram_core::ProfileInfo> for ProfileDto {
    fn from(p: corgigram_core::ProfileInfo) -> Self {
        Self {
            user_id: p.user_id,
            display_name: p.display_name,
            bundle_json: p.bundle_json,
            safety_hint: p.safety_hint,
            avatar_data_url: p.avatar_data_url,
        }
    }
}

impl From<ContactRecord> for ContactDto {
    fn from(c: ContactRecord) -> Self {
        Self {
            user_id: c.user_id,
            display_name: c.display_name,
            bundle_json: serde_json::to_string(&c.bundle).unwrap_or_default(),
            created_at: c.created_at.to_rfc3339(),
            avatar_data_url: c.avatar_data_url,
        }
    }
}

impl From<MessageRecord> for MessageDto {
    fn from(m: MessageRecord) -> Self {
        Self {
            id: m.id,
            contact_id: m.contact_id,
            direction: m.direction,
            body: m.body,
            status: m.status,
            created_at: m.created_at.to_rfc3339(),
            kind: m.kind,
            attachment_name: m.attachment_name,
            attachment_mime: m.attachment_mime,
        }
    }
}

impl From<corgigram_core::AppSnapshot> for SnapshotDto {
    fn from(s: corgigram_core::AppSnapshot) -> Self {
        Self {
            has_identity: s.has_identity,
            profile: s.profile.map(ProfileDto::from),
            contacts: s.contacts.into_iter().map(ContactDto::from).collect(),
            connected_contact_id: s.connected_contact_id,
            connecting_contact_id: s.connecting_contact_id,
            firebase_configured: s.firebase_configured,
            firebase_database_url: s.firebase_database_url,
            firebase_database_url_override: s.firebase_database_url_override,
            firebase_uses_default_url: s.firebase_uses_default_url,
            outbox_count: s.outbox_count as i32,
            pending_invitations: s
                .pending_invitations
                .into_iter()
                .map(|i| InvitationDto {
                    from_user_id: i.from_user_id,
                    display_name: i.display_name,
                })
                .collect(),
            contact_presence: s.contact_presence,
        }
    }
}

impl From<corgigram_core::ConnectAutoResult> for ConnectAutoDto {
    fn from(v: corgigram_core::ConnectAutoResult) -> Self {
        Self {
            contact_id: v.contact_id,
            connected: v.connected,
        }
    }
}

impl From<corgigram_core::PushNotification> for PushPayloadDto {
    fn from(p: corgigram_core::PushNotification) -> Self {
        Self {
            kind: p.kind,
            sender_id: p.sender_id,
        }
    }
}

pub fn config_from(
    firebase_database_url: Option<String>,
    firebase_auth_token: Option<String>,
) -> AppConfig {
    AppConfig {
        firebase_database_url,
        firebase_auth_token,
    }
}
