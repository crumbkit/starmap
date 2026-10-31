//! The variables that the example application reads once, in `main`.

#[derive(Debug, starmap::Env)]
pub(crate) struct Env {
    #[starmap("SHELF_ENV")]
    stage: Stage,
    #[starmap("SHELF_PORT")]
    port: starmap::Port,
    #[starmap("SHELF_CATALOGUE_URL")]
    catalogue: Option<url::Url>,
    #[starmap("SHELF_DATABASE")]
    database: std::path::PathBuf,
    #[starmap("SHELF_DEBUG")]
    debug: bool,
    #[starmap("SHELF_PAGE_SIZE")]
    page_size: u8,
    #[starmap("SHELF_MIRROR_TOKEN")]
    mirror_token: Option<starmap::Secret<String>>,
    #[starmap("SHELF_SESSION_KEY")]
    session_key: starmap::Secret<String>,
}

#[derive(Debug, starmap::Enumeration)]
pub(crate) enum Stage {
    Dev,
    Prod,
}
