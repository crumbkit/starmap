//! Tests for the Keychain resolver. The tests that touch the Keychain run
//! only on macOS. Each of their workspaces is named
//! `starmap-test-<pid>-<label>`, so their items sit under the test-only
//! service prefix `starmap.starmap-test-`, and they delete the items when
//! they finish.

use std::time::{Duration, Instant};

use starmap::{Name, SecretRef};

use super::Keychain;
use crate::{AppName, Lookup, Resolver, SecretError, WorkspaceName};

/// Returns a deadline far enough away that no test reaches it.
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Returns a workspace name that no other test process uses, so tests that
/// run at the same time never share an item.
fn workspace(label: &str) -> WorkspaceName {
    format!("starmap-test-{}-{label}", std::process::id())
        .parse()
        .unwrap()
}

fn app(text: &str) -> AppName {
    text.parse().unwrap()
}

#[test]
fn a_lookup_by_reference_is_left_to_another_resolver() {
    let reference = SecretRef::OnePassword("op://Personal/shelf mirror/token".parse().unwrap());
    let result = Keychain::new().resolve(Lookup::Reference(&reference), deadline());
    assert!(matches!(result, Err(SecretError::Unsupported)));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn every_call_is_unsupported_on_a_platform_without_a_keychain() {
    let keychain = Keychain::new();
    let lookup = Lookup::Generated {
        workspace: &workspace("unsupported"),
        app: &app("shelf"),
        name: &name("SHELF_SESSION_KEY"),
        bytes: starmap::SecretBytes::DEFAULT,
    };
    assert!(matches!(
        keychain.resolve(lookup, deadline()),
        Err(SecretError::Unsupported)
    ));
    assert!(matches!(
        keychain.forget(&workspace("unsupported")),
        Err(SecretError::Unsupported)
    ));
}

#[cfg(target_os = "macos")]
mod macos {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    use security_framework::base::Error;
    use starmap::{Name, Secret, SecretBytes};
    use zeroize::Zeroizing;

    use super::super::macos::{problem, store};
    use super::super::service;
    use super::{Keychain, app, deadline, name, workspace};
    use crate::{AppName, Lookup, Resolver, SecretError, WorkspaceName};

    /// Forgets the test workspaces when a test ends, whether it passed or
    /// failed, so no test leaves an item in the Keychain.
    struct Cleanup<'a>(&'a Keychain, Vec<WorkspaceName>);

    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            for workspace in &self.1 {
                let _ = self.0.forget(workspace);
            }
        }
    }

    fn bytes(count: u16) -> SecretBytes {
        SecretBytes::new(count).unwrap()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the tests check the text that a stored secret holds"
    )]
    fn text(secret: &Secret<String>) -> String {
        secret.expose().clone()
    }

    /// Returns the generated value of `name` as text.
    fn generated(
        keychain: &Keychain,
        workspace: &WorkspaceName,
        app: &AppName,
        name: &Name,
        bytes: SecretBytes,
    ) -> Result<String, SecretError> {
        let lookup = Lookup::Generated {
            workspace,
            app,
            name,
            bytes,
        };
        Ok(text(&keychain.resolve(lookup, deadline())?))
    }

    #[test]
    fn a_generated_value_has_the_declared_bytes_and_is_returned_unchanged_afterwards() {
        let keychain = Keychain::new();
        let shelf = workspace("generate");
        let _cleanup = Cleanup(&keychain, vec![shelf.clone()]);
        let (session, cookie) = (name("SHELF_SESSION_KEY"), name("SHELF_COOKIE_KEY"));

        let first = generated(&keychain, &shelf, &app("shelf"), &session, bytes(48)).unwrap();
        let again = generated(&keychain, &shelf, &app("shelf"), &session, bytes(48)).unwrap();
        let other = generated(&keychain, &shelf, &app("shelf"), &cookie, bytes(16)).unwrap();

        assert_eq!(URL_SAFE_NO_PAD.decode(&first).unwrap().len(), 48);
        assert_eq!(again, first);
        assert_eq!(URL_SAFE_NO_PAD.decode(&other).unwrap().len(), 16);
    }

    #[test]
    fn forget_deletes_every_item_of_one_workspace_and_no_other() {
        let keychain = Keychain::new();
        // The kept workspace's name starts with the forgotten one's name.
        let (gone, kept) = (workspace("forget"), workspace("forget-kept"));
        let _cleanup = Cleanup(&keychain, vec![gone.clone(), kept.clone()]);
        let key = name("SHELF_SESSION_KEY");
        let size = SecretBytes::DEFAULT;

        let before = generated(&keychain, &gone, &app("shelf"), &key, size).unwrap();
        generated(&keychain, &gone, &app("reader"), &key, size).unwrap();
        let survivor = generated(&keychain, &kept, &app("shelf"), &key, size).unwrap();

        assert_eq!(keychain.forget(&gone).unwrap(), 2);
        assert_eq!(keychain.forget(&gone).unwrap(), 0);
        let after = generated(&keychain, &gone, &app("shelf"), &key, size).unwrap();
        assert_ne!(after, before);
        let unchanged = generated(&keychain, &kept, &app("shelf"), &key, size).unwrap();
        assert_eq!(unchanged, survivor);
    }

    #[test]
    fn a_passed_deadline_returns_deadline_and_creates_nothing() {
        let keychain = Keychain::new();
        let late = workspace("deadline");
        let _cleanup = Cleanup(&keychain, vec![late.clone()]);
        let lookup = Lookup::Generated {
            workspace: &late,
            app: &app("shelf"),
            name: &name("SHELF_SESSION_KEY"),
            bytes: SecretBytes::DEFAULT,
        };

        let result = keychain.resolve(lookup, std::time::Instant::now());

        assert!(matches!(result, Err(SecretError::Deadline)));
        assert_eq!(keychain.forget(&late).unwrap(), 0);
    }

    #[test]
    fn a_second_store_of_the_same_item_returns_the_first_value_unchanged() {
        let keychain = Keychain::new();
        let raced = workspace("store");
        let _cleanup = Cleanup(&keychain, vec![raced.clone()]);
        let service = service(&raced);
        let value = |text: &str| Zeroizing::new(text.to_owned());

        let first = store(&service, "shelf/KEY", value("first"), deadline()).unwrap();
        let second = store(&service, "shelf/KEY", value("second"), deadline()).unwrap();

        assert_eq!(text(&first), "first");
        assert_eq!(text(&second), "first");
    }

    #[test]
    fn each_keychain_code_maps_to_the_problem_it_names() {
        let cases = [
            (-128, "denied"),
            (-25293, "denied"),
            (-25308, "locked"),
            (-25307, "unsupported"),
            (-50, "program"),
        ];
        for (code, expected) in cases {
            let kind = match problem(Error::from_code(code)) {
                SecretError::Denied => "denied",
                SecretError::Locked => "locked",
                SecretError::Unsupported => "unsupported",
                SecretError::Program(_) => "program",
                _ => "other",
            };
            assert_eq!(kind, expected, "code {code}");
        }
    }
}
