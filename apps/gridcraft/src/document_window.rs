//! macOS document-window state: the title bar's file icon, its drag, and the edited dot.
//!
//! A macOS document window carries its file: the close button wears the edited dot while there
//! are unsaved changes, and the file's icon drags out of the window (into Finder, Mail, a chat).
//! winit 0.30 exposes the edited flag (`WindowExtMacOS::set_document_edited`, safe); the icon and
//! the drag are [`gridcraft_macos_window`]'s (see its docs for why the interop lives there).

/// Applies the active document's edited state to the window, only when it changes.
#[derive(Default)]
pub struct DocumentWindow {
    /// The edited flag the window was last told about.
    last_edited: Option<bool>,
    /// The file the title-bar icon was loaded for, so it is loaded only when the file changes.
    pub doc_icon_path: Option<String>,
}

impl DocumentWindow {
    /// Tell the window about the active document's edited state. Call once per frame with the
    /// frame the callback got; a frame without a window yet (still starting up) is skipped and
    /// retried next frame.
    pub fn sync(&mut self, frame: &eframe::Frame, dirty: bool) {
        if self.last_edited == Some(dirty) {
            return;
        }
        self.last_edited = Some(dirty);
        let Some(window) = frame.winit_window() else { return };
        use winit::platform::macos::WindowExtMacOS;
        // The dot on the close button; standard even for an untitled document with changes.
        window.set_document_edited(dirty);
    }
}

/// The active document's Finder icon as a texture, or `None` without a file (an untitled book has
/// nothing to show or drag).
pub fn load_doc_icon(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    let png = gridcraft_macos_window::file_icon_png(path, 32).ok()?;
    let img = image::load_from_memory(&png).ok()?.to_rgba8();
    let size = [img.width() as _, img.height() as _];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("document_icon", color, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::DocumentWindow;

    #[test]
    fn default_starts_with_no_state() {
        // The first sync must reach the window: nothing has been applied before it.
        let window = DocumentWindow::default();
        assert_eq!(window.last_edited, None);
        assert_eq!(window.doc_icon_path, None);
    }
}
