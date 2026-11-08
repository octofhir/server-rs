//! Apply the AWS-maintained SigV4 signer with S3's URI rules.
//!
//! # Contents
//! Signing settings and sensitive HTTP header installation for a bodyless GET.
//!
//! # Invariants
//! Signing does not normalize or double-encode S3 object paths. Secret-bearing
//! signer errors are replaced with fixed diagnostics; credentials stay in headers.

use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    PayloadChecksumKind, PercentEncodingMode, SignableBody, SignableRequest, SigningSettings,
    UriPathNormalizationMode, sign as aws_sign,
};
use aws_sigv4::sign::v4;

pub(super) fn sign(
    request: &mut reqwest::Request,
    credentials: &Credentials,
    region: &str,
) -> Result<(), String> {
    let identity = credentials.clone().into();
    let mut settings = SigningSettings::default();
    settings.percent_encoding_mode = PercentEncodingMode::Single;
    settings.uri_path_normalization_mode = UriPathNormalizationMode::Disabled;
    settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("s3")
        .time(std::time::SystemTime::now())
        .settings(settings)
        .build()
        .map_err(|_| "Cannot configure S3 request signing")?
        .into();
    let signable = SignableRequest::new(
        request.method().as_str(),
        request.url().as_str(),
        request
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str(), value.to_str().unwrap_or_default())),
        SignableBody::Bytes(&[]),
    )
    .map_err(|_| "Cannot prepare S3 request signing")?;
    let (instructions, _) = aws_sign(signable, &params)
        .map_err(|_| "Cannot sign S3 request")?
        .into_parts();
    for (name, value) in instructions.headers() {
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "Invalid S3 signing header")?;
        let mut value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| "Invalid S3 signing header")?;
        value.set_sensitive(true);
        request.headers_mut().insert(name, value);
    }
    Ok(())
}
