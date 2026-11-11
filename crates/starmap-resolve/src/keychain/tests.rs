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
    let result = Keychain::new().resolve(Lookup::reference(&reference), deadline());
    assert!(matches!(result, Err(SecretError::Unsupported)));
}

#[test]
fn a_save_of_any_lookup_but_a_saved_one_is_left_to_another_resolver() {
    let reference = SecretRef::OnePassword("op://Personal/shelf mirror/token".parse().unwrap());
    let value = starmap::Secret::new("a-test-value".to_owned());
    let result = Keychain::new().save(Lookup::reference(&reference), &value, deadline());
    assert!(matches!(result, Err(SecretError::Unsupported)));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn every_call_is_unsupported_on_a_platform_without_a_keychain() {
    let keychain = Keychain::new();
    let (unsupported, shelf) = (workspace("unsupported"), app("shelf"));
    let (key, token) = (name("SHELF_SESSION_KEY"), name("SHELF_UPSTREAM_TOKEN"));
    let lookup = Lookup::Generated {
        slot: crate::Slot::new(&unsupported, &shelf, &key),
        bytes: starmap::ByteCount::DEFAULT,
        encoding: starmap::Encoding::Base64Url,
    };
    assert!(matches!(
        keychain.resolve(lookup, deadline()),
        Err(SecretError::Unsupported)
    ));
    let saved = Lookup::Saved {
        slot: crate::Slot::new(&unsupported, &shelf, &token),
    };
    assert!(matches!(
        keychain.resolve(saved, deadline()),
        Err(SecretError::Unsupported)
    ));
    let value = starmap::Secret::new("a-test-value".to_owned());
    assert!(matches!(
        keychain.save(saved, &value, deadline()),
        Err(SecretError::Unsupported)
    ));
    assert!(matches!(
        keychain.forget(&workspace("unsupported")),
        Err(SecretError::Unsupported)
    ));
}

#[cfg(target_os = "macos")]
mod macos {
    use security_framework::base::Error;
    use starmap::{ByteCount, ByteLength, ByteRule, Encoding, Name, Secret, Type, TypeError};
    use zeroize::Zeroizing;

    use super::super::macos::{create, problem, store};
    use super::super::service;
    use super::{Keychain, app, deadline, name, workspace};
    use crate::{AppName, Lookup, Resolver, SecretError, Slot, WorkspaceName};

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

    fn bytes(count: u16) -> ByteCount {
        ByteCount::generated(count).unwrap()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the tests check the text that a stored secret holds"
    )]
    fn text(secret: &Secret) -> String {
        secret.expose().to_owned()
    }

    /// Returns the bytes type of exactly `count` bytes in `encoding`.
    fn exactly(encoding: Encoding, count: u16) -> Type {
        Type::Bytes(ByteRule::new(
            encoding,
            ByteLength::Exactly(ByteCount::new(count).unwrap()),
        ))
    }

    /// Returns the generated value of `name` as base64url text.
    fn generated(
        keychain: &Keychain,
        workspace: &WorkspaceName,
        app: &AppName,
        name: &Name,
        bytes: ByteCount,
    ) -> Result<String, SecretError> {
        let lookup = Lookup::Generated {
            slot: Slot::new(workspace, app, name),
            bytes,
            encoding: Encoding::Base64Url,
        };
        Ok(text(&keychain.resolve(lookup, deadline())?))
    }

    #[test]
    fn a_new_value_is_written_in_each_encoding_at_the_declared_length_and_passes_its_check() {
        for encoding in [Encoding::Hex, Encoding::Base64, Encoding::Base64Url] {
            for count in [16, 33, 1024] {
                let value = Secret::new(create(bytes(count), encoding).unwrap().to_string());

                assert_eq!(
                    exactly(encoding, count).check_secret(&value),
                    Ok(()),
                    "{count} bytes as {encoding}"
                );
            }
        }
    }

    #[test]
    fn a_stored_value_in_another_encoding_fails_its_check_without_its_text() {
        let stored = create(bytes(32), Encoding::Base64Url).unwrap().to_string();

        let error = exactly(Encoding::Hex, 32)
            .check_secret(&Secret::new(stored.clone()))
            .unwrap_err();

        assert_eq!(error, TypeError::NotEncoded(Encoding::Hex));
        assert!(!error.to_string().contains(&stored), "{error}");
    }

    #[test]
    #[ignore = "it writes to the login Keychain, so only the macOS lane runs it, with --run-ignored all"]
    fn a_generated_value_has_the_declared_bytes_and_is_returned_unchanged_afterwards() {
        let keychain = Keychain::new();
        let shelf = workspace("generate");
        let _cleanup = Cleanup(&keychain, vec![shelf.clone()]);
        let (session, cookie) = (name("SHELF_SESSION_KEY"), name("SHELF_COOKIE_KEY"));

        let first = generated(&keychain, &shelf, &app("shelf"), &session, bytes(48)).unwrap();
        let again = generated(&keychain, &shelf, &app("shelf"), &session, bytes(48)).unwrap();
        let other = generated(&keychain, &shelf, &app("shelf"), &cookie, bytes(16)).unwrap();

        let check = |count, value: &str| {
            exactly(Encoding::Base64Url, count).check_secret(&Secret::new(value.to_owned()))
        };
        assert_eq!(check(48, &first), Ok(()));
        assert_eq!(again, first);
        assert_eq!(check(16, &other), Ok(()));
    }

    #[test]
    #[ignore = "it writes to the login Keychain, so only the macOS lane runs it, with --run-ignored all"]
    fn forget_deletes_every_item_of_one_workspace_and_no_other() {
        let keychain = Keychain::new();
        // The kept workspace's name starts with the forgotten one's name.
        let (gone, kept) = (workspace("forget"), workspace("forget-kept"));
        let _cleanup = Cleanup(&keychain, vec![gone.clone(), kept.clone()]);
        let key = name("SHELF_SESSION_KEY");
        let size = ByteCount::DEFAULT;

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
    #[ignore = "it writes to the login Keychain, so only the macOS lane runs it, with --run-ignored all"]
    fn a_saved_secret_is_found_by_a_later_lookup_and_forgotten_with_its_workspace() {
        let keychain = Keychain::new();
        let saving = workspace("save");
        let _cleanup = Cleanup(&keychain, vec![saving.clone()]);
        let token = name("SHELF_UPSTREAM_TOKEN");
        let shelf = app("shelf");
        let lookup = Lookup::Saved {
            slot: Slot::new(&saving, &shelf, &token),
        };

        assert!(matches!(
            keychain.resolve(lookup, deadline()),
            Err(SecretError::NotFound)
        ));
        let typed = Secret::new("a-typed-test-value".to_owned());
        keychain.save(lookup, &typed, deadline()).unwrap();
        assert_eq!(
            text(&keychain.resolve(lookup, deadline()).unwrap()),
            "a-typed-test-value"
        );
        assert_eq!(keychain.forget(&saving).unwrap(), 1);
        assert!(matches!(
            keychain.resolve(lookup, deadline()),
            Err(SecretError::NotFound)
        ));
    }

    #[test]
    #[ignore = "it writes to the login Keychain, so only the macOS lane runs it, with --run-ignored all"]
    fn a_passed_deadline_returns_deadline_and_creates_nothing() {
        let keychain = Keychain::new();
        let late = workspace("deadline");
        let _cleanup = Cleanup(&keychain, vec![late.clone()]);
        let (shelf, key) = (app("shelf"), name("SHELF_SESSION_KEY"));
        let lookup = Lookup::Generated {
            slot: Slot::new(&late, &shelf, &key),
            bytes: ByteCount::DEFAULT,
            encoding: Encoding::Base64Url,
        };

        let result = keychain.resolve(lookup, std::time::Instant::now());

        assert!(matches!(result, Err(SecretError::Deadline)));
        assert_eq!(keychain.forget(&late).unwrap(), 0);
    }

    #[test]
    #[ignore = "it writes to the login Keychain, so only the macOS lane runs it, with --run-ignored all"]
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
