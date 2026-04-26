//! Property tests for `PackageName` and `AgentId` grammar.

use proptest::prelude::*;
use std::str::FromStr;

use tau_domain::{AgentId, AgentIdError, PackageName, PackageNameError};

proptest! {
    #[test]
    fn package_name_round_trips(s in "[a-z][a-z0-9-]{0,63}") {
        let n = PackageName::from_str(&s).unwrap();
        prop_assert_eq!(n.to_string(), s);
    }

    #[test]
    fn agent_id_round_trips(s in "[a-z][a-z0-9-]{0,63}") {
        let id = AgentId::from_str(&s).unwrap();
        prop_assert_eq!(id.to_string(), s);
    }

    #[test]
    fn package_name_invalid_leading_rejected(s in "[A-Z0-9-][a-z0-9-]{0,63}") {
        let result = PackageName::from_str(&s);
        let ok = matches!(
            result,
            Err(PackageNameError::InvalidLeadingCharacter { .. }) | Err(PackageNameError::Empty)
        );
        prop_assert!(ok);
    }

    #[test]
    fn agent_id_invalid_leading_rejected(s in "[A-Z0-9-][a-z0-9-]{0,63}") {
        let result = AgentId::from_str(&s);
        let ok = matches!(
            result,
            Err(AgentIdError::InvalidLeadingCharacter { .. }) | Err(AgentIdError::Empty)
        );
        prop_assert!(ok);
    }
}
