//! Tests for the Keychain resolver. The tests that touch the Keychain run
//! only on macOS, where they create items under the test-only service
//! prefix `starmap-test` and delete them when they finish.

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
    format!("t{}-{label}", std::process::id()).parse().unwrap()
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

    use super::{Keychain, app, deadline, name, workspace};
    use crate::{AppName, Lookup, Resolver, SecretError, WorkspaceName};
    use starmap::{Name, SecretBytes};

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

    fn test_keychain() -> Keychain {
        Keychain::with_prefix("starmap-test")
    }

    /// Returns the generated value of `name` as text.
    #[expect(
        clippy::disallowed_methods,
        reason = "the tests check the text that a generated secret holds"
    )]
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
        Ok(keychain.resolve(lookup, deadline())?.expose().clone())
    }

    #[test]
    fn a_generated_value_has_the_declared_bytes_and_is_returned_unchanged_afterwards() {
        let keychain = test_keychain();
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
        let keychain = test_keychain();
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
        let keychain = test_keychain();
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
}
