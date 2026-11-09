//! Origin-backed QIDO runs in its own process to isolate server bootstrap caches.
#[path = "common/dicom_gateway_protocol.rs"]
mod protocol;

#[test]
fn remote_qido_preserves_authorized_membership_and_origin_matching() {
    protocol::run_scenario(true);
}
