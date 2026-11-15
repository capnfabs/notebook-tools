//! Adds methods to the application delegate winit installs, for what AppKit asks it and winit
//! doesn't answer: whether to quit ([`crate::quit`]) and which files to open
//! ([`crate::finder_open`]).

use std::ffi::CStr;

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send};

/// Why [`add_method`] didn't add the method.
#[derive(Debug)]
pub enum NotAdded {
    /// The application has no delegate yet.
    NoDelegate,
    /// The delegate already has a method by that name, which is left alone.
    AlreadyThere,
}

/// Adds `imp` to the application delegate's class as the method `sel`, with the Objective-C type
/// encoding `types`.
///
/// # Safety
///
/// Call on the main thread, once the application has its delegate. `imp` must be an
/// `extern "C-unwind"` function taking the receiver, the selector and `sel`'s arguments, matching
/// `types`.
pub unsafe fn add_method(sel: Sel, imp: Imp, types: &CStr) -> Result<(), NotAdded> {
    // SAFETY: on the main thread, as the caller promises, where the application and its delegate
    // live; the method's signature is the caller's to get right.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![app, delegate];
        let Some(delegate) = delegate.as_ref() else {
            return Err(NotAdded::NoDelegate);
        };
        let class = delegate.class() as *const AnyClass as *mut AnyClass;
        if !objc2::ffi::class_addMethod(class, sel, imp, types.as_ptr()).as_bool() {
            return Err(NotAdded::AlreadyThere);
        }
        // AppKit may have noted what the delegate does when it was set.
        let _: () = msg_send![app, setDelegate: delegate];
    }
    Ok(())
}
