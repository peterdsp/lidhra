//! Issue Lidhra license keys. Keep the private key SECRET (never commit it).
//!
//!   lidhra-keygen genkey                        # make an issuer keypair (once)
//!   lidhra-keygen sign <private_hex> <owner>    # mint a license for a buyer
//!
//! After `genkey`, paste the public key into lidhra-license `ISSUER_PUBKEY_HEX`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("genkey") => {
            let sk = SigningKey::generate(&mut OsRng);
            println!("private (KEEP SECRET): {}", hex(&sk.to_bytes()));
            println!("public  (embed):       {}", hex(&sk.verifying_key().to_bytes()));
        }
        Some("sign") if args.len() >= 4 => {
            let priv_hex = &args[2];
            let owner = &args[3];
            let bytes = lidhra_license::hex_to_32(priv_hex).expect("private key must be 64 hex chars");
            let sk = SigningKey::from_bytes(&bytes);
            let sig = sk.sign(owner.as_bytes());
            println!("LIDHRA-{}.{}", B64.encode(owner.as_bytes()), B64.encode(sig.to_bytes()));
        }
        _ => {
            eprintln!("usage:\n  lidhra-keygen genkey\n  lidhra-keygen sign <private_hex> <owner>");
            std::process::exit(2);
        }
    }
}
