//! Local QIDO and DICOMweb transport contract against an independent PACS peer.
#[path = "common/dicom_gateway_protocol.rs"]
mod protocol;

#[test]
fn standard_gateway_contract_preserves_authorized_membership() {
    protocol::run_scenario(false);
}
