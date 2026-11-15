//! Lets the app ask to save before macOS quits it from outside the app's own Quit command: Quit
//! in the Dock, logging out, shutting down. Those send `terminate:` to the application, which
//! winit's application delegate lets through without telling the app. So the delegate is given an
//! `applicationShouldTerminate:` that cancels while there are unsaved changes and asks the app to
//! quit instead, as Quit does: [`crate::menu::NativeMenu`] turns that into
//! [`crate::Command::Quit`]. See `decisions/2026-10-31-projects.md`.

use std::ffi::CStr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::sel;

use crate::app_delegate::{self, NotAdded};

/// `NSApplicationTerminateReply`.
const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;

/// `applicationShouldTerminate:`'s types: returns an `NSUInteger`, takes the receiver, the
/// selector, and the application.
const TYPES: &CStr = c"Q@:@";

static UNSAVED: AtomicBool = AtomicBool::new(false);
static REQUESTED: AtomicBool = AtomicBool::new(false);
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Adds `applicationShouldTerminate:` to the application's delegate. Call on the main thread once
/// the app has launched, as the menu bar is installed.
pub fn install(ctx: &egui::Context) {
    CONTEXT.set(ctx.clone()).ok();
    // SAFETY: on the main thread, after launch, so the application and its delegate exist. The
    // method's signature matches `TYPES` and `applicationShouldTerminate:`.
    let added = unsafe {
        let imp: Imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
            Imp,
        >(should_terminate);
        app_delegate::add_method(sel!(applicationShouldTerminate:), imp, TYPES)
    };
    match added {
        Ok(()) => {}
        Err(NotAdded::NoDelegate) => {
            log::warn!("no application delegate: quitting from the Dock won't ask to save");
        }
        Err(NotAdded::AlreadyThere) => {
            log::warn!("the application delegate already decides whether to quit; leaving it");
        }
    }
}

/// Whether quitting now would lose unsaved changes. The app says so each frame.
pub fn set_unsaved(unsaved: bool) {
    UNSAVED.store(unsaved, Ordering::SeqCst);
}

/// Whether macOS asked to quit since this was last called.
pub fn take_request() -> bool {
    REQUESTED.swap(false, Ordering::SeqCst)
}

extern "C-unwind" fn should_terminate(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> usize {
    if !UNSAVED.load(Ordering::SeqCst) {
        return TERMINATE_NOW;
    }
    REQUESTED.store(true, Ordering::SeqCst);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
    TERMINATE_CANCEL
}
