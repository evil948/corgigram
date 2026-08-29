use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice::mdns::MulticastDnsMode;
use webrtc::ice::network_type::NetworkType;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy;

pub struct IceConfig {
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    pub turn_username: Option<String>,
    pub turn_credential: Option<String>,
    pub ipv4_only: bool,
    /// Force traffic through TURN relay (needed for most cross-NAT Internet connects).
    pub relay_only: bool,
}

impl Default for IceConfig {
    fn default() -> Self {
        Self {
            stun_urls: vec![
                "stun:stun.l.google.com:19302".into(),
                "stun:stun1.l.google.com:19302".into(),
            ],
            turn_urls: vec![
                "turn:openrelay.metered.ca:80".into(),
                "turn:openrelay.metered.ca:443".into(),
                "turn:openrelay.metered.ca:443?transport=tcp".into(),
                "turns:openrelay.metered.ca:443".into(),
            ],
            turn_username: Some("openrelayproject".into()),
            turn_credential: Some("openrelayproject".into()),
            ipv4_only: true,
            relay_only: false,
        }
    }
}

impl IceConfig {
    /// Host candidates only — for same-machine or LAN testing.
    pub fn localhost() -> Self {
        Self {
            stun_urls: vec![],
            turn_urls: vec![],
            turn_username: None,
            turn_credential: None,
            ipv4_only: true,
            relay_only: false,
        }
    }

    pub fn apply_setting_engine(&self, settings: &mut SettingEngine) {
        settings.set_ice_multicast_dns_mode(MulticastDnsMode::Disabled);
        // Longer ICE checks for slow TURN / mobile networks.
        settings.set_ice_timeouts(
            Some(std::time::Duration::from_secs(30)),
            Some(std::time::Duration::from_secs(90)),
            Some(std::time::Duration::from_secs(2)),
        );
        if self.ipv4_only {
            settings.set_network_types(vec![NetworkType::Udp4, NetworkType::Tcp4]);
        }
    }

    pub fn ice_transport_policy(&self) -> RTCIceTransportPolicy {
        if self.relay_only && !self.turn_urls.is_empty() {
            RTCIceTransportPolicy::Relay
        } else {
            RTCIceTransportPolicy::All
        }
    }

    pub fn rtc_ice_servers(&self) -> Vec<RTCIceServer> {
        let mut servers = Vec::new();

        for url in &self.stun_urls {
            servers.push(RTCIceServer {
                urls: vec![url.clone()],
                username: String::new(),
                credential: String::new(),
            });
        }

        for url in &self.turn_urls {
            servers.push(RTCIceServer {
                urls: vec![url.clone()],
                username: self.turn_username.clone().unwrap_or_default(),
                credential: self.turn_credential.clone().unwrap_or_default(),
            });
        }

        servers
    }
}
