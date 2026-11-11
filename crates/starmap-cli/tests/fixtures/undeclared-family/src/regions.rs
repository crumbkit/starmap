//! Reads SHELF_PORT, a member of the declared family SHELF_DB_URL, a
//! member of SHELF_CACHE_URL, which the spec does not declare, and a name
//! that starts with a placeholder, which the scan cannot resolve. Cargo
//! never builds this file.

pub(crate) fn regions(region: &str, tenant: &str) {
    let _port = std::env::var("SHELF_PORT");
    let _db = std::env::var(format!("SHELF_DB_URL_{region}"));
    let _cache = std::env::var(&std::format!("SHELF_CACHE_URL_{}", region));
    let _token = std::env::var(format!("{tenant}_TOKEN"));
}
