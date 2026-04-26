//! Property tests for `PackageSource` parser/round-trip.

use proptest::prelude::*;
use std::str::FromStr;

use tau_domain::{PackageSource, PackageSourceError};

fn arb_url_source() -> impl Strategy<Value = String> {
    let scheme = prop_oneof![Just("https"), Just("http"), Just("ssh"), Just("git")];
    let host = "[a-z][a-z0-9-]{0,30}(\\.[a-z][a-z0-9-]{0,30}){1,3}";
    let path = "[a-z0-9]{1,20}(/[a-z0-9]{1,20}){0,3}\\.git";
    (scheme, host, path).prop_map(|(s, h, p)| format!("{s}://{h}/{p}"))
}

fn arb_scp_source() -> impl Strategy<Value = String> {
    let user = "[a-z][a-z0-9]{0,15}";
    let host = "[a-z][a-z0-9-]{0,30}(\\.[a-z][a-z0-9-]{0,30}){1,3}";
    let path = "[a-z0-9]{1,20}(/[a-z0-9]{1,20}){0,3}\\.git";
    (user, host, path).prop_map(|(u, h, p)| format!("{u}@{h}:{p}"))
}

proptest! {
    #[test]
    fn url_source_round_trips(s in arb_url_source()) {
        let parsed = PackageSource::from_str(&s).unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn scp_source_round_trips(s in arb_scp_source()) {
        let parsed = PackageSource::from_str(&s).unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn empty_input_rejected(_unit in any::<()>()) {
        prop_assert_eq!(PackageSource::from_str(""), Err(PackageSourceError::Empty));
    }

    #[test]
    fn empty_rev_rejected(s in arb_url_source()) {
        let with_empty = format!("{s}#");
        prop_assert_eq!(PackageSource::from_str(&with_empty), Err(PackageSourceError::EmptyRevision));
    }
}
