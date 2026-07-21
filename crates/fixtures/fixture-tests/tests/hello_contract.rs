use boxology_contract::{
    ContractError, ContractType, ContractValue, DecodeErrorKind, OpaquePayload, OpaqueTree,
    PathSegment, SlotValue, ValueRef,
};
use hello_contract::GreetError;

#[test]
fn empty_name_has_exact_wire_shape_and_round_trips() {
    let error = GreetError::EmptyName;
    let encoded = error.encode().unwrap();
    let SlotValue::Value(value) = &encoded else {
        panic!("errors must encode as values");
    };
    let ValueRef::Enum { tag, payload } = value.view() else {
        panic!("errors must encode as enums");
    };
    assert_eq!(tag, "EmptyName");
    assert_eq!(payload, &SlotValue::Null);
    assert_eq!(GreetError::decode(&encoded).unwrap(), error);
    assert_eq!(error.error_tag(), "EmptyName");
}

#[test]
fn empty_name_rejects_non_null_payload_at_the_variant_path() {
    let encoded = SlotValue::Value(ContractValue::enum_value(
        "EmptyName",
        SlotValue::Value(ContractValue::string("unexpected")),
    ));
    let error = GreetError::decode(&encoded).unwrap_err();
    assert_eq!(error.kind(), &DecodeErrorKind::UnexpectedPayload);
    assert_eq!(error.path(), &[PathSegment::Variant("EmptyName".into())]);
}

#[test]
fn opaque_unknown_variant_round_trips_forwards_and_redacts() {
    const SECRET: &str = "future-error-detail";
    let tree = OpaqueTree::Object(vec![("detail".into(), OpaqueTree::String(SECRET.into()))]);
    let error = GreetError::Unknown {
        tag: "FutureError".into(),
        payload: OpaquePayload::new(tree.clone()),
    };
    let encoded = error.encode().unwrap();
    let decoded = GreetError::decode(&encoded).unwrap();
    assert_eq!(decoded, error);
    assert_eq!(decoded.encode().unwrap(), encoded);
    let GreetError::Unknown { tag, payload } = &decoded else {
        panic!("unknown tag must remain unknown");
    };
    assert_eq!(tag, "FutureError");
    assert_eq!(payload.reveal(), &tree);
    assert_eq!(payload.forward().reveal(), &tree);
    let debug = format!("{decoded:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(SECRET));
}

#[test]
fn raw_unknown_variant_is_rejected_at_the_variant_path() {
    let encoded = SlotValue::Value(ContractValue::enum_value(
        "FutureError",
        SlotValue::Value(ContractValue::string("not conformed")),
    ));
    let error = GreetError::decode(&encoded).unwrap_err();
    assert_eq!(
        error.kind(),
        &DecodeErrorKind::UnknownVariant("FutureError".into())
    );
    assert_eq!(error.path(), &[PathSegment::Variant("FutureError".into())]);
}

#[test]
fn generated_contract_has_no_runtime_or_http_dependency() {
    let manifest = include_str!("../../hello/generated/contract/Cargo.toml");
    let dependencies = manifest
        .split_once("[dependencies]\n")
        .expect("generated contract must declare dependencies")
        .1
        .trim();
    assert_eq!(
        dependencies,
        "boxology-contract = { version = \"=0.0.0\", path = \"../../../../boxology-contract\" }"
    );
    assert!(!dependencies.contains("runtime"));
    assert!(!dependencies.contains("http"));
}
