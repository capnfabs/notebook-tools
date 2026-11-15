//! Which credentials pages are sent with: the environment's, else a key saved in the keychain.
//! The keychain is read at launch only when the environment has none, else when the user first
//! asks to enter a key. See `decisions/2026-11-15-api-key-in-keychain.md`.

use std::sync::Arc;

use notebook_core::claude_transcribe::OcrError;
use notebook_core::credentials::{Credentials, KeyStore, Source, Stored};

use crate::transcriptions::Transcriber;

/// Whether the API accepts some credentials. Runs off the UI thread.
pub type CheckKey = Arc<dyn Fn(&Credentials) -> Result<(), OcrError> + Send + Sync>;

/// A transcriber that sends pages with the credentials given.
pub type MakeTranscriber = Arc<dyn Fn(Credentials) -> Transcriber + Send + Sync>;

pub struct Auth {
    /// From the environment, which wins over the keychain.
    env: Option<(Credentials, Source)>,
    store: Arc<dyn KeyStore>,
    /// What the store held when last read, or `None` before it's read.
    stored: Option<Stored>,
    check: CheckKey,
    make_transcriber: MakeTranscriber,
}

impl Auth {
    /// Reads `store` straight away, unless `env` has credentials.
    pub fn new(
        env: Option<(Credentials, Source)>,
        store: Arc<dyn KeyStore>,
        check: CheckKey,
        make_transcriber: MakeTranscriber,
    ) -> Self {
        let mut auth = Self {
            env,
            store,
            stored: None,
            check,
            make_transcriber,
        };
        if auth.env.is_none() {
            auth.stored();
        }
        auth
    }

    /// The credentials pages are sent with, and where they came from.
    pub fn current(&self) -> Option<(Credentials, Source)> {
        self.env.clone().or_else(|| match &self.stored {
            Some(Stored::Has(key)) => Some((Credentials::ApiKey(key.clone()), Source::Keychain)),
            _ => None,
        })
    }

    /// A transcriber with [`Self::current`]'s credentials, if there are any.
    pub fn transcriber(&self) -> Option<Transcriber> {
        let (credentials, _) = self.current()?;
        Some((self.make_transcriber)(credentials))
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

    fn auth(env: Option<(Credentials, Source)>, store: Arc<Counting>) -> Auth {
        let make: MakeTranscriber =
            Arc::new(|_| Transcriber::new(Arc::new(|_, _, _, _| unreachable!())));
        Auth::new(env, store, Arc::new(|_| Ok(())), make)
    }

    fn counting(stored: Stored) -> Arc<Counting> {
        Arc::new(Counting(MemoryStore::new(stored), AtomicUsize::new(0)))
    }

    #[test]
    fn keychain_key_used_without_env() {
        let store = counting(Stored::Has("saved".into()));
        let auth = auth(None, store.clone());
        assert_eq!(store.1.load(Ordering::SeqCst), 1);
        assert_eq!(
            auth.current(),
            Some((Credentials::ApiKey("saved".into()), Source::Keychain))
        );
        assert!(auth.transcriber().is_some());
    }

    #[test]
    fn env_wins_and_keychain_is_read_only_when_asked() {
        let store = counting(Stored::Has("saved".into()));
        let env = (Credentials::OAuth("tok".into()), Source::EnvAuthToken);
        let mut auth = auth(Some(env.clone()), store.clone());
        assert_eq!(store.1.load(Ordering::SeqCst), 0);
        assert_eq!(auth.current(), Some(env.clone()));
        assert!(!auth.has_saved_key());

        assert_eq!(auth.stored(), &Stored::Has("saved".into()));
        auth.stored();
        assert_eq!(store.1.load(Ordering::SeqCst), 1);
        assert!(auth.has_saved_key());
        assert_eq!(auth.current(), Some(env));
    }

    #[test]
    fn saving_and_removing_change_the_credentials() {
        let store = counting(Stored::Empty);
        let mut auth = auth(None, store.clone());
        assert_eq!(auth.current(), None);
        assert!(auth.transcriber().is_none());

        auth.save("new").unwrap();
        assert_eq!(
            auth.current(),
            Some((Credentials::ApiKey("new".into()), Source::Keychain))
        );
        assert_eq!(store.0.load(), Stored::Has("new".into()));

        auth.remove().unwrap();
        assert_eq!(auth.current(), None);
        assert_eq!(store.0.load(), Stored::Empty);
    }

    #[test]
    fn failed_save_keeps_what_was_there() {
        let store = counting(Stored::Has("old".into()));
        let mut auth = auth(None, store.clone());
        store.0.fail_with("locked");
        assert_eq!(auth.save("new"), Err("locked".into()));
        assert_eq!(
            auth.current(),
            Some((Credentials::ApiKey("old".into()), Source::Keychain))
        );
    }
}
