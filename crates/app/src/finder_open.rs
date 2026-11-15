//! Opens the files Finder and the Dock ask the app to open: a double-clicked project, photos
//! opened with Open With, files dropped on the Dock icon. AppKit hands them to the application
//! delegate's `application:openURLs:`, which winit's delegate doesn't have, so it's added
//! ([`crate::app_delegate`]). That has to happen once the delegate exists but before launching
//! finishes, as the files that launched the app arrive before `applicationDidFinishLaunching:`,
//! which is when eframe builds the app: so on `NSApplicationWillFinishLaunchingNotification`. Each
//! batch goes to the app as [`OpenedFiles`], to be taken as if it had been dropped on the window.
//! See `decisions/2026-11-15-macos-bundle.md`.

use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc;

use eframe::egui;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, NSObject, Sel};
use objc2::{ClassType, define_class, msg_send, sel};
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSURL, ns_string};

use crate::app_delegate::{self, NotAdded};
use crate::ingest::OpenedFiles;

/// `application:openURLs:`'s types: returns nothing, takes the receiver, the selector, the
/// application and an array of URLs.
const TYPES: &CStr = c"v@:@@";

static SENDER: OnceLock<mpsc::Sender<Vec<PathBuf>>> = OnceLock::new();
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this doesn't implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "NotebookToolsLaunchObserver"]
    struct LaunchObserver;

    impl LaunchObserver {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            add_open_urls();
        }
    }
);

/// Starts listening for files to open, which come out of what's returned. Call on the main
/// thread, once, before eframe starts.
pub fn install() -> OpenedFiles {
    let (send, receive) = mpsc::channel();
    if SENDER.set(send).is_err() {
        log::warn!("already listening for files to open");
    }
    // SAFETY: `new` on an NSObject subclass gives a retained, initialized instance.
    let observer: Retained<LaunchObserver> = unsafe { msg_send![LaunchObserver::class(), new] };
    // SAFETY: the observer has the method the selector names, taking the notification.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &observer,
            sel!(willFinishLaunching:),
            Some(ns_string!("NSApplicationWillFinishLaunchingNotification")),
            None,
        );
    }
    // The notification centre doesn't keep its observers alive.
    std::mem::forget(observer);
    receive
}

/// Repaints the app with `ctx` when files arrive, so it takes them at once.
pub fn set_context(ctx: &egui::Context) {
    CONTEXT.set(ctx.clone()).ok();
}

/// Gives the application delegate `application:openURLs:`.
fn add_open_urls() {
    // SAFETY: on the main thread, as notifications posted there are delivered there, once winit
    // has set its delegate. The method's signature matches `TYPES` and `application:openURLs:`.
    let added = unsafe {
        let imp: Imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
            Imp,
        >(open_urls);
        app_delegate::add_method(sel!(application:openURLs:), imp, TYPES)
    };
    match added {
        Ok(()) => {}
        Err(NotAdded::NoDelegate) => {
            log::warn!("no application delegate: files opened from Finder won't open");
        }
        Err(NotAdded::AlreadyThere) => {
            log::warn!("the application delegate already opens files; leaving it");
        }
    }
}

extern "C-unwind" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *mut NSArray<NSURL>,
) {
    // SAFETY: AppKit passes an array of URLs, valid for the call.
    let Some(urls) = (unsafe { urls.as_ref() }) else {
        return;
    };
    let paths: Vec<PathBuf> = urls.iter().filter_map(|url| url.to_file_path()).collect();
    log::debug!("Asked to open {} file(s)", paths.len());
    if paths.is_empty() {
        return;
    }
    if let Some(send) = SENDER.get() {
        let _ = send.send(paths);
    }
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}
