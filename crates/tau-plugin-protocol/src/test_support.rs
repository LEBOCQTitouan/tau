//! Test-support helpers for driving the plugin protocol without
//! spawning real processes. Behind the `test-support` cargo feature.
//!
//! See spec §10.2 for the design rationale and intended usage.

use serde::Serialize;
use tokio::io::DuplexStream;

use crate::error::ProtocolError;
use crate::frame::Frame;
use crate::framer::{FramedReader, FramedWriter, FramerOptions};
use crate::handshake::{HandshakeRequest, HandshakeResponse};

/// One side of a synthetic stdio pair used to drive the plugin
/// protocol from tests without a real process.
///
/// Spawn with [`FakeStdioPeer::new`] which returns a `(peer, reader,
/// writer)` triple. The peer is the test-driven side; the reader and
/// writer represent what a normal plugin/host pair would see.
pub struct FakeStdioPeer {
    /// Frames written by the system-under-test arrive here.
    pub reader: FramedReader<DuplexStream>,
    /// Frames written here are read by the system-under-test.
    pub writer: FramedWriter<DuplexStream>,
}

impl FakeStdioPeer {
    /// Create a fake-peer pair plus the corresponding framer pair the
    /// peer's "other side" would see.
    ///
    /// The first returned value is the peer (test-driven). The second
    /// is the [`FramedReader`] that the system-under-test should read
    /// from (i.e., this is what an `IpcLlmBackend` or
    /// `run_llm_backend` would consume). The third is the
    /// [`FramedWriter`] for the SUT to write to.
    pub fn new() -> (Self, FramedReader<DuplexStream>, FramedWriter<DuplexStream>) {
        // Two duplex pairs: peer-side reader pairs with SUT-side writer,
        // peer-side writer pairs with SUT-side reader.
        let (peer_read_half, sut_write_half) = tokio::io::duplex(64 * 1024);
        let (sut_read_half, peer_write_half) = tokio::io::duplex(64 * 1024);
        let peer = FakeStdioPeer {
            reader: FramedReader::new(peer_read_half, FramerOptions::default()),
            writer: FramedWriter::new(peer_write_half),
        };
        let sut_reader = FramedReader::new(sut_read_half, FramerOptions::default());
        let sut_writer = FramedWriter::new(sut_write_half);
        (peer, sut_reader, sut_writer)
    }

    /// Receive the next frame from the SUT and decode it as a
    /// [`Frame::Request`] containing a `meta.handshake`. Panics if the
    /// frame is not a Request, the method isn't `meta.handshake`, or
    /// the params don't decode.
    pub async fn expect_handshake(&mut self) -> (u32, HandshakeRequest) {
        let body = self
            .reader
            .next_frame()
            .await
            .expect("frame read")
            .expect("clean EOF before handshake");
        let frame = Frame::decode(&body).expect("Frame::decode");
        let Frame::Request { id, method, params } = frame else {
            panic!("expected Request, got {frame:?}")
        };
        assert_eq!(
            method,
            crate::handshake::meta::HANDSHAKE_METHOD,
            "expected meta.handshake, got {method:?}"
        );
        // params is the rmp-serde-encoded Vec<HandshakeRequest>
        // (single-element vec since params is array-shaped).
        let parsed: Vec<HandshakeRequest> =
            rmp_serde::from_slice(&params).expect("params decode as [HandshakeRequest]");
        assert_eq!(
            parsed.len(),
            1,
            "handshake params must be a 1-element array"
        );
        (id, parsed.into_iter().next().unwrap())
    }

    /// Send a [`Frame::Response`] carrying the given handshake response
    /// on the given msgid.
    pub async fn send_handshake_response(
        &mut self,
        id: u32,
        resp: HandshakeResponse,
    ) -> Result<(), ProtocolError> {
        let result_bytes = rmp_serde::to_vec(&resp)?;
        let frame = Frame::Response {
            id,
            error: None,
            result: Some(result_bytes),
        };
        let body = frame.encode()?;
        self.writer.write_frame(&body).await
    }

    /// Receive the next request frame and assert its method matches.
    /// Returns `(msgid, raw params bytes)`.
    pub async fn expect_request(&mut self, expected_method: &str) -> (u32, Vec<u8>) {
        let body = self
            .reader
            .next_frame()
            .await
            .expect("frame read")
            .expect("clean EOF before request");
        let frame = Frame::decode(&body).expect("Frame::decode");
        let Frame::Request { id, method, params } = frame else {
            panic!("expected Request, got {frame:?}")
        };
        assert_eq!(
            method, expected_method,
            "expected method {expected_method:?}, got {method:?}"
        );
        (id, params)
    }

    /// Send a [`Frame::Response`] with the given result, encoded via
    /// `rmp-serde`.
    pub async fn send_response<T: Serialize>(
        &mut self,
        id: u32,
        result: T,
    ) -> Result<(), ProtocolError> {
        let result_bytes = rmp_serde::to_vec(&result)?;
        let frame = Frame::Response {
            id,
            error: None,
            result: Some(result_bytes),
        };
        let body = frame.encode()?;
        self.writer.write_frame(&body).await
    }

    /// Send a [`Frame::Response`] with an error envelope.
    pub async fn send_response_error(
        &mut self,
        id: u32,
        code: i32,
        message: &str,
    ) -> Result<(), ProtocolError> {
        let envelope = crate::error::RpcErrorEnvelope {
            code,
            message: message.to_string(),
            data: None,
        };
        let frame = Frame::Response {
            id,
            error: Some(envelope),
            result: None,
        };
        let body = frame.encode()?;
        self.writer.write_frame(&body).await
    }

    /// Send a `stream.chunk` notification carrying the originating
    /// msgid + chunk.
    pub async fn send_stream_chunk<T: Serialize>(
        &mut self,
        originating_id: u32,
        chunk: T,
    ) -> Result<(), ProtocolError> {
        // The stream.chunk notification's params is [originating_id, chunk]
        let params_value = (originating_id, chunk);
        let params_bytes = rmp_serde::to_vec(&params_value)?;
        let frame = Frame::Notification {
            method: "stream.chunk".to_string(),
            params: params_bytes,
        };
        let body = frame.encode()?;
        self.writer.write_frame(&body).await
    }

    /// Drop the peer (closes both transport halves). The SUT will see
    /// EOF on its next read; in-flight calls return `PluginCrashed`
    /// from the host-side framer perspective.
    pub fn send_crash(self) {
        // Default drop closes both halves.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handshake::{
        meta, HandshakeRequest, HandshakeResponse, TraceContext, PROTOCOL_VERSION,
    };
    use std::collections::BTreeMap;
    use tau_domain::PortKind;

    #[tokio::test]
    async fn fake_peer_drives_handshake_roundtrip() {
        let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

        // SUT (acting like the host) sends a handshake request.
        let req = HandshakeRequest::new(
            PROTOCOL_VERSION.to_string(),
            PortKind::LlmBackend,
            TraceContext::new("r".into(), "a".into(), "s".into()),
            serde_json::Value::Null,
        );
        let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
        let frame = Frame::Request {
            id: 1,
            method: meta::HANDSHAKE_METHOD.to_string(),
            params: params_bytes,
        };
        sut_writer
            .write_frame(&frame.encode().unwrap())
            .await
            .unwrap();

        // Peer receives + asserts.
        let (msgid, parsed) = peer.expect_handshake().await;
        assert_eq!(msgid, 1);
        assert_eq!(parsed, req);

        // Peer responds.
        let resp = HandshakeResponse::new(
            PROTOCOL_VERSION.to_string(),
            PortKind::LlmBackend,
            "echo".to_string(),
            "0.1.0".to_string(),
            vec![],
            BTreeMap::new(),
        );
        peer.send_handshake_response(msgid, resp.clone())
            .await
            .unwrap();

        // SUT reads the response.
        let response_body = sut_reader.next_frame().await.unwrap().unwrap();
        let response_frame = Frame::decode(&response_body).unwrap();
        let Frame::Response { id, error, result } = response_frame else {
            panic!()
        };
        assert_eq!(id, 1);
        assert!(error.is_none());
        let result_bytes = result.unwrap();
        let parsed_resp: HandshakeResponse = rmp_serde::from_slice(&result_bytes).unwrap();
        assert_eq!(parsed_resp, resp);
    }
}
