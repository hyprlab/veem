//! Print preview, shown inside Vireo (issue #16).
//!
//! The print dialog's own Preview button belongs to the portal and, in a
//! sandbox, produces nothing. Writing a PDF and handing it to an external viewer
//! was tried and is a chain of things that can each fail quietly — a temporary
//! file, a URI, the document portal, and whatever the desktop has registered for
//! `application/pdf`. None of that is needed to answer the only question a
//! preview is asked: *what will come out of the printer?*
//!
//! So the preview is a window of Vireo's own, showing the message with its print
//! styling applied — the header block, light colours — laid out on a page-shaped
//! sheet. Printing from that window prints the very thing being looked at.

use adw::prelude::*;
use gtk::prelude::*;
use webkit6::prelude::WebViewExt;

/// Open the print dialog for a web view and print it.
///
/// Split in two deliberately: `GtkPrintDialog` collects the settings through a
/// callback, and WebKit prints with them. WebKit's own `run_dialog` spins a
/// nested main loop, and polling a glib future inside one aborts the process.
pub fn print_webview(webview: &webkit6::WebView, job_name: &str, parent: Option<gtk::Window>) {
    let print = webkit6::PrintOperation::new(webview);
    let dialog = gtk::PrintDialog::new();
    dialog.set_title("Print Message");

    let settings = gtk::PrintSettings::new();
    // Names the job in the queue and seeds the filename when printing to a file,
    // which is otherwise "unknown".
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_BASENAME, Some(job_name));
    dialog.set_print_settings(&settings);

    dialog.setup(
        parent.as_ref(),
        gtk::gio::Cancellable::NONE,
        move |result| match result {
            Ok(setup) => {
                print.set_print_settings(&setup.print_settings());
                print.set_page_setup(&setup.page_setup());
                print.connect_failed(|_, error| {
                    tracing::warn!("printing failed: {error}");
                });
                // Keep the operation alive until WebKit says it is done; dropping
                // it here would cancel the job.
                let keep = std::cell::RefCell::new(Some(print.clone()));
                print.connect_finished(move |_| {
                    keep.borrow_mut().take();
                });
                print.print();
            }
            // Dismissing the dialog arrives here as an error; it is the ordinary
            // way to change your mind, not a failure.
            Err(e) => tracing::debug!("print dialog dismissed: {e}"),
        },
    );
}

/// The name of a printer that writes to a file, for saving a PDF.
///
/// Asks GTK rather than assuming: the file printer's name is translated, and
/// enumeration is asynchronous — `wait = true` blocks until the backends have
/// answered, which is why a literal "Print to File" can fail even when the
/// printer exists.
fn file_printer() -> Option<String> {
    let found = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let collector = found.clone();
    gtk::enumerate_printers(
        move |printer| {
            if printer.is_virtual() && printer.accepts_pdf() {
                if let Ok(mut slot) = collector.lock() {
                    *slot = Some(printer.name().to_string());
                }
                return true; // stop at the first one
            }
            false
        },
        true,
    );
    let name = found.lock().ok()?.clone();
    name
}

/// Write the view to a PDF the user picks, with no print dialog in the way.
///
/// The file comes from the portal's file chooser, so the path handed back is one
/// the sandbox may write to; its URI comes from GIO rather than being built by
/// hand, because a mail subject makes a filename full of spaces.
fn save_as_pdf(
    webview: &webkit6::WebView,
    suggested_name: &str,
    parent: &adw::Window,
    toasts: &adw::ToastOverlay,
) {
    let Some(printer) = file_printer() else {
        toasts.add_toast(adw::Toast::new("No PDF writer is available on this system"));
        tracing::warn!("no printer available that can write a PDF");
        return;
    };

    let chooser = gtk::FileDialog::new();
    chooser.set_title("Save as PDF");
    chooser.set_initial_name(Some(&format!("{suggested_name}.pdf")));

    let webview = webview.clone();
    let toasts = toasts.clone();
    chooser.save(Some(parent), gtk::gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else {
            // Cancelled: the ordinary way to change one's mind.
            return;
        };
        let uri = file.uri().to_string();
        let name = file
            .basename()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "PDF".to_string());

        let settings = gtk::PrintSettings::new();
        settings.set_printer(&printer);
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&uri));
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));

        let print = webkit6::PrintOperation::new(&webview);
        print.set_print_settings(&settings);
        let failed_toasts = toasts.clone();
        print.connect_failed(move |_, error| {
            tracing::warn!("saving the PDF failed: {error}");
            failed_toasts.add_toast(adw::Toast::new("Could not save the PDF"));
        });
        let keep = std::cell::RefCell::new(Some(print.clone()));
        let done_toasts = toasts.clone();
        print.connect_finished(move |_| {
            keep.borrow_mut().take();
            tracing::info!(%uri, "saved a PDF");
            done_toasts.add_toast(adw::Toast::new(&format!("Saved {name}")));
        });
        print.print();
    });
}

/// Show `html` as a print preview, with a Print button that prints it.
pub fn open(parent: &adw::ApplicationWindow, html: &str, job_name: &str) {
    let win = adw::Window::builder()
        .transient_for(parent)
        .modal(false)
        .title("Print Preview")
        .default_width(820)
        .default_height(900)
        .build();

    let webview = crate::ui::message_view::new_preview_webview();
    webview.set_vexpand(true);
    webview.load_html(html, Some("https://vireo.localhost/print-preview"));

    // Toasts confirm a save without stealing focus from the preview.
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&webview));

    let header = adw::HeaderBar::new();
    let print_btn = gtk::Button::builder()
        .label("Print…")
        .css_classes(["suggested-action"])
        .build();
    {
        let webview = webview.clone();
        let job = job_name.to_string();
        let win = win.clone();
        print_btn.connect_clicked(move |_| {
            // Print the preview itself, so what comes out is what is on screen.
            print_webview(&webview, &job, Some(win.clone().upcast()));
        });
    }
    header.pack_end(&print_btn);

    let save_btn = gtk::Button::builder().label("Save as PDF…").build();
    {
        let webview = webview.clone();
        let job = job_name.to_string();
        let win = win.clone();
        let toasts = toasts.clone();
        save_btn.connect_clicked(move |_| {
            save_as_pdf(&webview, &job, &win, &toasts);
        });
    }
    header.pack_start(&save_btn);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));
    win.set_content(Some(&toolbar));

    // Escape closes it, as it does the shortcuts window.
    let keys = gtk::EventControllerKey::new();
    let closer = win.clone();
    keys.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape {
            closer.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    win.add_controller(keys);

    win.present();
}

/// Styling that turns the reader's document into something page-shaped: the
/// print-only header revealed, light colours, and a sheet with margins on a grey
/// desk. Applied on screen only — printing uses the document's own `@media
/// print` rules, which say the same thing.
pub const PREVIEW_STYLES: &str = "\
    html{background:#e0e0e0;}\
    body{background:#e0e0e0 !important;color:#000 !important;padding:24px 0;}\
    .vireo-print-sheet{background:#fff;color:#000;width:21cm;max-width:calc(100% - 32px);\
      min-height:29.7cm;margin:0 auto;padding:1.6cm 1.5cm;box-sizing:border-box;\
      box-shadow:0 2px 12px rgba(0,0,0,0.25);}\
    .vireo-print-hdr{display:block !important;padding:0 0 10pt;margin:0 0 12pt;\
      border-bottom:1pt solid #999;font:10pt/1.45 system-ui,sans-serif;color:#000;}\
    .vireo-print-subject{font-size:14pt;font-weight:700;margin:0 0 8pt;}\
    .vireo-print-row{margin:0 0 2pt;}\
    .vireo-print-label{font-weight:700;}\
    .vireo-msg-hdr{background:none !important;padding:8pt 0 4pt;}\
    .vireo-msg{border-bottom:1pt solid #999;}\
    @media print{\
      html,body{background:#fff !important;padding:0;}\
      .vireo-print-sheet{width:auto;min-height:0;margin:0;padding:0;box-shadow:none;}\
    }";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_shows_what_printing_hides() {
        // The header exists in the document but is hidden on screen; the preview
        // is the one place it must be visible without printing.
        assert!(PREVIEW_STYLES.contains(".vireo-print-hdr{display:block !important"));
        // Light, on a page-shaped sheet.
        assert!(PREVIEW_STYLES.contains("color:#000 !important"));
        assert!(PREVIEW_STYLES.contains("width:21cm"));
        // Printing the preview must not print the grey desk or the shadow.
        assert!(PREVIEW_STYLES.contains("@media print"));
        assert!(PREVIEW_STYLES.contains("box-shadow:none"));
    }
}
