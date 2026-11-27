use crate::scan::{Target, lines_of_reads};

/// Returns each read of a literal name in `text` as its name and line.
fn reads(text: &str) -> Vec<(String, usize)> {
    lines_of_reads(text)
        .unwrap()
        .into_iter()
        .filter_map(|read| match read.target {
            Target::Variable(name) => Some((name.as_str().to_owned(), read.line)),
            Target::Family(_) | Target::Unresolved => None,
        })
        .collect()
}

/// Returns `name` read on `line`.
fn read(name: &str, line: usize) -> (String, usize) {
    (name.to_owned(), line)
}

#[test]
fn a_method_call_with_a_literal_name_on_a_parameter_of_from_source_is_a_read() {
    let text = "impl Config {\n    fn from_source(source: &S) -> Self {\n        let port = source.get(\"APP_PORT\");\n    }\n}\n";
    assert_eq!(reads(text), [read("APP_PORT", 3)]);
}

#[test]
fn a_parameter_typed_impl_source_dyn_config_source_or_a_bounded_generic_is_a_source() {
    let text = "fn a(env: &impl Source) { env.get(\"A\"); }\n\
                fn b(env: Box<dyn ConfigSource>) { env.get(\"B\"); }\n\
                fn c<S: Source + Sync>(env: &S) { env.get(\"C\"); }\n\
                fn d<S>(env: &S) where S: Source { env.get(\"D\"); }\n";
    assert_eq!(
        reads(text),
        [read("A", 1), read("B", 2), read("C", 3), read("D", 4)]
    );
}

#[test]
fn a_call_that_passes_the_source_and_then_a_literal_name_is_a_read() {
    let text = "fn from_source(source: &S) {\n    required(source, \"APP_URL\");\n    Self::parse(&source, \"APP_TIMEOUT\",);\n}\n";
    assert_eq!(reads(text), [read("APP_URL", 2), read("APP_TIMEOUT", 3)]);
}

#[test]
fn a_turbofish_method_call_and_a_call_with_a_default_after_the_name_are_reads() {
    let text = "fn from_source(source: &S) {\n    source.parse::<u16>(\"APP_PORT\");\n    source.get_or(\"APP_HOST\", \"localhost\");\n}\n";
    assert_eq!(reads(text), [read("APP_PORT", 2), read("APP_HOST", 3)]);
}

#[test]
fn a_method_call_on_a_parameter_that_is_no_source_is_not_a_read() {
    let text = "fn f(map: &HashMap<String, String>, s: &S) where S: Sink {\n    map.get(\"APP_PORT\");\n    s.get(\"APP_HOST\");\n}\n";
    assert_eq!(reads(text), []);
}

#[test]
fn a_lower_case_name_or_a_name_that_is_not_a_literal_through_a_source_is_not_a_read() {
    let text =
        "fn from_source(source: &S) {\n    source.get(\"orders\");\n    source.get(NAME);\n}\n";
    assert_eq!(reads(text), []);
}

#[test]
fn a_field_of_the_same_name_as_the_source_is_not_the_source() {
    let text = "fn from_source(source: &S) {\n    self.source.get(\"APP_PORT\");\n}\n";
    assert_eq!(reads(text), []);
}

#[test]
fn a_read_through_a_source_in_test_code_is_skipped() {
    let text = "#[cfg(test)]\nmod tests {\n    fn from_source(source: &S) {\n        source.get(\"APP_PORT\");\n    }\n}\n";
    assert_eq!(reads(text), []);
}

#[test]
fn a_function_type_and_a_declaration_without_a_body_read_nothing() {
    let text = "type F = fn(u8) -> u8;\ntrait Load {\n    fn from_source(source: &impl Source) -> Self;\n}\nfn g() { source.get(\"APP_PORT\"); }\n";
    assert_eq!(reads(text), []);
}
