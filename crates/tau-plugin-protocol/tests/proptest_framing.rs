//! Proptest: arbitrary frame bytes round-trip through writer → reader.

use proptest::prelude::*;
use tau_plugin_protocol::{FramedReader, FramedWriter, FramerOptions};
use tokio::io::duplex;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn round_trip_arbitrary_bytes(payload in proptest::collection::vec(any::<u8>(), 0..16384)) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (a, b) = duplex(32 * 1024);
            let mut writer = FramedWriter::new(a);
            let mut reader = FramedReader::new(b, FramerOptions::default());
            writer.write_frame(&payload).await.unwrap();
            let got = reader.next_frame().await.unwrap().unwrap();
            prop_assert_eq!(got, payload);
            Ok(())
        }).unwrap();
    }
}
