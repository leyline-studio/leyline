//! Commands that follow new files arriving: tethered capture and folder watching.

use super::*;

/// Connects to a USB camera and imports every shot as it's taken
/// (`docs/adr/0038-tethered-capture.md`,
/// `docs/adr/0087-tethered-capture-bar.md`), until the camera disconnects
/// or the process is interrupted (Ctrl+C).
///
/// The bar's controls, minus the ones a terminal cannot draw: the session
/// folder, the develop preset, the exposure settings, and the shutter —
/// which on a command line becomes the one thing a bar cannot offer, an
/// intervalometer (`--capture-every`).
pub(crate) fn tether(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["session", "preset", "capture-every", "set"])?;
    let [root] = positional.as_slice() else {
        return Err(
            "usage: leyline tether <library> [--session <name>] [--preset <name>] \
             [--set <setting>=<value>] [--capture-every <seconds>]"
                .to_owned(),
        );
    };
    let library = open(root)?;
    let preset = match options.value("preset") {
        Some(name) => Some(find_preset(&library, name)?),
        None => None,
    };
    let interval = match options.value("capture-every") {
        Some(seconds) => {
            Some(Duration::from_secs_f64(seconds.parse::<f64>().map_err(
                |_| format!("--capture-every expects seconds, got {seconds:?}"),
            )?))
        }
        None => None,
    };
    // Parsed before connecting: a typo in `--set` should not cost the user
    // a camera session to find out about.
    let mut wanted: Vec<(TetherSetting, String)> = Vec::new();
    for pair in options.all("set") {
        let (name, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("--set expects <setting>=<value>, got {pair:?}"))?;
        let setting = TetherSetting::parse(name).ok_or_else(|| {
            format!("unknown setting {name:?}; expected one of shutter, aperture, iso, wb")
        })?;
        wanted.push((setting, value.to_owned()));
    }

    let events = library.subscribe();
    library
        .tether_connect(&TetherOptions {
            session: options.value("session").unwrap_or_default().to_owned(),
            preset,
        })
        .map_err(|e| e.to_string())?;
    for (setting, value) in &wanted {
        library.tether_set(*setting, value);
    }
    eprintln!("connected — waiting for shots (Ctrl+C to stop)");

    // The intervalometer lives on its own thread rather than in the event
    // loop below: that loop blocks in `recv()` until the camera has
    // something to say, which on a quiet set is exactly when the next
    // frame is due.
    if let Some(interval) = interval {
        let library = library.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(interval);
                library.tether_capture();
            }
        });
    }

    loop {
        match events.recv() {
            Ok(leyline_sdk::Event::AssetsAdded { asset_ids }) => {
                for asset in asset_ids {
                    println!("captured asset {asset}");
                }
            }
            Ok(leyline_sdk::Event::TetherSettingsChanged) => {
                print_camera_settings(&library.tether_settings());
            }
            Ok(leyline_sdk::Event::TetherCommandFailed { message }) => {
                eprintln!("camera refused: {message}");
            }
            Ok(leyline_sdk::Event::TetherDisconnected { reason }) => {
                match reason {
                    Some(reason) => eprintln!("disconnected: {reason}"),
                    None => eprintln!("disconnected"),
                }
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
}

/// One line per exposure setting the body exposes, plus the body itself —
/// what the capture bar shows, in the shape a terminal can show it.
pub(crate) fn print_camera_settings(settings: &CameraSettings) {
    if settings.model.is_empty() {
        return;
    }
    let mut line = settings.model.clone();
    for setting in TetherSetting::ALL {
        if let Some(value) = settings.get(setting) {
            line.push_str(&format!("  {}={}", setting.as_str(), value.value));
        }
    }
    eprintln!("{line}");
}

/// Watches a folder and imports every file that settles there
/// (`docs/adr/0039-watched-folder-import.md`), until the process is
/// interrupted (Ctrl+C).
pub(crate) fn watch(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, folder] = positional.as_slice() else {
        return Err("usage: leyline watch <library> <folder>".to_owned());
    };
    let library = open(root)?;
    let events = library.subscribe();
    library
        .watch_start(Path::new(folder))
        .map_err(|e| e.to_string())?;
    eprintln!("watching {folder} — waiting for files (Ctrl+C to stop)");
    loop {
        match events.recv() {
            Ok(leyline_sdk::Event::AssetsAdded { asset_ids }) => {
                for asset in asset_ids {
                    println!("imported asset {asset}");
                }
            }
            Ok(leyline_sdk::Event::WatchStopped { reason }) => {
                match reason {
                    Some(reason) => eprintln!("stopped: {reason}"),
                    None => eprintln!("stopped"),
                }
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
}
