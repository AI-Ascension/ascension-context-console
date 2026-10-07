use hmac::{Hmac, KeyInit, Mac};
use owner_seed_sha2::Sha256;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::authenticated_ingress::{
    PrincipalVerificationError, PrincipalVerifier, VerifiedPrincipalClaims,
};

use super::config::OperatorConfig;
use super::protected_files::{AdapterError, PrivateRoot, validate_name};

type HmacSha256 = Hmac<Sha256>;
const DOMAIN: &[u8] = b"ascension.console18.operator-principal.v1\0";
const SCHEMA: &str = "ascension.context-console.principals.v1";
const MAX_REGISTRY_BYTES: usize = 64 * 1024;
const MAX_RECORDS: usize = 512;
const MAX_BEARER_BYTES: usize = 166;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    records: Vec<Record>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    reference: String,
    issuer: String,
    subject: String,
    audience: String,
    credential_id: String,
    expires_at: u64,
    revoked: bool,
    tag: String,
}

pub(super) struct FilePrincipalVerifier {
    root: PrivateRoot,
    registry_ref: String,
    mac_key_ref: String,
}

impl FilePrincipalVerifier {
    pub(super) fn new(config: &OperatorConfig) -> Self {
        Self {
            root: config.root.clone(),
            registry_ref: config.principal_registry_ref.clone(),
            mac_key_ref: config.principal_mac_key_ref.clone(),
        }
    }

    pub(super) fn validate_startup(&self) -> Result<(), AdapterError> {
        let registry_bytes = self
            .root
            .read_file(&self.registry_ref, MAX_REGISTRY_BYTES)?;
        let registry: Registry =
            serde_json::from_slice(&registry_bytes).map_err(|_| AdapterError::Invalid)?;
        validate_registry(&registry)?;
        let key = self.root.read_file(&self.mac_key_ref, 32)?;
        if key.len() != 32 {
            return Err(AdapterError::Invalid);
        }
        Ok(())
    }
}

impl PrincipalVerifier for FilePrincipalVerifier {
    fn verify(
        &mut self,
        bearer: &[u8],
    ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError> {
        let (reference, token) =
            parse_token(bearer).map_err(|_| PrincipalVerificationError::Invalid)?;
        let bytes = self
            .root
            .read_file(&self.registry_ref, MAX_REGISTRY_BYTES)
            .map_err(map_read_error)?;
        let registry: Registry =
            serde_json::from_slice(&bytes).map_err(|_| PrincipalVerificationError::Invalid)?;
        validate_registry(&registry).map_err(|_| PrincipalVerificationError::Invalid)?;
        let record = registry
            .records
            .iter()
            .find(|record| record.reference == reference)
            .ok_or(PrincipalVerificationError::Invalid)?;
        let now = unix_now().ok_or(PrincipalVerificationError::Unavailable)?;
        if record.revoked || record.expires_at <= now {
            return Err(PrincipalVerificationError::Invalid);
        }
        let key_bytes = self
            .root
            .read_file(&self.mac_key_ref, 32)
            .map_err(map_read_error)?;
        let key: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| PrincipalVerificationError::Invalid)?;
        let key = Zeroizing::new(key);
        let tag = decode_hex::<32>(&record.tag).map_err(|_| PrincipalVerificationError::Invalid)?;
        let mac = record_mac(&key, record, &token)
            .map_err(|_| PrincipalVerificationError::Unavailable)?;
        mac.verify_slice(&tag)
            .map_err(|_| PrincipalVerificationError::Invalid)?;
        Ok(VerifiedPrincipalClaims {
            issuer: record.issuer.clone(),
            subject: record.subject.clone(),
            audience: record.audience.clone(),
            credential_id: record.credential_id.clone(),
            expires_at: record.expires_at,
        })
    }
}

fn validate_registry(registry: &Registry) -> Result<(), AdapterError> {
    if registry.schema_version != SCHEMA
        || registry.records.is_empty()
        || registry.records.len() > MAX_RECORDS
    {
        return Err(AdapterError::Invalid);
    }
    let mut refs = std::collections::BTreeSet::new();
    let mut credentials = std::collections::BTreeSet::new();
    for record in &registry.records {
        validate_name(&record.reference)?;
        if !valid_text(&record.issuer)
            || !crate::harness_facade::valid_id(&record.subject)
            || !valid_text(&record.audience)
            || !valid_text(&record.credential_id)
            || record.expires_at == 0
            || decode_hex::<32>(&record.tag).is_err()
            || !refs.insert(record.reference.as_str())
            || !credentials.insert(record.credential_id.as_str())
        {
            return Err(AdapterError::Invalid);
        }
    }
    Ok(())
}

fn parse_token(value: &[u8]) -> Result<(String, Zeroizing<[u8; 32]>), AdapterError> {
    if value.len() > MAX_BEARER_BYTES {
        return Err(AdapterError::Denied);
    }
    let text = std::str::from_utf8(value).map_err(|_| AdapterError::Denied)?;
    let remainder = text.strip_prefix("ccp1.").ok_or(AdapterError::Denied)?;
    let (reference, encoded_secret) = remainder.rsplit_once('.').ok_or(AdapterError::Denied)?;
    validate_name(reference)?;
    let secret = decode_hex::<32>(encoded_secret)?;
    Ok((reference.to_owned(), Zeroizing::new(secret)))
}

fn record_mac(
    key: &[u8; 32],
    record: &Record,
    token: &[u8; 32],
) -> Result<HmacSha256, AdapterError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| AdapterError::KeyUnavailable)?;
    mac.update(DOMAIN);
    let expires_at = record.expires_at.to_be_bytes();
    let revoked = [u8::from(record.revoked)];
    for field in [
        record.reference.as_bytes(),
        record.issuer.as_bytes(),
        record.subject.as_bytes(),
        record.audience.as_bytes(),
        record.credential_id.as_bytes(),
        expires_at.as_slice(),
        revoked.as_slice(),
        token,
    ] {
        mac.update(&(field.len() as u64).to_be_bytes());
        mac.update(field);
    }
    Ok(mac)
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], AdapterError> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AdapterError::Invalid);
    }
    let mut output = [0; N];
    for (index, slot) in output.iter_mut().enumerate() {
        let high = hex_nibble(value.as_bytes()[index * 2]).ok_or(AdapterError::Invalid)?;
        let low = hex_nibble(value.as_bytes()[index * 2 + 1]).ok_or(AdapterError::Invalid)?;
        *slot = (high << 4) | low;
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn unix_now() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}

fn map_read_error(error: AdapterError) -> PrincipalVerificationError {
    match error {
        AdapterError::Invalid | AdapterError::Denied => PrincipalVerificationError::Invalid,
        #[cfg(not(unix))]
        AdapterError::UnsupportedPlatform => PrincipalVerificationError::Unavailable,
        AdapterError::Unavailable | AdapterError::KeyUnavailable => {
            PrincipalVerificationError::Unavailable
        }
    }
}

#[cfg(all(test, unix))]
pub(super) struct TestTagInput<'a> {
    pub(super) reference: &'a str,
    pub(super) issuer: &'a str,
    pub(super) subject: &'a str,
    pub(super) audience: &'a str,
    pub(super) credential_id: &'a str,
    pub(super) expires_at: u64,
    pub(super) revoked: bool,
}

#[cfg(all(test, unix))]
pub(super) fn test_tag(key: &[u8; 32], input: TestTagInput<'_>, token: &[u8; 32]) -> String {
    let record = Record {
        reference: input.reference.to_owned(),
        issuer: input.issuer.to_owned(),
        subject: input.subject.to_owned(),
        audience: input.audience.to_owned(),
        credential_id: input.credential_id.to_owned(),
        expires_at: input.expires_at,
        revoked: input.revoked,
        tag: String::new(),
    };
    let bytes = record_mac(key, &record, token)
        .expect("fixed HMAC key")
        .finalize()
        .into_bytes();
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("writing to String");
    }
    encoded
}
