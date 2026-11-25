use super::*;

#[test]
fn round_trips_basic_values() {
    let v = parse(r#"{"a":[1,2.5e3,"x\n\u00e9\ud83d\ude00"],"b":null,"c":true}"#).unwrap();
    assert_eq!(parse(&v.to_json()).unwrap(), v);
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let deep = "[".repeat(100_000);
    assert!(parse(&deep).is_err());
    let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(parse(&ok).is_ok());
    let too = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert!(parse(&too).is_err());
}

#[test]
fn skip_agrees_with_parse() {
    let samples = [
        r#"{"a":[1,2.5e3,"x\n\u00e9\ud83d\ude00"],"b":null,"c":true}"#,
        "[]",
        "{}",
        "1",
        r#""a\n""#,
        "01",
        "[1,]",
        r#"{"a":1,"a":2}"#,
        r#""\ud800""#,
    ];
    for sample in samples {
        let parsed = parse(sample);
        let mut parser = Parser::new(sample);
        let skipped = parser.skip_value(0).and_then(|_| parser.finish());
        assert_eq!(parsed.is_ok(), skipped.is_ok(), "{sample} {skipped:?}");
    }
}

#[test]
fn rejects_duplicates_controls_and_lone_surrogates() {
    assert!(parse(r#"{"a":1,"a":2}"#).is_err());
    assert!(parse("\"a\nb\"").is_err());
    assert!(parse(r#""\ud800""#).is_err());
    assert!(parse("01").is_err());
    assert!(parse("[1,]").is_err());
    assert!(parse("{} x").is_err());
}

#[test]
fn integers_only_for_as_u64() {
    assert_eq!(parse("42").unwrap().as_u64(), Some(42));
    assert_eq!(parse("-1").unwrap().as_u64(), None);
    assert_eq!(parse("1.0").unwrap().as_u64(), None);
    assert_eq!(parse("99999999999999999999999").unwrap().as_u64(), None);
}

#[test]
fn integers_and_floats_stay_distinct() {
    assert_eq!(parse("-2").unwrap().as_i64(), Some(-2));
    assert_eq!(parse("1.5").unwrap().as_i64(), None);
    assert_eq!(parse("1e2").unwrap().as_i64(), None);
    assert_eq!(parse("1.5").unwrap().as_f64(), Some(1.5));
    assert_eq!(parse("1").unwrap().as_f64(), Some(1.0));
    assert!(parse("1e9999").unwrap().as_f64().is_none());
    let mut obj = parse(r#"{"a":1,"b":2}"#).unwrap();
    assert_eq!(obj.take("b").unwrap().as_i64(), Some(2));
    obj.insert("a", Value::int(3));
    assert_eq!(obj.get("a").unwrap().as_i64(), Some(3));
    assert!(obj.get("b").is_none());
}
