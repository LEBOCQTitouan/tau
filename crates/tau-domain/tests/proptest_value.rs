//! Property test: arbitrary Value round-trips through serde_json.

#![cfg(feature = "serde")]

use proptest::prelude::*;

use tau_domain::Value;

fn arb_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Integer),
        any::<f64>()
            .prop_filter("normal or zero", |f| f.is_finite()
                && (f.is_normal() || *f == 0.0))
            .prop_map(Value::Float),
        ".{0,16}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(".{0,8}", inner, 0..4).prop_map(Value::Object),
        ]
    })
}

proptest! {
    #[test]
    fn value_round_trips_through_json(v in arb_value()) {
        let s = serde_json::to_string(&v).unwrap();
        let back: Value = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(v, back);
    }
}
