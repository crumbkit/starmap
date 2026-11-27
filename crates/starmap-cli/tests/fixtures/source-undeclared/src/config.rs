//! Reads LEDGER_PORT, which starmap.toml declares, and LEDGER_REGION, which
//! it does not, through a source of variables. Cargo never builds this
//! file.

pub(crate) trait Source {
    fn get(&self, name: &str) -> Option<String>;
}

pub(crate) struct Config {
    port: Option<String>,
    region: Option<String>,
}

impl Config {
    pub(crate) fn from_source(source: &impl Source) -> Self {
        Self {
            port: source.get("LEDGER_PORT"),
            region: source.get("LEDGER_REGION"),
        }
    }
}
