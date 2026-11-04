//! Tests of the `op inject` template and the split of its output.

use super::{Template, batchable};

/// Returns what `op inject` would print for `template` when the references
/// have `values`.
fn injected(template: &Template, values: &[&[u8]]) -> Vec<u8> {
    let mut output = template.markers[0].as_bytes().to_vec();
    for (value, marker) in values.iter().zip(&template.markers[1..]) {
        output.extend_from_slice(value);
        output.extend_from_slice(marker.as_bytes());
    }
    output
}

#[test]
fn a_reference_of_letters_digits_spaces_and_punctuation_marks_is_batchable() {
    for reference in [
        "op://Personal/shelf mirror/token",
        "op://Dev vault/api_key.v2/section-1/credential",
    ] {
        assert!(batchable(reference), "{reference}");
    }
}

#[test]
fn a_reference_that_a_template_would_change_or_refuse_is_not_batchable() {
    for reference in [
        "op://Personal/$ITEM/token",
        "op://Personal/${ITEM}/token",
        "op://Personal/${ITEM:-shelf}/token",
        "op://Personal/shelf {{mirror}}/token",
        "op://Personal/shelf/token?attribute=otp",
        "op://Personal/shelf!/token",
        "op://Personal/\"shelf\"/token",
        "op://Personal/shelf[0]/token",
        "op://Personal/shelf (old)/token",
        "op://Personal/ shelf/token",
        "op://Personal/shelf /token",
        "op://Personal/café/token",
    ] {
        assert!(!batchable(reference), "{reference}");
    }
}

#[test]
fn the_template_holds_each_reference_between_markers_of_32_hex_digits() {
    let template = Template::new(&["op://A/b/c", "op://A/d/e"]).unwrap();
    assert_eq!(template.markers.len(), 3);
    for marker in &template.markers {
        assert_eq!(marker.len(), 32);
        assert!(marker.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    let [m0, m1, m2] = template.markers.as_slice() else {
        panic!();
    };
    assert_eq!(
        template.text(),
        format!("{m0}{{{{ op://A/b/c }}}}{m1}{{{{ op://A/d/e }}}}{m2}")
    );
}

#[test]
fn each_template_has_markers_of_its_own() {
    let first = Template::new(&["op://A/b/c"]).unwrap();
    let second = Template::new(&["op://A/b/c"]).unwrap();
    assert_ne!(first.markers, second.markers);
}

#[test]
fn the_split_returns_each_value_exactly_with_its_newlines_and_bytes() {
    let template = Template::new(&["op://A/b/c", "op://A/d/e", "op://A/f/g"]).unwrap();
    let values: [&[u8]; 3] = [b" padded\n", b"line one\nline two\n\n", b"\xff\x00"];
    let output = injected(&template, &values);
    assert_eq!(template.split(&output).unwrap(), values);
}

#[test]
fn the_split_accepts_one_newline_after_the_last_marker() {
    let template = Template::new(&["op://A/b/c"]).unwrap();
    let mut output = injected(&template, &[b"token"]);
    output.push(b'\n');
    assert_eq!(template.split(&output).unwrap(), [b"token"]);
}

#[test]
fn the_split_refuses_output_whose_markers_are_missing_repeated_or_out_of_order() {
    let template = Template::new(&["op://A/b/c", "op://A/d/e"]).unwrap();
    let [m0, m1, m2] = template.markers.as_slice() else {
        panic!();
    };
    let outputs = [
        // A marker is missing.
        format!("{m0}one{m2}"),
        // A value holds a marker, so the marker appears twice.
        format!("{m0}one{m1}two{m1}{m2}"),
        // The markers are out of order.
        format!("{m0}one{m2}two{m1}"),
        // Text comes before the first marker or after the last.
        format!("x{m0}one{m1}two{m2}"),
        format!("{m0}one{m1}two{m2}x"),
        // op printed the template unchanged.
        template.text().to_owned(),
        String::new(),
    ];
    for output in outputs {
        let values = template.split(output.as_bytes());
        assert!(values.is_none(), "{output}");
    }
}
