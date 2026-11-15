//! Who pays for transcribing: an API key or OAuth token from the environment, else an API key the
//! user saved in the OS keychain. The environment wins, and the keychain isn't read while it's
//! set, so development builds (which change with every build, and so ask again for the keychain)
//! don't ask. Signing in with a Claude.ai subscription isn't allowed for apps like this one.
//!
//! See `decisions/2026-11-15-api-key-in-keychain.md`.

use std::fmt;
use std::sync::Mutex;

/// The keychain entry's service: the app's bundle identifier.
pub const SERVICE: &str = "net.ftamp.notebook-tools";
/// The keychain entry's account.
const ACCOUNT: &str = "anthropic-api-key";

/// How to authenticate with the API.
#[derive(Clone, PartialEq)]
pub enum Credentials {
    ApiKey(String),
    /// An OAuth access token, e.g. from `ant auth print-credentials --access-token`.
    OAuth(String),
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(_) => f.write_str("ApiKey(..)"),
            Self::OAuth(_) => f.write_str("OAuth(..)"),
        }
    }
}

impl Credentials {
    /// The key or token, for showing [`mask`]ed.
    pub fn secret(&self) -> &str {
        match self {
            Self::ApiKey(secret) | Self::OAuth(secret) => secret,
        }
    }
}

/// Where credentials came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    EnvApiKey,
    EnvAuthToken,
    Keychain,
}

/// `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN`.
pub fn from_env() -> Option<(Credentials, Source)> {
    from_vars(|name| std::env::var(name).ok())
}

/// [`from_env`] with the variables looked up by `get`. Blank values count as unset.
fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<(Credentials, Source)> {
    let get = |name| {
        get(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    get("ANTHROPIC_API_KEY")
        .map(|key| (Credentials::ApiKey(key), Source::EnvApiKey))
        .or_else(|| {
            get("ANTHROPIC_AUTH_TOKEN")
                .map(|token| (Credentials::OAuth(token), Source::EnvAuthToken))
        })
}

/// What reading the keychain found, which also says whether there is one.
#[derive(Clone, PartialEq)]
pub enum Stored {
    Has(String),
    Empty,
    /// No keychain to use, as on Linux without a Secret Service, or it refused: why.
    Unavailable(String),
}

impl fmt::Debug for Stored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Has(_) => f.write_str("Has(..)"),
            Self::Empty => f.write_str("Empty"),
            Self::Unavailable(why) => f.debug_tuple("Unavailable").field(why).finish(),
        }
    }
}

/// Where a saved API key is kept.
pub trait KeyStore: Send + Sync {
    fn load(&self) -> Stored;
    /// Saves `key`, replacing any saved before; or why it couldn't.
    fn save(&self, key: &str) -> Result<(), String>;
    /// Forgets the saved key, if there is one; or why it couldn't.
    fn remove(&self) -> Result<(), String>;
}

/// The OS keychain: Keychain Services on macOS, Credential Manager on Windows, the Secret Service
/// elsewhere.
pub struct Keychain;

impl Keychain {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
    }
}

impl KeyStore for Keychain {
    fn load(&self) -> Stored {
        let entry = match self.entry() {
            Ok(entry) => entry,
            Err(err) => return Stored::Unavailable(err),
        };
        match entry.get_password() {
            Ok(key) => Stored::Has(key),
            Err(keyring::Error::NoEntry) => Stored::Empty,
            // Something's there, but not a key: saving one replaces it.
            Err(err @ (keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..))) => {
                log::warn!("ignoring the unreadable API key in the keychain: {err}");
                Stored::Empty
            }
            Err(err) => Stored::Unavailable(err.to_string()),
        }
    }

    fn save(&self, key: &str) -> Result<(), String> {
        self.entry()?.set_password(key).map_err(|e| e.to_string())
    }

    fn remove(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// A key store in memory, for tests: it starts holding what it's given, and can be made to fail.
pub struct MemoryStore {
    stored: Mutex<Stored>,
    /// Why saving or removing fails, if it does.
    fail: Mutex<Option<String>>,
}

impl MemoryStore {
    pub fn new(stored: Stored) -> Self {
        Self {
            stored: Mutex::new(stored),
            fail: Mutex::new(None),
        }
    }

    /// Makes saving and removing fail with `why` from now on.
    pub fn fail_with(&self, why: &str) {
        *self.fail.lock().unwrap() = Some(why.to_owned());
    }

    fn change(&self, to: Stored) -> Result<(), String> {
        if let Some(why) = self.fail.lock().unwrap().clone() {
            return Err(why);
        }
        let mut stored = self.stored.lock().unwrap();
        if let Stored::Unavailable(why) = &*stored {
            return Err(why.clone());
        }
        *stored = to;
        Ok(())
    }
}

impl KeyStore for MemoryStore {
    fn load(&self) -> Stored {
        self.stored.lock().unwrap().clone()
    }

    fn save(&self, key: &str) -> Result<(), String> {
        self.change(Stored::Has(key.to_owned()))
    }

    fn remove(&self) -> Result<(), String> {
        self.change(Stored::Empty)
    }
}

/// `secret` with all but its start and last four characters hidden, as "sk-ant-…a1B2".
pub fn mask(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    let prefix = if secret.starts_with("sk-ant-") { 7 } else { 0 };
    if chars.len() < prefix + 8 {
        return "…".to_owned();
    }
    let end: String = chars[chars.len() - 4..].iter().collect();
    format!("{}…{end}", &secret[..prefix])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(set: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            set.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn api_key_wins_over_auth_token() {
        let both = [
            ("ANTHROPIC_API_KEY", "key"),
            ("ANTHROPIC_AUTH_TOKEN", "tok"),
        ];
        assert_eq!(
            from_vars(vars(&both)),
            Some((Credentials::ApiKey("key".into()), Source::EnvApiKey))
        );
        let token = [
            ("ANTHROPIC_API_KEY", "  "),
            ("ANTHROPIC_AUTH_TOKEN", "tok\n"),
        ];
        assert_eq!(
            from_vars(vars(&token)),
            Some((Credentials::OAuth("tok".into()), Source::EnvAuthToken))
        );
        assert_eq!(from_vars(vars(&[])), None);
    }

    #[test]
    fn debug_hides_secrets() {
        assert_eq!(
            format!("{:?}", Credentials::ApiKey("sk-secret".into())),
            "ApiKey(..)"
        );
        assert_eq!(format!("{:?}", Stored::Has("sk-secret".into())), "Has(..)");
    }

    #[test]
    fn mask_keeps_prefix_and_last_four() {
        assert_eq!(mask("sk-ant-api03-abcdefghijklmnopa1B2"), "sk-ant-…a1B2");
        assert_eq!(mask("some-oauth-token-wxyz"), "…wxyz");
        // Too short to show any of without giving most of it away.
        assert_eq!(mask("sk-ant-abc"), "…");
        assert_eq!(mask("short"), "…");
    }

    #[test]
    fn memory_store_saves_removes_and_fails() {
        let store = MemoryStore::new(Stored::Empty);
        store.save("key").unwrap();
        assert_eq!(store.load(), Stored::Has("key".into()));
        store.remove().unwrap();
        assert_eq!(store.load(), Stored::Empty);
        store.fail_with("locked");
        assert_eq!(store.save("key"), Err("locked".into()));
        assert_eq!(store.load(), Stored::Empty);

        let none = MemoryStore::new(Stored::Unavailable("no keychain".into()));
        assert_eq!(none.save("key"), Err("no keychain".into()));
    }
}
