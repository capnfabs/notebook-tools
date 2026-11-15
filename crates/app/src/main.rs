//! Starts the app with real credentials (none with `--offline`), cache, settings file and native
//! dialogs, and the project named on the command line open, if any
//! (`decisions/2026-10-31-open-by-drop-and-argument.md`); or clears the cache and exits, if the
//! command line says to. Tests build [`NotebookApp`] with stand-ins for these instead.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use notebook_app::cli::{self, Launch};
use notebook_app::{NotebookApp, Pick, t};
use notebook_core::cache::Cache;
use notebook_core::credentials::ant::{Ant, LoggedIn};
use notebook_core::credentials::{self, Credentials, Source};
use notebook_core::settings::Settings;
use notebook_core::{decode, project};

/// How long the window waits for `ant` to say whether it's logged in, once it's ready to open.
const ANT_WAIT: Duration = Duration::from_secs(3);

fn main() -> eframe::Result {
    env_logger::init();
    let (project, offline, no_keychain) = match cli::launch(std::env::args_os().skip(1)) {
        Ok(Launch::Open {
            project,
            offline,
            no_keychain,
        }) => (project, offline, no_keychain),
        Ok(Launch::ClearCaches) => match cli::clear_caches(Cache::default_dir()) {
            Ok(done) => {
                println!("{done}");
                return Ok(());
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };
    let env = (!offline).then(credentials::from_env).flatten();
    // Only needed without the environment's credentials; asked while the window opens.
    let ant = (!offline && env.is_none()).then(ant_login);
    // Before eframe starts, as the files that launched the app arrive before it builds the app.
    #[cfg(target_os = "macos")]
    let opened_files = notebook_app::finder_open::install();
    eframe::run_native(
        &t!("app-name"),
        notebook_app::native_options(),
        Box::new(move |cc| {
            let cache = cache();
            let app = NotebookApp::with_transcription(None, cache.clone());
            let app = if offline {
                log::info!("Offline: cached transcripts load, but nothing is sent");
                app
            } else {
                app.with_auth(auth(env, no_keychain, ant, cache))
            };
            let app = app
                .with_save_path(save_dialog())
                .with_file_picker(file_picker());
            let app = match Settings::default_path() {
                Some(path) => app.with_settings_file(path),
                None => app,
            };
            // Only the macOS menu needs the context.
            #[cfg(not(target_os = "macos"))]
            let _ = cc;
            #[cfg(target_os = "macos")]
            let app = {
                notebook_app::finder_open::set_context(&cc.egui_ctx);
                app.with_opened_files(opened_files).with_frontend(Box::new(
                    notebook_app::menu::NativeMenu::install(&cc.egui_ctx),
                ))
            };
            // Last, so the transcripts are restyled in the settings' style.
            let app = match project {
                Some((path, opened)) => app.with_project(&path, opened),
                None => app,
            };
            Ok(Box::new(app))
        }),
    )
}

/// Asks where to save with the system's save dialog, offering the suggested name's file type.
/// See `decisions/2026-10-23-rfd-save-dialog.md`.
fn save_dialog() -> notebook_app::SavePath {
    std::sync::Arc::new(|suggested| {
        let extension = std::path::Path::new(suggested)
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        let kind = match &*extension {
            "pdf" => t!("file-type-pdf"),
            project::EXTENSION => t!("file-type-project"),
            _ => t!("file-type-markdown"),
        };
        rfd::FileDialog::new()
            .add_filter(kind, &[&*extension])
            .set_file_name(suggested)
            .save_file()
    })
}

/// Asks which photos to add, or which project to open, with the system's open dialog.
fn file_picker() -> notebook_app::PickFiles {
    std::sync::Arc::new(|pick| match pick {
        Pick::Photos => rfd::FileDialog::new()
            .add_filter(t!("file-type-photos"), &decode::EXTENSIONS)
            .pick_files()
            .unwrap_or_default(),
        Pick::Project => rfd::FileDialog::new()
            .add_filter(t!("file-type-project"), &[project::EXTENSION])
            .pick_file()
            .into_iter()
            .collect(),
    })
}

/// Where transcriptions are cached, if the system has somewhere: see
/// `decisions/2026-10-31-transcription-cache.md`.
fn cache() -> Option<Cache> {
    let cache = Cache::default_dir().map(Cache::new);
    match &cache {
        Some(cache) => log::info!("Caching transcriptions in {}", cache.dir().display()),
        None => log::warn!("No cache directory: transcriptions won't be cached"),
    }
    cache
}

/// `ant`'s login, if it's installed and logged in, asked for on another thread.
fn ant_login() -> mpsc::Receiver<Option<LoggedIn>> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let found = Ant::installed().and_then(|ant| match ant.login() {
            Ok(login) => Some((Arc::new(ant), login)),
            Err(err) => {
                log::info!("Not using ant: {err}");
                None
            }
        });
        let _ = send.send(found);
    });
    receive
}

/// Transcribes with Claude, with credentials from the environment, else the keychain (unless
/// `no_keychain`), else `ant`, caching what comes back in `cache`. Without credentials, cached transcripts still load, and
/// transcribing asks for a key: see `decisions/2026-11-15-api-key-in-keychain.md` and
/// `decisions/2026-11-15-ant-login-fallback.md`.
fn auth(
    env: Option<(Credentials, Source)>,
    no_keychain: bool,
    ant: Option<mpsc::Receiver<Option<LoggedIn>>>,
    cache: Option<Cache>,
) -> notebook_app::Auth {
    use notebook_app::eval_log;
    use notebook_core::claude_transcribe::{check_key, fetch_page};
    use notebook_core::credentials::{KeyStore, Keychain, MemoryStore, Stored};

    let logged_cache = cache;
    let make_transcriber = move |login| {
        let fetch = move |bytes: &[u8], params: &_, corners, model| {
            fetch_page(&login, bytes, params, corners, model)
        };
        let transcriber = notebook_app::Transcriber::new(Arc::new(fetch));
        // Comparing reads other models' cache entries: only worth it if someone's reading the log.
        match &logged_cache {
            Some(cache) if eval_log::enabled() => {
                transcriber.on_transcribe_completed(eval_log::log_comparisons(cache.clone()))
            }
            _ => transcriber,
        }
    };
    let ant = ant.and_then(|ant| {
        ant.recv_timeout(ANT_WAIT).unwrap_or_else(|_| {
            log::warn!(
                "Not using ant: it didn't answer within {}s",
                ANT_WAIT.as_secs()
            );
            None
        })
    });
    let store: Arc<dyn KeyStore> = if no_keychain {
        let why = format!("not used, as the app was started with {}", cli::NO_KEYCHAIN);
        Arc::new(MemoryStore::new(Stored::Unavailable(why)))
    } else {
        Arc::new(Keychain)
    };
    let auth = notebook_app::Auth::new(
        env,
        store,
        ant,
        Arc::new(check_key),
        Arc::new(make_transcriber),
    );
    match auth.current() {
        Some((_, source)) => log::info!("Transcribing with credentials from {source:?}"),
        None => log::info!("No credentials: transcribing asks for an API key"),
    }
    auth
}
