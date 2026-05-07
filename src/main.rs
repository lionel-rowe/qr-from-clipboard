use clipboard_rs::{
    common::RustImage, Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext,
};
use regex::Regex;
use rxing::{helpers::detect_in_image, BarcodeFormat};
use tracing::{debug, info, warn};
use win32_notif::{
    notification::actions::{
        action::{ActivationType, HintButtonStyle},
        ActionButton,
    },
    notification::visual::{text::HintStyle, Text},
    notification::Scenario,
    notifier::ToastsNotifier,
    NotificationActivatedEventHandler, NotificationBuilder,
};

struct ClipboardToastHandler {
    clipboard: ClipboardContext,
    notifier: ToastsNotifier,
    sequence: u32,
    url_regex: Regex,
    last_result: Option<String>,
}

impl ClipboardToastHandler {
    fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            clipboard: ClipboardContext::new()?,
            notifier: ToastsNotifier::new(Some("Microsoft.Windows.Explorer"))?,
            sequence: 0,
            url_regex: Regex::new(r"(?i)^https?://")?,
            last_result: None,
        })
    }
}

impl ClipboardHandler for ClipboardToastHandler {
    fn on_clipboard_change(&mut self) {
        debug!("clipboard changed");

        let image = match self.clipboard.get_image() {
            Ok(x) => x,
            Err(_) => return,
        };

        let dynamic_image = match image.get_dynamic_image() {
            Ok(x) => x,
            Err(e) => {
                debug!("failed to decode clipboard image: {e}");
                return;
            }
        };

        let qr_text = match detect_in_image(dynamic_image, Some(BarcodeFormat::QR_CODE)) {
            Ok(result) => {
                let text = result.getText().to_owned();
                if text.is_empty() {
                    return;
                }
                text
            }
            Err(_) => {
                debug!("no QR code found in clipboard image");
                return;
            }
        };

        if let Some(last) = &self.last_result {
            if qr_text == *last {
                debug!("duplicate QR result, skipping");
                return;
            }
        }
        self.last_result = Some(qr_text.clone());

        info!(qr_text, "QR code detected");

        self.sequence = self.sequence.wrapping_add(1);

        let is_url = self.url_regex.is_match(&qr_text);

        let mut builder = NotificationBuilder::new()
            .with_use_button_style(true)
            .with_scenario(Scenario::Urgent)
            .visual(
                Text::create(0, "QR code detected")
                    .with_style(HintStyle::Title)
                    .with_wrap(true),
            )
            .visual(
                Text::create(1, &qr_text)
                    .with_style(HintStyle::Body)
                    .with_wrap(true),
            );

        if is_url {
            let text_to_copy = qr_text.clone();

            builder = builder
                .action(
                    ActionButton::create("🔗 Open link")
                        .with_id(&qr_text)
                        .with_activation_type(ActivationType::Protocol)
                        .with_button_style(HintButtonStyle::Success),
                )
                .action(ActionButton::create("Copy link").with_id("copy_link"))
                .on_activated(NotificationActivatedEventHandler::new(move |_, args| {
                    if matches!(
                        args.as_ref().and_then(|x| x.button_id.as_deref()),
                        Some("copy_link")
                    ) {
                        if let Ok(clipboard) = ClipboardContext::new() {
                            let _ = clipboard.set_text(text_to_copy.clone());
                        }
                    }

                    Ok(())
                }));
        } else {
            let text_to_copy = qr_text.clone();

            builder = builder
                .action(
                    ActionButton::create("📋 Copy text")
                        .with_id("copy_text")
                        .with_button_style(HintButtonStyle::Success),
                )
                .on_activated(NotificationActivatedEventHandler::new(move |_, args| {
                    if matches!(
                        args.as_ref().and_then(|x| x.button_id.as_deref()),
                        Some("copy_text")
                    ) {
                        if let Ok(clipboard) = ClipboardContext::new() {
                            let _ = clipboard.set_text(text_to_copy.clone());
                        }
                    }

                    Ok(())
                }));
        }

        let notification = builder
            .build(
                self.sequence,
                &self.notifier,
                &format!("clipboard-{}", self.sequence),
                "watch",
            )
            .and_then(|notification| notification.show());

        if let Err(err) = notification {
            warn!("failed to show notification: {err}");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("starting clipboard watcher");
    let handler = ClipboardToastHandler::new()?;
    let mut watcher = ClipboardWatcherContext::new()?;

    watcher.add_handler(handler);

    // start_watch blocks and keeps listening until the process exits.
    watcher.start_watch();

    Ok(())
}
