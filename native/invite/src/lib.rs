//! Browser camera invite links and QR codes. Token issuance belongs to the caller.
use base64::{Engine, engine::general_purpose::STANDARD};
use qrcode::{EcLevel, QrCode, render::svg};

#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    #[error("camera page URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("camera invite QR code: {0}")]
    Qr(#[from] qrcode::types::QrError),
}

/// Contains a bearer token; deliberately does not implement `Debug`.
pub struct CameraInvite {
    pub url: String,
    pub qr_code_image: String,
}

/// Creates an invite for the browser camera page using an already-issued token.
/// Credentials are encoded in the URL fragment, matching the browser page protocol.
/// Existing query parameters are preserved; an existing fragment is replaced.
/// Treat both returned fields as secrets because either grants room access.
///
/// # Errors
/// Returns an invalid page URL or a payload too large for a QR code.
pub fn camera_invite(
    page_url: &str,
    server_url: &str,
    token: &str,
) -> Result<CameraInvite, InviteError> {
    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("url", server_url)
        .append_pair("token", token)
        .finish();
    let mut invite_url = url::Url::parse(page_url)?;
    invite_url.set_fragment(Some(&fragment));
    let url = invite_url.to_string();
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L)?;
    // qrcode's SVG renderer has a four-module quiet zone. Render the matrix
    // ourselves to preserve the requested two-module border and scale four.
    let matrix_svg = code
        .render::<svg::Color>()
        .quiet_zone(false)
        .module_dimensions(4, 4)
        .build();
    let matrix_svg = matrix_svg
        .split_once("?>")
        .map_or(matrix_svg.as_str(), |(_, body)| body);
    let inner_size = code.width() * 4;
    let size = inner_size + 16;
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="-8 -8 {size} {size}"><rect x="-8" y="-8" width="{size}" height="{size}" fill="white"/>{matrix_svg}</svg>"#
    );
    let qr_code_image = format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg));
    Ok(CameraInvite { url, qr_code_image })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_preserves_page_query_and_encodes_credentials_in_fragment()
    -> Result<(), Box<dyn std::error::Error>> {
        let invite = camera_invite(
            "https://example.com/cameras/?mode=publish#old",
            "wss://media.example.com/path?region=a&secure=true",
            "token+with/special=characters&",
        )?;
        let parsed = url::Url::parse(&invite.url)?;
        assert_eq!(parsed.query(), Some("mode=publish"));
        let parameters: Vec<_> =
            url::form_urlencoded::parse(parsed.fragment().ok_or("missing fragment")?.as_bytes())
                .into_owned()
                .collect();
        assert_eq!(
            parameters,
            vec![
                (
                    "url".to_owned(),
                    "wss://media.example.com/path?region=a&secure=true".to_owned()
                ),
                (
                    "token".to_owned(),
                    "token+with/special=characters&".to_owned()
                )
            ]
        );
        let encoded_svg = invite
            .qr_code_image
            .strip_prefix("data:image/svg+xml;base64,")
            .ok_or("missing SVG data URL")?;
        let svg = String::from_utf8(STANDARD.decode(encoded_svg)?)?;
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains(r#"viewBox="-8 -8"#));
        Ok(())
    }

    #[test]
    fn rejects_invalid_page_and_oversized_invite() {
        assert!(matches!(
            camera_invite("not a URL", "wss://example.com", "token"),
            Err(InviteError::Url(_))
        ));
        assert!(matches!(
            camera_invite(
                "https://example.com/",
                "wss://example.com",
                &"a".repeat(10_000)
            ),
            Err(InviteError::Qr(_))
        ));
    }
}
