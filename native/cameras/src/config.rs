use crate::CameraError;
use std::collections::BTreeMap;

/// Camera participant names use 1–40 lowercase ASCII letters, digits, underscores.
///
/// # Errors
/// Returns a configuration error for an invalid name.
pub fn validate_camera_name(name: &str) -> Result<(), CameraError> {
    if name.is_empty()
        || name.len() > 40
        || !name.bytes().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == b'_'
        })
    {
        return Err(CameraError::Configuration(format!(
            "invalid camera name {name:?}; use 1–40 lowercase letters, digits or underscores"
        )));
    }
    Ok(())
}

/// Configure a camera from an explicit `name=device` mapping.
///
/// # Errors
/// Returns a configuration error for an invalid name or specification.
pub fn configure_camera(
    cameras: &mut BTreeMap<String, String>,
    specification: &str,
) -> Result<(), CameraError> {
    let (name, device) = specification.split_once('=').ok_or_else(|| {
        CameraError::Configuration(format!("expected name=device, got {specification:?}"))
    })?;
    validate_camera_name(name)?;
    if device.is_empty() {
        return Err(CameraError::Configuration(format!(
            "empty device for {name}"
        )));
    }
    cameras.insert(name.to_owned(), device.to_owned());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_camera_name;

    #[test]
    fn camera_names_are_safe_and_bounded() {
        for name in ["a", "front_2", "a".repeat(40).as_str()] {
            assert!(validate_camera_name(name).is_ok());
        }
        for name in ["", "UPPER", "../outside", "café", "a".repeat(41).as_str()] {
            assert!(validate_camera_name(name).is_err());
        }
    }
}
