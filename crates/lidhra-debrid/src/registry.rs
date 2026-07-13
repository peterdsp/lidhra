//! Build a provider from an id + credential — what the settings UI / CLI use to
//! turn "the user picked Real-Debrid and pasted this key" into a live adapter.

use crate::error::{DebridError, Result};
use crate::model::{Credential, ProviderId};
use crate::provider::DebridProvider;
use crate::providers::{AllDebrid, Premiumize, RealDebrid, TorBox};

impl ProviderId {
    /// Providers with a working adapter today (drives the settings UI list).
    pub const IMPLEMENTED: &'static [ProviderId] =
        &[ProviderId::RealDebrid, ProviderId::AllDebrid, ProviderId::TorBox, ProviderId::Premiumize];

    /// Parse a user-typed provider name/alias, case- and separator-insensitive.
    pub fn from_key(s: &str) -> Option<ProviderId> {
        let k: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
        match k.as_str() {
            "realdebrid" | "rd" => Some(ProviderId::RealDebrid),
            "alldebrid" | "ad" => Some(ProviderId::AllDebrid),
            "torbox" | "tb" => Some(ProviderId::TorBox),
            "premiumize" | "pm" => Some(ProviderId::Premiumize),
            "debridlink" | "dl" => Some(ProviderId::DebridLink),
            "offcloud" | "oc" => Some(ProviderId::Offcloud),
            "megadebrid" => Some(ProviderId::MegaDebrid),
            "deepbrid" => Some(ProviderId::Deepbrid),
            "highway" => Some(ProviderId::HighWay),
            _ => None,
        }
    }
}

/// Instantiate a provider adapter (does not authenticate — call
/// [`DebridProvider::authenticate`] next). Errors for ids without an adapter yet.
pub fn build_provider(id: ProviderId, cred: Credential) -> Result<Box<dyn DebridProvider>> {
    let key = match cred {
        Credential::ApiKey(k) => k,
        Credential::OAuth { access, .. } => access,
    };
    Ok(match id {
        ProviderId::RealDebrid => Box::new(RealDebrid::new(key)),
        ProviderId::AllDebrid => Box::new(AllDebrid::new(key)),
        ProviderId::TorBox => Box::new(TorBox::new(key)),
        ProviderId::Premiumize => Box::new(Premiumize::new(key)),
        other => {
            return Err(DebridError::Provider(format!("{}: adapter not implemented yet", other.label())))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_names_and_aliases() {
        assert_eq!(ProviderId::from_key("Real-Debrid"), Some(ProviderId::RealDebrid));
        assert_eq!(ProviderId::from_key("rd"), Some(ProviderId::RealDebrid));
        assert_eq!(ProviderId::from_key("TORBOX"), Some(ProviderId::TorBox));
        assert_eq!(ProviderId::from_key("all debrid"), Some(ProviderId::AllDebrid));
        assert_eq!(ProviderId::from_key("nope"), None);
    }

    #[test]
    fn builds_implemented_providers_and_reports_ids() {
        for &id in ProviderId::IMPLEMENTED {
            let p = build_provider(id, Credential::ApiKey("x".into())).unwrap();
            assert_eq!(p.id(), id);
        }
    }

    #[test]
    fn unimplemented_provider_errors_cleanly() {
        assert!(build_provider(ProviderId::Offcloud, Credential::ApiKey("x".into())).is_err());
    }
}
