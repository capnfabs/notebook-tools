//! Which credentials pages are sent with: the environment's, else a key saved in the keychain,
//! else the `ant` command line's login, if it was logged in at launch. Only the first is used. The
//! keychain is read at launch only when the environment has none, else when the user first asks
//! to enter a key: macOS asks for access again with every development build, as it's signed anew.
//! See `decisions/2026-11-15-api-key-in-keychain.md` and
//! `decisions/2026-11-15-ant-login-fallback.md`.

use std::sync::Arc;

use notebook_core::claude_transcribe::OcrError;
use notebook_core::credentials::ant::{Ant, LoggedIn};
use notebook_core::credentials::{Credentials, KeyStore, Login, Source, Stored};

use crate::transcriptions::Transcriber;

/// Whether the API accepts some credentials. Runs off the UI thread.
pub type CheckKey = Arc<dyn Fn(&Credentials) -> Result<(), OcrError> + Send + Sync>;

/// A transcriber that sends pages with the login given.
pub type MakeTranscriber = Arc<dyn Fn(Login) -> Transcriber + Send + Sync>;

/// `ant`'s login, and whose account it is, for Settings.
struct AntAccount {
    ant: Arc<Ant>,
    email: String,
    org: String,
}

/// Where pages are sent from, for Settings to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Active {
    /// A key or token from `source`, [`notebook_core::credentials::mask`]ed.
    Key { source: Source, masked: String },
    /// `ant`'s login, as `email` in `org`.
    Ant { email: String, org: String },
}

pub struct Auth {
    /// From the environment, which wins over the keychain.
    env: Option<(Credentials, Source)>,
    store: Arc<dyn KeyStore>,
    /// What the store held when last read or changed, or `None` before it's read.
    stored: Option<Stored>,
    /// Used only without the environment's credentials or a saved key.
    ant: Option<AntAccount>,
    check: CheckKey,
    make_transcriber: MakeTranscriber,
}

impl Auth {
    /// Reads `store` straight away, unless `env` has credentials. `ant` is `ant`'s login, if it was
    /// logged in at launch, with what it said then.
    pub fn new(
        env: Option<(Credentials, Source)>,
        store: Arc<dyn KeyStore>,
        ant: Option<LoggedIn>,
        check: CheckKey,
        make_transcriber: MakeTranscriber,
    ) -> Self {
        let ant = ant.map(|(ant, login)| AntAccount {
            ant,
            email: login.account_email.unwrap_or_default(),
            org: login.organization_name.unwrap_or_default(),
        });
        let mut auth = Self {
            env,
            store,
            stored: None,
            ant,
            check,
            make_transcriber,
        };
        if auth.env.is_none() {
            auth.stored();
        }
        auth
    }

    /// The login pages are sent with, and where it came from.
    pub fn current(&self) -> Option<(Login, Source)> {
        if let Some((credentials, source)) = &self.env {
            return Some((Login::Fixed(credentials.clone()), *source));
        }
        if let Some(Stored::Has(key)) = &self.stored {
            let key = Credentials::ApiKey(key.clone());
            return Some((Login::Fixed(key), Source::Keychain));
        }
        self.ant
            .as_ref()
            .map(|account| (Login::Ant(account.ant.clone()), Source::Ant))
    }

    /// Where pages are sent from, for Settings.
    pub fn active(&self) -> Option<Active> {
        let (login, source) = self.current()?;
        Some(match login {
            Login::Fixed(credentials) => Active::Key {
                source,
                masked: notebook_core::credentials::mask(credentials.secret()),
            },
            Login::Ant(_) => {
                let account = self.ant.as_ref()?;
                Active::Ant {
                    email: account.email.clone(),
                    org: account.org.clone(),
                }
            }
        })
    }

    /// A transcriber with [`Self::current`]'s login, if there is one.
    pub fn transcriber(&self) -> Option<Transcriber> {
        let (login, _) = self.current()?;
        Some((self.make_transcriber)(login))
    }

    /// Where the environment's credentials came from, if it has any.
    pub fn env_source(&self) -> Option<Source> {
        self.env.as_ref().map(|(_, source)| *source)
    }

    /// What the keychain holds, reading it if it hasn't been.
    pub fn stored(&mut self) -> &Stored {
        self.stored.get_or_insert_with(|| {
            let stored = self.store.load();
            if let Stored::Unavailable(why) = &stored {
                log::warn!("no keychain for the API key: {why}");
            }
            stored
        })
    }

    /// Whether the keychain is known to hold a key, without reading it.
    pub fn has_saved_key(&self) -> bool {
        matches!(self.stored, Some(Stored::Has(_)))
    }

    pub fn check(&self) -> CheckKey {
        self.check.clone()
    }

    /// Saves `key` in the keychain, to send pages with unless the environment has credentials.
    pub fn save(&mut self, key: &str) -> Result<(), String> {
        self.store.save(key)?;
        self.stored = Some(Stored::Has(key.to_owned()));
        Ok(())
    }

    /// Forgets the key saved in the keychain.
    pub fn remove(&mut self) -> Result<(), String> {
        self.store.remove()?;
        self.stored = Some(Stored::Empty);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use notebook_core::credentials::MemoryStore;

    use super::*;

    /// Counts how often it's read.
    struct Counting(MemoryStore, AtomicUsize);

    impl KeyStore for Counting {
        fn load(&self) -> Stored {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.load()
        }
        fn save(&self, key: &str) -> Result<(), String> {
            self.0.save(key)
        }
        fn remove(&self) -> Result<(), String> {
            self.0.remove()
        }
    }

    fn auth(
        env: Option<(Credentials, Source)>,
        store: Arc<dyn KeyStore>,
        ant: Option<LoggedIn>,
    ) -> Auth {
        let make: MakeTranscriber =
            Arc::new(|_| Transcriber::new(Arc::new(|_, _, _, _| unreachable!())));
        Auth::new(env, store, ant, Arc::new(|_| Ok(())), make)
    }

    fn store(stored: Stored) -> Arc<MemoryStore> {
        Arc::new(MemoryStore::new(stored))
    }

    /// A logged-in `ant` whose token is "ant-tok".
    fn ant() -> Option<LoggedIn> {
        let json = r#"{"access_token":"ant-tok","organization_name":"Org","account_email":"me@example.com"}"#;
        let ant = Ant::new(Box::new(move || Ok(json.as_bytes().to_vec())));
        let login = ant.login().unwrap();
        Some((Arc::new(ant), login))
    }

    /// What `auth` sends pages with, and where it came from.
    fn sends_with(auth: &Auth) -> Option<(Credentials, Source)> {
        let (login, source) = auth.current()?;
        Some((login.credentials().unwrap(), source))
    }

    #[test]
    fn env_then_keychain_then_ant() {
        let env = (Credentials::OAuth("tok".into()), Source::EnvAuthToken);
        let saved = || store(Stored::Has("saved".into()));
        let all = auth(Some(env.clone()), saved(), ant());
        assert_eq!(sends_with(&all), Some(env));

        let no_env = auth(None, saved(), ant());
        assert_eq!(
            sends_with(&no_env),
            Some((Credentials::ApiKey("saved".into()), Source::Keychain))
        );
        assert_eq!(
            no_env.active(),
            Some(Active::Key {
                source: Source::Keychain,
                masked: "…".into()
            })
        );

        let only_ant = auth(None, store(Stored::Empty), ant());
        assert_eq!(
            sends_with(&only_ant),
            Some((Credentials::OAuth("ant-tok".into()), Source::Ant))
        );
        assert_eq!(
            only_ant.active(),
            Some(Active::Ant {
                email: "me@example.com".into(),
                org: "Org".into()
            })
        );
        assert!(only_ant.transcriber().is_some());

        let none = auth(None, store(Stored::Empty), None);
        assert_eq!(sends_with(&none), None);
        assert_eq!(none.active(), None);
        assert!(none.transcriber().is_none());
    }

    #[test]
    fn env_wins_and_keychain_is_read_only_when_asked() {
        let store = Arc::new(Counting(
            MemoryStore::new(Stored::Has("saved".into())),
            AtomicUsize::new(0),
        ));
        let env = (Credentials::OAuth("tok".into()), Source::EnvAuthToken);
        let mut with_env = auth(Some(env.clone()), store.clone(), None);
        assert_eq!(store.1.load(Ordering::SeqCst), 0);
        assert_eq!(sends_with(&with_env), Some(env.clone()));
        assert!(!with_env.has_saved_key());

        assert_eq!(with_env.stored(), &Stored::Has("saved".into()));
        with_env.stored();
        assert_eq!(store.1.load(Ordering::SeqCst), 1);
        assert!(with_env.has_saved_key());
        assert_eq!(sends_with(&with_env), Some(env));

        let without_env = auth(None, store.clone(), None);
        assert_eq!(
            store.1.load(Ordering::SeqCst),
            2,
            "read at once without env"
        );
        assert!(without_env.has_saved_key());
    }

    #[test]
    fn saving_and_removing_a_key_switch_from_and_back_to_ant() {
        let store = store(Stored::Empty);
        let mut auth = auth(None, store.clone(), ant());
        auth.save("new").unwrap();
        assert_eq!(
            sends_with(&auth),
            Some((Credentials::ApiKey("new".into()), Source::Keychain))
        );
        assert_eq!(store.load(), Stored::Has("new".into()));

        auth.remove().unwrap();
        assert_eq!(
            sends_with(&auth),
            Some((Credentials::OAuth("ant-tok".into()), Source::Ant))
        );
        assert_eq!(store.load(), Stored::Empty);
    }

    #[test]
    fn failed_save_keeps_what_was_there() {
        let store = store(Stored::Has("old".into()));
        let mut auth = auth(None, store.clone(), None);
        store.fail_with("locked");
        assert_eq!(auth.save("new"), Err("locked".into()));
        assert_eq!(
            sends_with(&auth),
            Some((Credentials::ApiKey("old".into()), Source::Keychain))
        );
    }
}
