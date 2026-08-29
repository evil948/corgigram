use serde::{Deserialize, Serialize};

/// Shared Realtime Database for signaling and offline mailbox (ciphertext only).
pub const DEFAULT_FIREBASE_DATABASE_URL: &str =
    "https://corgigram-shared-default-rtdb.europe-west1.firebasedatabase.app";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// User override. `None` or empty → built-in default URL.
    pub firebase_database_url: Option<String>,
    pub firebase_auth_token: Option<String>,
}

impl AppConfig {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn effective_firebase_database_url(&self) -> String {
        self.firebase_database_url
            .as_ref()
            .map(|u| u.trim())
            .filter(|u| !u.is_empty())
            .map(|u| u.trim_end_matches('/').to_string())
            .unwrap_or_else(|| DEFAULT_FIREBASE_DATABASE_URL.to_string())
    }

    pub fn firebase_database_url_override(&self) -> Option<String> {
        Self::normalize_override(self.firebase_database_url.clone())
    }

    pub fn uses_default_firebase_url(&self) -> bool {
        self.firebase_database_url_override().is_none()
    }

    pub fn firebase_configured(&self) -> bool {
        true
    }

    pub fn with_normalized_firebase_url(mut self) -> Self {
        self.firebase_database_url = Self::normalize_override(self.firebase_database_url);
        self
    }

    fn normalize_override(url: Option<String>) -> Option<String> {
        let Some(url) = url else {
            return None;
        };
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return None;
        }
        let normalized = trimmed.trim_end_matches('/');
        if normalized == DEFAULT_FIREBASE_DATABASE_URL.trim_end_matches('/') {
            return None;
        }
        Some(normalized.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_default_when_override_missing() {
        let config = AppConfig::default();
        assert_eq!(
            config.effective_firebase_database_url(),
            DEFAULT_FIREBASE_DATABASE_URL
        );
        assert!(config.uses_default_firebase_url());
    }

    #[test]
    fn respects_custom_override() {
        let config = AppConfig {
            firebase_database_url: Some("https://custom.example.com".into()),
            ..Default::default()
        };
        assert_eq!(
            config.effective_firebase_database_url(),
            "https://custom.example.com"
        );
        assert!(!config.uses_default_firebase_url());
    }

    #[test]
    fn default_override_is_normalized_away() {
        let config = AppConfig {
            firebase_database_url: Some(DEFAULT_FIREBASE_DATABASE_URL.into()),
            ..Default::default()
        }
        .with_normalized_firebase_url();
        assert!(config.firebase_database_url.is_none());
    }
}
