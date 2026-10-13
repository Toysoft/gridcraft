//! macOS document-window interop for the app shell, behind a safe API.
//!
//! A macOS document window carries its file: the title bar shows the file's icon (the *proxy
//! icon*), dragging it out of the window drags the file (into Finder, an e-mail, a chat), and the
//! close button wears the edited dot while the document has unsaved changes. AppKit drives all of
//! this, and winit 0.30 exposes only the edited flag (`WindowExtMacOS::set_document_edited`,
//! safe): the icon and the drag need AppKit, i.e. Objective-C messaging, i.e. `unsafe`, which the
//! workspace forbids everywhere else (the same story as `fmv-macos-events` around Apple events,
//! #69). This crate holds exactly that interop, behind safe, fallible functions.
//!
//! There are two ways to show the icon, and both live here. The native way is
//! [`set_represented_filename`]: AppKit draws the icon after the traffic lights and drags out of
//! it, in a title bar AppKit owns. An app that paints its own title bar (GridCraft's does,
//! `ribbon.rs` in `gridcraft-ui-egui`) instead leaves the represented file unset, draws the
//! file's real Finder icon wherever its design puts it ([`file_icon_png`]), and turns a drag of
//! that icon into a native drag of the file ([`begin_file_drag`]). [`set_titlebar_transparent`]
//! goes with a painted bar that keeps the native traffic lights.
//!
//! Call on the main thread (winit windows are main-thread on macOS anyway); every other thread
//! returns an error. The `unsafe` is confined to the Objective-C message sends behind these
//! functions and the view→window lookup; each is a documented AppKit call with no invariants
//! beyond "main thread, real window".

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, runtime::NSObject, runtime::NSObjectProtocol};
use objc2_app_kit::{
    NSApplication, NSBitmapImageFileType, NSBitmapImageRep, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGSize};
use objc2_foundation::{NSArray, NSDictionary, NSRect, NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The call failed; the window keeps its previous state and nothing was dragged or drawn.
#[derive(Debug)]
pub enum Error {
    /// The platform window could not be reached (`HasWindowHandle` failed).
    WindowHandle(raw_window_handle::HandleError),
    /// The window is not an AppKit window (wrong platform or headless).
    NotAppKit,
    /// The view has no window yet (not on screen), or the mouse is not down: a drag can only
    /// begin inside a mouse drag.
    Window,
    /// Called from a thread AppKit does not consider the main thread.
    MainThread,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::WindowHandle(e) => write!(f, "window handle unavailable: {e}"),
            Error::NotAppKit => write!(f, "not an AppKit window"),
            Error::Window => write!(f, "the view has no window, or the mouse is not down"),
            Error::MainThread => write!(f, "not on the main thread"),
        }
    }
}

impl std::error::Error for Error {}

/// The file's Finder icon, as PNG bytes, at the requested square size.
///
/// What the proxy icon would show: LaunchServices' icon for the file's type. Decode with an image
/// library and draw it (an app that paints its own title bar keeps the native icon look without
/// setting a represented file).
pub fn file_icon_png(path: &str, size_px: u32) -> Result<Vec<u8>, Error> {
    let icon = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path));
    icon.setSize(CGSize { width: size_px as f64, height: size_px as f64 });
    let tiff = icon.TIFFRepresentation().ok_or(Error::Window)?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or(Error::Window)?;
    let properties = NSDictionary::new();
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties) };
    Ok(png.ok_or(Error::Window)?.to_vec())
}

/// Sets (or clears) the window's represented file — the native proxy icon in the title bar.
///
/// `Some(path)` makes AppKit draw the file's icon after the traffic lights and drag out of it;
/// `None` clears both. This is the native layout strategy (see the crate docs): for an app that
/// paints its own title bar, use [`file_icon_png`] + [`begin_file_drag`] instead, so the icon
/// sits where the design puts it.
pub fn set_represented_filename(window: &impl HasWindowHandle, path: Option<&str>) -> Result<(), Error> {
    let ns_window = ns_window(window)?;
    let filename = NSString::from_str(path.unwrap_or(""));
    unsafe {
        let _: () = msg_send![ns_window, setRepresentedFilename: &*filename];
    }
    Ok(())
}

/// Makes the title bar transparent so the app's own drawing shows through it.
///
/// Pairs with a full-size content view and a shown (but app-painted) title bar: the traffic
/// lights stay native, while the background becomes the app's. winit has this
/// (`WindowAttributes::with_titlebar_transparent`) but eframe's `ViewportBuilder` cannot express
/// it, so call this once the window exists.
pub fn set_titlebar_transparent(window: &impl HasWindowHandle, transparent: bool) -> Result<(), Error> {
    let ns_window = ns_window(window)?;
    unsafe {
        let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: transparent];
    }
    Ok(())
}

define_class!(
    // The drag source AppKit calls back on. Every callback the protocol offers is optional and
    // irrelevant for a file drag out of a painted icon, except the operation mask, which answers
    // "any operation" so every drop target accepts the file as itself.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct FileDragSource;

    unsafe impl NSObjectProtocol for FileDragSource {}

    unsafe impl NSDraggingSource for FileDragSource {
        // objc2 names protocol methods after their Objective-C selectors.
        #[allow(non_snake_case)]
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn draggingSession_sourceOperationMaskForDraggingContext(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Every
        }
    }
);

// Keeps the drag sources of recent drags alive. AppKit holds the source only for the session, and
// the session outlives this call; parking it here (the last few, bounded) costs a few bytes each
// and takes the lifetime question off the caller.
thread_local! {
    static DRAG_SOURCES: std::cell::RefCell<Vec<Retained<FileDragSource>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Starts a native drag of the file out of the window — the proxy icon's drag.
///
/// Call while the user is holding the mouse down on the drawn icon (the same frame it saw the
/// drag start): the running mouse gesture becomes a drag of `path`, which Finder, Mail or a chat
/// can accept. The drag image is the file's Finder icon. `Ok` means the drag session took over;
/// the mouse events it consumes end when the user drops the file.
pub fn begin_file_drag(window: &impl HasWindowHandle, path: &str) -> Result<(), Error> {
    let mtm = MainThreadMarker::new().ok_or(Error::MainThread)?;
    let ns_window = ns_window(window)?;
    // The gesture in flight: a drag session must begin with the mouse down on our window.
    let event = NSApplication::sharedApplication(mtm).currentEvent().ok_or(Error::Window)?;

    let file = NSString::from_str(path);
    let url = NSURL::fileURLWithPath_isDirectory(&file, false);
    let item = NSDraggingItem::initWithPasteboardWriter(mtm.alloc::<NSDraggingItem>(), ProtocolObject::from_ref(&*url));
    // The image while dragging is the file's Finder icon, at its natural size, under the cursor.
    let icon = NSWorkspace::sharedWorkspace().iconForFile(&file);
    let size = icon.size();
    unsafe {
        item.setDraggingFrame_contents(NSRect::new(CGPoint::ZERO, size), Some(&icon));
    }
    // NSObject's `init` completes the allocation; the class overrides no initializer.
    let source: Retained<FileDragSource> = unsafe { msg_send![super(mtm.alloc::<FileDragSource>().set_ivars(())), init] };
    let items = NSArray::from_retained_slice(&[item]);
    // `source` answers "any operation" (see `FileDragSource`), so every drop target accepts the
    // file as itself.
    let _session = unsafe {
        // Cast: the window was found through its AppKit view, on the main thread.
        let w = &*(ns_window as *mut objc2_app_kit::NSWindow);
        w.beginDraggingSessionWithItems_event_source(&items, &event, ProtocolObject::from_ref(&*source))
    };
    DRAG_SOURCES.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() >= 8 {
            s.remove(0);
        }
        s.push(source);
    });
    Ok(())
}

/// The `NSWindow` behind a window handle, as a borrowed +0 pointer valid for the call.
fn ns_window(window: &impl HasWindowHandle) -> Result<*mut AnyObject, Error> {
    if MainThreadMarker::new().is_none() {
        return Err(Error::MainThread);
    }
    let handle = window.window_handle().map_err(Error::WindowHandle)?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(Error::NotAppKit);
    };
    // SAFETY: the view pointer is borrowed from the window, which outlives this call and keeps it
    // alive; message it as +0 (no retain) and never deallocate it here.
    let view = appkit.ns_view.as_ptr() as *mut AnyObject;
    if view.is_null() {
        return Err(Error::NotAppKit);
    }
    // SAFETY: `-[NSView window]` is a plain accessor; a view off screen has none.
    let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
    if ns_window.is_null() {
        return Err(Error::Window);
    }
    Ok(ns_window)
}

#[cfg(test)]
mod tests {
    use super::Error;
    use raw_window_handle::HasWindowHandle;

    #[test]
    fn not_on_the_main_thread_errors() {
        // Tests run on a worker thread; the main-thread guard is the one path a unit test can
        // reach without a window server session.
        struct Any;
        impl HasWindowHandle for Any {
            fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
                unreachable!("guarded before the handle is touched")
            }
        }
        assert!(matches!(super::begin_file_drag(&Any, "/tmp/x"), Err(Error::MainThread)));
    }
}
