//! The `ant` command line's login, the last place credentials come from: `ant auth
//! print-credentials` gives an OAuth access token, refreshing it itself when it's near expiry. The
//! token is kept until shortly before it expires, or until the API rejects it, then asked for
//! again. Any `ANTHROPIC_BASE_URL` its profile sets is ignored.
//!
//! See `decisions/2026-11-15-ant-login-fallback.md`.

use std::ffi::OsStr;
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

/// How long before it expires a token is asked for again, in seconds.
const REFRESH_MARGIN: u64 = 5 * 60;
/// How long `ant` gets to answer: it may be refreshing the token over the network.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Where apps launched from Finder, which don't get the shell's `PATH`, may still find `ant`.
fn fallback_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    dirs.extend(home.map(|home| home.join(".local/bin")));
    dirs
}

/// The `ant` program in `path_var`'s directories, else in [`fallback_dirs`]; whichever comes
/// first that `is_file` says is there.
pub fn find(
    path_var: Option<&OsStr>,
    home: Option<&Path>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let name = format!("ant{}", std::env::consts::EXE_SUFFIX);
    path_var
        .map(|path| std::env::split_paths(path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(fallback_dirs(home))
        .map(|dir| dir.join(&name))
        .find(|candidate| is_file(candidate))
}

/// What `ant auth print-credentials` says, less its refresh token, which is never kept.
#[derive(Clone, PartialEq, Deserialize)]
pub struct AntLogin {
    pub access_token: String,
    /// When the token expires, in seconds since the Unix epoch, if it does.
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub organization_name: Option<String>,
    #[serde(default)]
    pub account_email: Option<String>,
}

impl fmt::Debug for AntLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AntLogin")
            .field("access_token", &"..")
            .field("expires_at", &self.expires_at)
            .field("organization_name", &self.organization_name)
            .field("account_email", &self.account_email)
            .finish()
    }
}

impl AntLogin {
    /// Reads `ant auth print-credentials`' JSON.
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        let login: Self = serde_json::from_slice(json)
            .map_err(|e| format!("unexpected output from `ant`: {e}"))?;
        if login.access_token.trim().is_empty() {
            return Err("`ant` gave no access token".to_owned());
        }
        Ok(login)
    }

    /// Whether the token is still good at `now` (seconds since the Unix epoch), with time to spare.
    fn fresh_at(&self, now: u64) -> bool {
        self.expires_at
            .is_none_or(|expires| now.saturating_add(REFRESH_MARGIN) < expires)
    }
}

/// `ant`, logged in, with what it said when asked.
pub type LoggedIn = (Arc<Ant>, AntLogin);

/// Runs `ant auth print-credentials`, giving its output or why it failed.
pub type RunAnt = Box<dyn Fn() -> Result<Vec<u8>, String> + Send + Sync>;

/// The `ant` login, asked for when there's no fresh token. Shared by every page being sent, which
/// take turns, so only one asks `ant` at a time.
pub struct Ant {
    run: RunAnt,
    /// Seconds since the Unix epoch.
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    cached: Mutex<Option<AntLogin>>,
}

impl Ant {
    /// Gets the login from `run`.
    pub fn new(run: RunAnt) -> Self {
        Self::with_clock(run, Arc::new(unix_now))
    }

    fn with_clock(run: RunAnt, now: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self {
            run,
            now,
            cached: Mutex::new(None),
        }
    }

    /// The `ant` installed on this system, if there is one. Doesn't run it.
    pub fn installed() -> Option<Self> {
        let path = find(
            std::env::var_os("PATH").as_deref(),
            dirs::home_dir().as_deref(),
            Path::is_file,
        )?;
        log::debug!("found ant at {}", path.display());
        Some(Self::new(Box::new(move || run_ant(&path, TIMEOUT))))
    }

    /// The login, from `ant` if there's no fresh token kept from last time.
    pub fn login(&self) -> Result<AntLogin, String> {
        let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(login) = &*cached
            && login.fresh_at((self.now)())
        {
            return Ok(login.clone());
        }
        *cached = None;
        let login = AntLogin::parse(&(self.run)()?)?;
        *cached = Some(login.clone());
        Ok(login)
    }

    /// The login kept from last time, if any, without asking `ant`.
    pub fn last_login(&self) -> Option<AntLogin> {
        self.cached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Forgets the token, as when the API rejected it, so the next request asks `ant` again.
    pub fn forget(&self) {
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Runs `program auth print-credentials`, killing it if it takes longer than `timeout`.
fn run_ant(program: &Path, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = Command::new(program)
        .args(["auth", "print-credentials"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't run {}: {e}", program.display()))?;
    // Read on threads, so a full pipe can't keep `ant` from finishing.
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = read(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = read(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("`ant` didn't answer within {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("couldn't wait for `ant`: {e}")),
        }
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        let why = String::from_utf8_lossy(&stderr).trim().to_owned();
        Err(if why.is_empty() {
            format!("`ant` failed ({status})")
        } else {
            why
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use super::*;

    const JSON: &str = r#"{"version":"1.0","type":"oauth_token","access_token":"tok-1",
        "expires_at":10000,"refresh_token":"refresh","organization_name":"Example Org",
        "account_email":"someone@example.com","workspace_name":"Default"}"#;

    #[test]
    fn find_looks_in_path_then_fallbacks() {
        let exe =
            |dir: &str| PathBuf::from(dir).join(format!("ant{}", std::env::consts::EXE_SUFFIX));
        let path = std::env::join_paths(["/first", "/second"]).unwrap();
        let home = Path::new("/home/me");
        let found =
            |there: &[PathBuf]| find(Some(&path), Some(home), |p| there.iter().any(|t| t == p));
        assert_eq!(
            found(&[exe("/second"), exe("/opt/homebrew/bin")]),
            Some(exe("/second"))
        );
        assert_eq!(found(&[exe("/usr/local/bin")]), Some(exe("/usr/local/bin")));
        assert_eq!(
            found(&[exe("/home/me/.local/bin")]),
            Some(exe("/home/me/.local/bin"))
        );
        assert_eq!(found(&[]), None);
        // No PATH at all, as from Finder.
        assert_eq!(
            find(None, None, |p| p == exe("/opt/homebrew/bin")),
            Some(exe("/opt/homebrew/bin"))
        );
    }

    #[test]
    fn parse_keeps_the_token_and_account_but_not_the_refresh_token() {
        let login = AntLogin::parse(JSON.as_bytes()).unwrap();
        assert_eq!(
            login,
            AntLogin {
                access_token: "tok-1".into(),
                expires_at: Some(10000),
                organization_name: Some("Example Org".into()),
                account_email: Some("someone@example.com".into()),
            }
        );
        assert!(!format!("{login:?}").contains("tok-1"));
        assert!(AntLogin::parse(b"not json").is_err());
        assert!(AntLogin::parse(br#"{"access_token":" "}"#).is_err());
    }

    /// An [`Ant`] whose clock is `now`, answering with `JSON` and counting its runs.
    fn counted(now: Arc<AtomicU64>) -> (Ant, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = runs.clone();
        let run: RunAnt = Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(JSON.as_bytes().to_vec())
        });
        let ant = Ant::with_clock(run, Arc::new(move || now.load(Ordering::SeqCst)));
        (ant, runs)
    }

    #[test]
    fn token_is_kept_until_near_expiry_or_forgotten() {
        let now = Arc::new(AtomicU64::new(1000));
        let (ant, runs) = counted(now.clone());
        assert_eq!(ant.last_login(), None);
        assert_eq!(ant.login().unwrap().access_token, "tok-1");
        ant.login().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(ant.last_login().is_some());

        now.store(10000 - REFRESH_MARGIN - 1, Ordering::SeqCst);
        ant.login().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1, "still fresh");
        now.store(10000 - REFRESH_MARGIN, Ordering::SeqCst);
        ant.login().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2, "near expiry");

        now.store(1000, Ordering::SeqCst);
        ant.forget();
        assert_eq!(ant.last_login(), None);
        ant.login().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 3, "forgotten");
    }

    #[test]
    fn pages_sent_together_ask_ant_once() {
        let (ant, runs) = counted(Arc::new(AtomicU64::new(1000)));
        let ant = Arc::new(ant);
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let ant = ant.clone();
                std::thread::spawn(move || ant.login().unwrap())
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failing_ant_is_an_error_and_nothing_is_kept() {
        let ant = Ant::new(Box::new(|| Err("not logged in".into())));
        assert_eq!(ant.login(), Err("not logged in".into()));
        assert_eq!(ant.last_login(), None);
    }

    #[cfg(unix)]
    mod subprocess {
        use std::os::unix::fs::PermissionsExt as _;

        use super::*;

        /// A fake `ant` running `script`, in a directory removed when the test ends.
        fn fake_ant(script: &str) -> (tempfile::TempDir, PathBuf) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("ant");
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            (dir, path)
        }

        #[test]
        fn runs_print_credentials() {
            let (_dir, ant) = fake_ant(
                r#"[ "$1 $2" = "auth print-credentials" ] && echo '{"access_token":"tok"}'"#,
            );
            let out = run_ant(&ant, TIMEOUT).unwrap();
            assert_eq!(AntLogin::parse(&out).unwrap().access_token, "tok");
        }

        #[test]
        fn failure_gives_ants_message() {
            let (_dir, ant) = fake_ant("echo 'not logged in' >&2; exit 1");
            assert_eq!(run_ant(&ant, TIMEOUT), Err("not logged in".into()));
        }

        #[test]
        fn slow_ant_is_killed() {
            let (_dir, ant) = fake_ant("exec sleep 10");
            let started = Instant::now();
            let result = run_ant(&ant, Duration::from_millis(200));
            assert!(result.unwrap_err().contains("didn't answer"));
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}
