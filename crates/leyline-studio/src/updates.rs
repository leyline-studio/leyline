//! Asking whether a newer version exists (ADR 0077).
//!
//! Everything here is off the UI thread and behind a consent stored in
//! `preferences.json` ([`crate::preferences`]). The rules this module has to
//! keep are short, and each is visible in the code below: the endpoint is
//! compiled in, nothing about the user or the library is transmitted, a
//! network failure is not an application error, and finding a version never
//! installs it.

use std::time::Duration;

use cargo_packager_updater::{Config, semver::Version, url::Url};

/// The manifest endpoint, compiled in and not configurable (ADR 0077 §1).
///
/// GitHub keeps `releases/latest/download/<asset>` pointing at the newest
/// release, so the text never changes from one version to the next — which
/// is what allows it to be a constant. Making it a setting would turn an
/// update mechanism into a redirection surface; changing it is a release,
/// not a preference.
const MANIFEST_URL: &str =
    "https://github.com/leyline-studio/leyline/releases/latest/download/latest.json";

/// The minisign public key the manifest's signature is verified against.
///
/// Its private half signs by hand at publication time and lives neither in
/// this repository nor in CI (ADR 0077 §1). Without this verification an
/// update is the download and execution of an arbitrary binary, so a build
/// with an empty key refuses to check at all — see [`check`].
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDIxOTdCQUUwNzRDNjA0MTQKUldRVUJNWjA0THFYSVU4WnVHdHRXelV3bGc1VTVZakdtRTJXb0Vsa1BldTczbmVsRkFEN2pCWXcK";

/// Gives up rather than hanging on a captive portal or a black hole. Being
/// offline is Leyline's normal state; a check that never returns would be
/// worse than one that says nothing.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// What a check found: the version and the release notes, and nothing else.
///
/// Deliberately plain data. The `Update` handle that could install it is
/// *not* kept: installing re-checks (see [`install_latest`]), which costs
/// one request and means no live network object has to cross a thread
/// boundary or outlive the dialog that displayed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AvailableUpdate {
    /// The version found on the far side, as the manifest spells it.
    pub(crate) version: String,
    /// The release notes, empty when the manifest carries none.
    pub(crate) notes: String,
}

/// The outcome of one check, in the three shapes the UI has to tell apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckOutcome {
    /// A newer version exists.
    Available(AvailableUpdate),
    /// This build is the newest one published.
    UpToDate,
    /// The manifest could not be fetched or verified. Not an application
    /// error (ADR 0077 §3): the automatic check swallows it, and only a
    /// check someone asked for says so out loud.
    Unreachable(String),
}

/// The updater configuration, identical for every call.
fn config() -> Result<Config, String> {
    let endpoint = Url::parse(MANIFEST_URL).map_err(|e| e.to_string())?;
    Ok(Config {
        endpoints: vec![endpoint],
        pubkey: PUBLIC_KEY.to_owned(),
        windows: None,
    })
}

/// Asks the manifest whether something newer than `current_version` exists.
///
/// Blocking, and to be called from a worker thread only — the underlying
/// client is synchronous, and the whole point of the cadence in ADR 0077 §2
/// is that nothing about this is ever noticeable.
///
/// The request carries the installed version and the platform in its URL
/// and nothing else: no identifier, no counter, no library data. Both are
/// used locally, to pick a line out of the manifest.
pub(crate) fn check(current_version: &str) -> CheckOutcome {
    if PUBLIC_KEY.is_empty() {
        return CheckOutcome::Unreachable("this build carries no update signing key".to_owned());
    }
    let (version, config) = match (Version::parse(current_version), config()) {
        (Ok(version), Ok(config)) => (version, config),
        (Err(error), _) => return CheckOutcome::Unreachable(error.to_string()),
        (_, Err(error)) => return CheckOutcome::Unreachable(error),
    };
    let updater = match cargo_packager_updater::UpdaterBuilder::new(version, config)
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(updater) => updater,
        Err(error) => return CheckOutcome::Unreachable(error.to_string()),
    };
    match updater.check() {
        Ok(Some(update)) => CheckOutcome::Available(AvailableUpdate {
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
        }),
        Ok(None) => CheckOutcome::UpToDate,
        Err(error) => CheckOutcome::Unreachable(error.to_string()),
    }
}

/// Downloads and installs the newest version, after checking again.
///
/// Blocking, worker thread only. The second check is what lets [`check`]
/// return plain strings: it re-fetches the signed manifest and hands the
/// verified handle straight to the installer, so nothing between the two
/// clicks can substitute what gets installed.
///
/// On Windows the installer takes over and the process exits inside this
/// call — it never returns. Elsewhere the package is replaced in place and
/// the new version starts being used at the next launch.
pub(crate) fn install_latest(current_version: &str) -> Result<(), String> {
    let version = Version::parse(current_version).map_err(|e| e.to_string())?;
    let updater = cargo_packager_updater::UpdaterBuilder::new(version, config()?)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    match updater.check() {
        Ok(Some(update)) => update.download_and_install().map_err(|e| e.to_string()),
        Ok(None) => Err("no newer version is published".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Runs `work` on a worker thread and hands its result to `deliver` on the
/// UI thread.
///
/// The whole reason this module needs threads: the update client is
/// synchronous, and a network call on Slint's event loop freezes the window
/// for as long as it takes — which on a bad connection is the timeout.
pub(crate) fn in_background<T, W, D>(work: W, deliver: D)
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(T) + Send + 'static,
{
    std::thread::spawn(move || {
        let outcome = work();
        // A failed post means the event loop is gone — the window was
        // closed while the request was in flight. There is nothing left
        // to report to, and nothing to do about it.
        let _ = slint::invoke_from_event_loop(move || deliver(outcome));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_the_stable_github_redirection() {
        // ADR 0077 §1 names this URL exactly. A typo here would be a
        // silent "no update ever exists", which is the failure mode
        // nobody reports.
        assert_eq!(
            MANIFEST_URL,
            "https://github.com/leyline-studio/leyline/releases/latest/download/latest.json"
        );
        assert!(config().is_ok());
    }

    #[test]
    fn a_signing_key_is_compiled_in() {
        // An unsigned manifest is an arbitrary executable download. This
        // fails the build rather than shipping a binary that would accept
        // one.
        assert!(!PUBLIC_KEY.is_empty());
        assert!(PUBLIC_KEY.len() > 64);
    }

    #[test]
    fn a_bad_current_version_is_unreachable_not_a_panic() {
        // `check` is called from a thread with no one to catch a panic;
        // every path out of it is a value.
        assert!(matches!(
            check("not-a-version"),
            CheckOutcome::Unreachable(_)
        ));
    }
}
