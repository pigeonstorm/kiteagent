use anyhow::{Context, Result};
use std::path::Path;
use web_push::*;

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

pub async fn send_push(
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    payload: &str,
    keys: &VapidKeys,
    client: &WebPushClient,
) -> Result<(), web_push::WebPushError> {
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
    client.send(builder.build()?).await
}
