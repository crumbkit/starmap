//! The Keychain calls on macOS, through security-framework.
//!
//! RUST-10: each call checks the deadline before it starts, and a call
//! that has started runs to its end. A Keychain prompt that waits for the
//! person can therefore hold a call past the deadline. Running the call on
//! a thread that the caller abandons at the deadline would replace this,
//! if a host needs it.

use std::io;
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use core_foundation::data::CFData;
use security_framework::base::Error;
use security_framework::item::{ItemAddOptions, ItemAddValue, ItemClass, ItemSearchOptions, Limit};
use security_framework::passwords::{delete_generic_password, generic_password};
use security_framework::passwords_options::PasswordOptions;
use security_framework::random::SecRandom;
use starmap::{Secret, SecretBytes};
use zeroize::Zeroizing;

use crate::SecretError;

/// The Keychain found no item.
const ITEM_NOT_FOUND: i32 = -25300;
/// The Keychain already holds an item with the same service and account.
const DUPLICATE_ITEM: i32 = -25299;
/// The person cancelled the Keychain's prompt.
const USER_CANCELED: i32 = -128;
/// The person gave the wrong password at the Keychain's prompt.
const AUTH_FAILED: i32 = -25293;
/// The keychain is locked, and the process may not prompt to unlock it.
const INTERACTION_NOT_ALLOWED: i32 = -25308;

/// Returns the generated value at `service` and `account`, and creates it
/// from `bytes` random bytes when the Keychain holds none.
pub(super) fn generated(
    service: &str,
    account: &str,
    bytes: SecretBytes,
    deadline: Instant,
) -> Result<Secret<String>, SecretError> {
    if let Some(value) = read(service, account, deadline)? {
        return Ok(value);
    }
    let mut value = create(bytes)?;
    in_time(deadline)?;
    let mut item = ItemAddOptions::new(ItemAddValue::Data {
        class: ItemClass::generic_password(),
        data: CFData::from_buffer(value.as_bytes()),
    });
    item.set_service(service).set_account_name(account);
    match item.add() {
        Ok(()) => Ok(Secret::new(std::mem::take(&mut *value))),
        // Another process created the value first, and every caller
        // returns the value that the Keychain holds.
        Err(error) if error.code() == DUPLICATE_ITEM => read(service, account, deadline)?
            .ok_or_else(|| SecretError::Program(io::Error::other(error))),
        Err(error) => Err(problem(error)),
    }
}

/// Deletes every item of `service`, and returns how many it deleted. The
/// login keychain deletes one item for each delete call, so the call
/// lists the items and deletes each by its account.
pub(super) fn forget(service: &str) -> Result<usize, SecretError> {
    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service(service)
        .load_attributes(true)
        .limit(Limit::All);
    let items = match query.search() {
        Ok(items) => items,
        Err(error) if error.code() == ITEM_NOT_FOUND => return Ok(0),
        Err(error) => return Err(problem(error)),
    };
    let mut deleted = 0;
    for item in items {
        let Some(account) = item
            .simplify_dict()
            .and_then(|mut item| item.remove("acct"))
        else {
            continue;
        };
        match delete_generic_password(service, &account) {
            Ok(()) => deleted += 1,
            Err(error) if error.code() == ITEM_NOT_FOUND => {}
            Err(error) => return Err(problem(error)),
        }
    }
    Ok(deleted)
}

/// Returns the value at `service` and `account`, or `None` when the
/// Keychain holds none.
fn read(
    service: &str,
    account: &str,
    deadline: Instant,
) -> Result<Option<Secret<String>>, SecretError> {
    in_time(deadline)?;
    match generic_password(PasswordOptions::new_generic_password(service, account)) {
        Ok(data) => match String::from_utf8(data) {
            Ok(text) => Ok(Some(Secret::new(text))),
            Err(error) => {
                drop(Zeroizing::new(error.into_bytes()));
                Err(SecretError::Program(io::Error::other(
                    "the Keychain item does not hold UTF-8 text",
                )))
            }
        },
        Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(problem(error)),
    }
}

/// Returns `bytes` cryptographically secure random bytes, written as
/// base64url without padding.
fn create(bytes: SecretBytes) -> Result<Zeroizing<String>, SecretError> {
    let mut random = Zeroizing::new(vec![0; usize::from(bytes.get())]);
    SecRandom::default()
        .copy_bytes(&mut random)
        .map_err(SecretError::Program)?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(&*random)))
}

/// Returns [`SecretError::Deadline`] once the deadline has passed.
fn in_time(deadline: Instant) -> Result<(), SecretError> {
    if Instant::now() < deadline {
        Ok(())
    } else {
        Err(SecretError::Deadline)
    }
}

/// Maps a Keychain failure to the problem it names.
fn problem(error: Error) -> SecretError {
    match error.code() {
        USER_CANCELED | AUTH_FAILED => SecretError::Denied,
        INTERACTION_NOT_ALLOWED => SecretError::Locked,
        _ => SecretError::Program(io::Error::other(error)),
    }
}
