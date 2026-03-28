use anyhow::{Context, Result};
use std::path::Path;
use web_push::{
    ContentEncoding, SubscriptionInfo, Urgency, VapidSignatureBuilder, WebPushClient, WebPushError,
    WebPushMessage, WebPushMessageBuilder,
};

#[derive(Clone)]
pub struct VapidKeys {
    pub public_key_pem: String,
    pub private_key_pem: String,
    /// VAPID `sub` claim — required by Apple APNs for iOS Web Push delivery.
    pub subject: String,
}

pub fn load_or_create_vapid_keys(path: impl AsRef<Path>, subject: &str) -> Result<VapidKeys> {
    let path = path.as_ref();
    if path.exists() {
        let content = std::fs::read_to_string(path).context("read vapid_keys.json")?;
        let parsed: VapidKeysFile =
            serde_json::from_str(&content).context("parse vapid_keys.json")?;
        return Ok(VapidKeys {
            public_key_pem: parsed.public_key_pem,
            private_key_pem: parsed.private_key_pem,
            subject: subject.to_string(),
        });
    }
    let keys_pem = generate_vapid_keys()?;
    let file = VapidKeysFile {
        public_key_pem: keys_pem.0.clone(),
        private_key_pem: keys_pem.1.clone(),
    };
    std::fs::write(path, serde_json::to_string_pretty(&file)?).context("write vapid_keys.json")?;
    tracing::info!(path = %path.display(), "generated new VAPID keys");
    Ok(VapidKeys {
        public_key_pem: keys_pem.0,
        private_key_pem: keys_pem.1,
        subject: subject.to_string(),
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VapidKeysFile {
    public_key_pem: String,
    private_key_pem: String,
}

fn generate_vapid_keys() -> Result<(String, String)> {
    use openssl::ec::{EcGroup, EcKey};
    use openssl::nid::Nid;
    use openssl::pkey::PKey;

    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).context("create EC group")?;
    let ec_key = EcKey::generate(&group).context("generate EC key")?;
    let pkey = PKey::from_ec_key(ec_key).context("create PKey")?;
    let private_key_pem =
        String::from_utf8(pkey.private_key_to_pem_pkcs8()?).context("encode private key PEM")?;
    let public_key_pem =
        String::from_utf8(pkey.public_key_to_pem()?).context("encode public key PEM")?;
    Ok((public_key_pem, private_key_pem))
}

impl VapidKeys {
    pub fn public_key_base64url(&self) -> Result<String> {
        let builder = web_push::VapidSignatureBuilder::from_pem_no_sub(std::io::Cursor::new(
            self.private_key_pem.as_bytes(),
        ))
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let bytes = builder.get_public_key();
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &bytes,
        ))
    }
}

/// Builds an encrypted Web Push message (VAPID + payload). Used by [`send_push`] and tests.
pub fn build_push_message(
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    payload: &str,
    keys: &VapidKeys,
) -> Result<WebPushMessage, WebPushError> {
    let subscription_info = SubscriptionInfo::new(endpoint, p256dh, auth);
    let mut sig_builder = VapidSignatureBuilder::from_pem(
        std::io::Cursor::new(keys.private_key_pem.as_bytes()),
        &subscription_info,
    )?;
    // `sub` is required by Apple APNs (iOS Web Push); without it the push is silently dropped.
    sig_builder.add_claim("sub", keys.subject.as_str());
    let sig = sig_builder.build()?;
    let mut builder = WebPushMessageBuilder::new(&subscription_info)?;
    builder.set_payload(ContentEncoding::Aes128Gcm, payload.as_bytes());
    builder.set_vapid_signature(sig);
    // Apple: "To attempt to deliver the notification immediately, specify `high`" for Urgency.
    // Without this, APNs may defer delivery for power saving — often seen as missing iPhone alerts.
    builder.set_urgency(Urgency::High);
    builder.build()
}

pub async fn send_push(
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    payload: &str,
    keys: &VapidKeys,
    client: &WebPushClient,
) -> Result<(), WebPushError> {
    let message = build_push_message(endpoint, p256dh, auth, payload, keys)?;
    client.send(message).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_keys() -> VapidKeys {
        VapidKeys {
            public_key_pem: String::new(),
            private_key_pem: include_str!("../test_keys/private.pem").to_string(),
            subject: "mailto:test@example.com".to_string(),
        }
    }

    /// Minimal valid-looking subscription (from web-push crate tests) so encryption succeeds.
    fn fixture_subscription() -> (String, String, String) {
        (
            "https://fcm.googleapis.com/fcm/send/eKClHsXFm9E:APA91bH2x3gNOMv4dF1lQfCgIfOet8EngqKCAUS5DncLOd5hzfSUxcjigIjw9ws-bqa-KmohqiTOcgepAIVO03N39dQfkEkopubML_m3fyvF03pV9_JCB7SxpUjcFmBSVhCaWS6m8l7x".to_string(),
            "BGa4N1PI79lboMR_YrwCiCsgp35DRvedt7opHcf0yM3iOBTSoQYqQLwWxAfRKE6tsDnReWmhsImkhDF_DBdkNSU".to_string(),
            "EvcWjEgzr4rbvhfi3yds0A".to_string(),
        )
    }

    #[test]
    fn push_message_sets_urgency_high_for_apple_immediate_delivery() {
        let keys = fixture_keys();
        let (endpoint, p256dh, auth) = fixture_subscription();
        let msg = build_push_message(
            &endpoint,
            &p256dh,
            &auth,
            r#"{"title":"t","body":"b"}"#,
            &keys,
        )
        .expect("build_push_message");
        assert_eq!(msg.urgency, Some(Urgency::High));
    }
}
