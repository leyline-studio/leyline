//! The develop view at the loupe's resolution, when a GPU renders it
//! (ADR 0166).
//!
//! `Small` stays the view's truth: the committed render, the disk cache,
//! the histogram. What this module adds is an *upgrade* of the image on
//! screen — rendered off the UI thread after each refresh, and shown only
//! if nothing has replaced the view since — and the class a live frame
//! asks for when `Library::live_on_gpu` says the GPU will take it.

use std::sync::mpsc::{Receiver, Sender};

use leyline_sdk::{AssetId, PreviewKind, Rgb8, Settings};
use slint::{ComponentHandle, Global};

use crate::app::App;
use crate::develop;
use crate::ui::{DevelopState, StudioWindow};

/// How much of the window's width the loupe occupies, roughly: the two
/// side columns take the rest. Only the class it falls in matters
/// (ADR 0166 §5), so an estimate is enough.
const LOUPE_SHARE: f32 = 0.72;

/// A loupe-sized render on its way back, tagged with the generation of the
/// view it was asked for.
pub(crate) struct LoupeFrame {
    generation: u64,
    image: Rgb8,
    settings: Settings,
}

/// The two ends of the channel the background renders answer on, and the
/// generation of the view on screen.
pub(crate) struct Loupe {
    generation: u64,
    sender: Sender<LoupeFrame>,
    receiver: Receiver<LoupeFrame>,
    /// The settings of the loupe-sized frame on screen, if one is: a
    /// refresh to exactly these settings keeps it rather than stepping down
    /// to `Small` and back up — a visible blink at every slider release.
    shown: Option<Settings>,
}

impl Loupe {
    pub(crate) fn new() -> Loupe {
        let (sender, receiver) = std::sync::mpsc::channel();
        Loupe {
            generation: 0,
            sender,
            receiver,
            shown: None,
        }
    }

    /// Something else is on screen now — a live frame, a look, another
    /// photo: an upgrade asked for before it must not land on top of it.
    pub(crate) fn invalidate(&mut self) {
        self.generation += 1;
        self.shown = None;
    }

    /// A loupe-sized frame of `settings` is now on screen.
    pub(crate) fn showing(&mut self, settings: Settings) {
        self.shown = Some(settings);
    }

    /// Whether the frame on screen is a loupe-sized render of exactly
    /// `settings`.
    pub(crate) fn shows(&self, settings: &Settings) -> bool {
        self.shown.as_ref() == Some(settings)
    }
}

/// The preview class that fills the loupe (ADR 0166 §5): `Small` up to
/// 1024 px, `Medium` up to 2048, `Large` above.
pub(crate) fn loupe_class(window: &StudioWindow) -> PreviewKind {
    let width = window.window().size().width as f32 * LOUPE_SHARE;
    if width <= 1024.0 {
        PreviewKind::Small
    } else if width <= 2048.0 {
        PreviewKind::Medium
    } else {
        PreviewKind::Large
    }
}

/// The class a live frame of `settings` asks for (ADR 0166 §3): the
/// loupe's when the GPU will render it now, `Small` otherwise — so a drag
/// the GPU cannot take costs what it always cost.
pub(crate) fn live_class(
    app: &App,
    window: &StudioWindow,
    asset: AssetId,
    settings: &Settings,
) -> PreviewKind {
    let class = loupe_class(window);
    if class != PreviewKind::Small && app.library.live_on_gpu(asset, class, settings) {
        class
    } else {
        PreviewKind::Small
    }
}

/// Starts the loupe-sized render of the head the view just showed at
/// `Small` (ADR 0166 §2), when there is a GPU and a loupe bigger than
/// `Small`. Never blocks: the first one of a photo pays a full decode, on
/// its own thread.
pub(crate) fn request_upgrade(
    app: &mut App,
    window: &StudioWindow,
    asset: AssetId,
    settings: Settings,
) {
    app.loupe.invalidate();
    let class = loupe_class(window);
    if class == PreviewKind::Small || app.library.preview_backend().is_none() {
        return;
    }
    let generation = app.loupe.generation;
    let library = app.library.clone();
    let sender = app.loupe.sender.clone();
    std::thread::spawn(move || {
        if let Ok(image) = library.preview_live(asset, class, &settings) {
            // A closed receiver means Studio is closing: nothing to show.
            let _ = sender.send(LoupeFrame {
                generation,
                image,
                settings,
            });
        }
    });
}

/// Shows the upgrades that arrived, if the view they were asked for is
/// still the one on screen; drops the others. Called by the event pump.
pub(crate) fn deliver(app: &mut App, window: &StudioWindow) {
    while let Ok(LoupeFrame {
        generation,
        image,
        settings,
    }) = app.loupe.receiver.try_recv()
    {
        if generation != app.loupe.generation || app.develop.is_none() {
            continue;
        }
        let mut painted = image.clone();
        develop::paint_clipping(painted.data_mut(), app.clip_highlights, app.clip_shadows);
        DevelopState::get(window).set_develop_image(crate::models::rgb8_to_slint_image(&painted));
        // The unpainted one, for the readout (ADR 0112 §2).
        app.dev_pixels = Some(image);
        app.loupe.showing(settings);
    }
}
