//! macOS open-documents and quit Apple events (#69).
//!
//! Finder double-clicks, Open With, drops on the Dock icon and `open -a GridCraft book.xlsx` don't
//! pass paths on the command line: LaunchServices sends the running (or just-launched) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`, so AppKit answered "GridCraft cannot open files in the Excel Workbook
//! format". Handling it ourselves needs Objective-C class declarations, i.e. `unsafe`, which this
//! workspace forbids. The audited `fmv-macos-events` crate (also used by PdfCraft and PhotoCraft)
//! wraps exactly that, an `NSAppleEventManager` handler registered before Finder's launch event
//! that leaves winit's delegate alone, behind a safe main-thread API.

use fmv_macos_events::{Event, Inbox, Registration};

/// Keeps the Apple-event handlers registered; hold it until the event loop returns.
pub struct AppleEvents {
    _registration: Registration,
    inbox: Inbox,
}

impl AppleEvents {
    /// Register the handlers. Call on the main thread before the event loop starts, so the event
    /// that launched the app (a Finder double-click) is caught too.
    pub fn install() -> Self {
        let (registration, inbox) = Registration::install();
        Self { _registration: registration, inbox }
    }

    /// The queue the app drains every frame ([`poll`]); events arriving later wake `ctx`.
    pub fn connect(&self, ctx: &egui::Context) -> Inbox {
        let ctx = ctx.clone();
        self.inbox.set_wake(move || ctx.request_repaint());
        self.inbox.clone()
    }
}

/// Events received since the last frame. The desktop shell routes open requests because it owns
/// the workbook windows and can focus an existing one or replace an untouched blank workbook.
pub fn drain(inbox: &Inbox) -> Vec<Event> {
    inbox.drain()
}
