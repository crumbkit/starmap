//! Reads SHELF_PORT, a member of each family, and SHELF_TOKEN_INITECH,
//! whose key the family SHELF_TOKEN does not list. Cargo never builds this
//! file.

pub(crate) fn regions() {
    let _port = std::env::var("SHELF_PORT");
    let _eu = std::env::var("SHELF_DB_URL_EU");
    let _acme = std::env::var("SHELF_TOKEN_ACME");
    let _initech = std::env::var("SHELF_TOKEN_INITECH");
}
