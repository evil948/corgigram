use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// FCM/APNs push payload — no message text, only metadata for wake-up sync.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushNotification {
    pub kind: String,
    pub sender_id: String,
}

impl PushNotification {
    pub fn new_message(sender_id: &str) -> Self {
        Self {
            kind: "new_message".into(),
            sender_id: sender_id.to_string(),
        }
    }

    pub fn to_fcm_data(&self) -> HashMap<String, String> {
        HashMap::from([
            ("type".into(), self.kind.clone()),
            ("sender_id".into(), self.sender_id.clone()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_has_no_body_field() {
        let p = PushNotification::new_message("alice");
        let data = p.to_fcm_data();
        assert_eq!(data.get("type").map(String::as_str), Some("new_message"));
        assert_eq!(data.get("sender_id").map(String::as_str), Some("alice"));
        assert!(!data.contains_key("body"));
        assert!(!data.contains_key("text"));
    }
}
