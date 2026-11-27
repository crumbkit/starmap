//! Builds the config from a source of variables, as an application does
//! when its tests pass a map in place of the process environment. Cargo
//! never builds this file.

/// A source of variables: the process environment, or a map in a test.
pub(crate) trait Source {
    /// Returns the value of the variable `name`, if it is set.
    fn get(&self, name: &str) -> Option<String>;
}

/// The process environment.
pub(crate) struct Env;

impl Source for Env {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

pub(crate) struct Config {
    port: String,
    host: String,
    database_url: String,
    timeout: String,
    pool_size: String,
    tls: Option<(String, String)>,
    admin_token: String,
}

impl Config {
    /// Reads every variable of the config from `source`.
    pub(crate) fn from_source<S: Source>(source: &S) -> Result<Self, String> {
        Ok(Self {
            port: source.get("LEDGER_PORT").unwrap_or_default(),
            host: source
                .get("LEDGER_HOST")
                .unwrap_or_else(|| "127.0.0.1".to_owned()),
            database_url: required(source, "LEDGER_DATABASE_URL")?,
            timeout: Self::parse(&source, "LEDGER_TIMEOUT_SECONDS"),
            pool_size: pool_size(source),
            tls: tls(source),
            admin_token: required(source, "LEDGER_ADMIN_TOKEN")?,
        })
    }

    fn parse(source: &impl Source, name: &str) -> String {
        source.get(name).unwrap_or_default()
    }
}

/// Returns the value of `name`, or a failure that names it.
fn required(source: &impl Source, name: &str) -> Result<String, String> {
    source.get(name).ok_or_else(|| format!("{name} is not set"))
}

fn pool_size(source: &dyn Source) -> String {
    source.get("LEDGER_POOL_SIZE").unwrap_or_default()
}

fn tls<S>(source: &S) -> Option<(String, String)>
where
    S: Source,
{
    Some((source.get("LEDGER_TLS_CERT")?, source.get("LEDGER_TLS_KEY")?))
}

pub(crate) fn main() {
    let _log = std::env::var("LEDGER_LOG");
    let _config = Config::from_source(&Env);
}

#[cfg(test)]
mod tests {
    use super::Source;

    struct Map;

    impl Source for Map {
        fn get(&self, _name: &str) -> Option<String> {
            None
        }
    }

    fn only_in_tests(source: &impl Source) -> Option<String> {
        source.get("LEDGER_TEST_ONLY")
    }

    #[test]
    fn a_test_reads_a_variable_that_the_application_never_reads() {
        let _value = only_in_tests(&Map);
    }
}
