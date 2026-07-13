//! Trial + offline license for Lidhra's Ko-fi / direct builds.
//!
//! Model:
//! - Ko-fi / direct download: free for [`TRIAL_DAYS`], then a license key is required
//!   (sold on Ko-fi). Keys are Ed25519-signed by the issuer's private key and verified
//!   offline against the embedded public key. No server needed.
//! - App Store build: paid upfront, no trial and no license logic (the store receipt is
//!   the license) - gate this crate out with a build flavor there.
//!
//! Honest limits: a key is not machine-bound, so it can be shared; the trial is stored
//! on disk, so a determined user can reset it. This is a "keep honest people honest"
//! system, which is the norm for indie apps. Add a server later if you need more.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::path::Path;

pub const TRIAL_DAYS: u64 = 7;

/// Issuer public key (hex, 32 bytes). Replace after `lidhra-keygen genkey`,
/// then keep the matching private key secret.
pub const ISSUER_PUBKEY_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Licensed { owner: String },
    Trial { days_left: u32 },
    Expired,
}

/// Decode 64 hex chars into 32 bytes.
pub fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Verify a license key against `pubkey_hex`; returns the owner string if valid.
/// Key format: `LIDHRA-<base64url(owner)>.<base64url(signature)>`.
pub fn verify_license(key: &str, pubkey_hex: &str) -> Option<String> {
    let body = key.trim().strip_prefix("LIDHRA-").unwrap_or(key.trim());
    let (owner_b64, sig_b64) = body.split_once('.')?;
    let owner = B64.decode(owner_b64).ok()?;
    let sig_bytes = B64.decode(sig_b64).ok()?;
    let vk = VerifyingKey::from_bytes(&hex_to_32(pubkey_hex)?).ok()?;
    let sig = Signature::from_slice(&sig_bytes).ok()?;
    vk.verify(&owner, &sig).ok()?;
    String::from_utf8(owner).ok()
}

/// Current entitlement: a valid license wins, otherwise the trial countdown.
pub fn status(now_unix: u64, install_unix: u64, license: Option<&str>, pubkey_hex: &str) -> Status {
    if let Some(k) = license {
        if let Some(owner) = verify_license(k, pubkey_hex) {
            return Status::Licensed { owner };
        }
    }
    let days = now_unix.saturating_sub(install_unix) / 86_400;
    if days < TRIAL_DAYS {
        Status::Trial { days_left: (TRIAL_DAYS - days) as u32 }
    } else {
        Status::Expired
    }
}

// ---------- on-disk state (app-facing) ----------

/// Read the install timestamp from `<dir>/install`, creating it (with `now_unix`)
/// on first run. This is what starts the trial clock.
pub fn load_or_init_install(dir: &Path, now_unix: u64) -> u64 {
    let path = dir.join("install");
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(v) = s.trim().parse::<u64>() {
            return v;
        }
    }
    std::fs::create_dir_all(dir).ok();
    std::fs::write(&path, now_unix.to_string()).ok();
    now_unix
}

/// Read a saved license key from `<dir>/license.key`, if present.
pub fn load_license(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("license.key")).ok().map(|s| s.trim().to_string())
}

/// Validate and persist a license key to `<dir>/license.key`. Returns the owner.
pub fn activate(dir: &Path, key: &str, pubkey_hex: &str) -> Result<String, String> {
    let owner = verify_license(key, pubkey_hex).ok_or("invalid license key")?;
    if !is_valid_for(&owner, &machine_id(dir)) {
        return Err("this license is locked to a different computer".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("license.key"), key.trim()).map_err(|e| e.to_string())?;
    Ok(owner)
}

/// A stable per-install id, used to node-lock a license to one machine.
/// Persisted in `<dir>/machine`. (For stronger binding, replace with a hardware
/// id; this is copyable if a user clones the whole config dir.)
pub fn machine_id(dir: &Path) -> String {
    use rand::RngCore;
    let path = dir.join("machine");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let t = s.trim();
        if t.len() == 32 {
            return t.to_string();
        }
    }
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    let id: String = b.iter().map(|x| format!("{x:02x}")).collect();
    std::fs::create_dir_all(dir).ok();
    std::fs::write(&path, &id).ok();
    id
}

/// A key whose signed subject is `MACHINE:<id>` only works on that machine.
/// Any other subject (e.g. an email) is unbound and works anywhere.
pub fn is_valid_for(subject: &str, machine_id: &str) -> bool {
    match subject.strip_prefix("MACHINE:") {
        Some(bound) => bound == machine_id,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn hex(b: [u8; 32]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn make_key(sk: &SigningKey, owner: &str) -> String {
        let sig = sk.sign(owner.as_bytes());
        format!("LIDHRA-{}.{}", B64.encode(owner.as_bytes()), B64.encode(sig.to_bytes()))
    }

    #[test]
    fn valid_license_verifies_and_forgery_fails() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk = hex(sk.verifying_key().to_bytes());
        let key = make_key(&sk, "alice@example.com");
        assert_eq!(verify_license(&key, &pk), Some("alice@example.com".into()));

        // wrong issuer key rejects
        let other = hex(SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes());
        assert_eq!(verify_license(&key, &other), None);
        // garbage rejects
        assert_eq!(verify_license("LIDHRA-nope.nope", &pk), None);
    }

    #[test]
    fn machine_id_is_stable_and_node_lock_works() {
        let dir = std::env::temp_dir().join(format!("lidhra-mtest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = machine_id(&dir);
        let b = machine_id(&dir);
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        // unbound subject works anywhere; a MACHINE: subject only on its machine
        assert!(is_valid_for("alice@example.com", &a));
        assert!(is_valid_for(&format!("MACHINE:{a}"), &a));
        assert!(!is_valid_for("MACHINE:someoneelse", &a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trial_counts_down_then_expires_unless_licensed() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk = hex(sk.verifying_key().to_bytes());
        let now = 1_700_000_000u64;
        assert_eq!(status(now, now, None, &pk), Status::Trial { days_left: 7 });
        assert_eq!(status(now, now - 3 * 86_400, None, &pk), Status::Trial { days_left: 4 });
        assert_eq!(status(now, now - 7 * 86_400, None, &pk), Status::Expired);
        // a valid license overrides an expired trial
        let key = make_key(&sk, "bob");
        assert_eq!(
            status(now, now - 30 * 86_400, Some(&key), &pk),
            Status::Licensed { owner: "bob".into() }
        );
    }
}
