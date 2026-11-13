use crate::error::PublisherError;

/// A named native camera and the operating-system identifier used to open it.
pub struct CameraConfiguration {
    name: String,
    device: String,
}

impl CameraConfiguration {
    /// Parse a camera name and device identifier at the input boundary.
    ///
    /// # Errors
    /// Returns an error if the name is invalid or the device identifier is empty.
    pub fn new(name: String, device: String) -> Result<Self, PublisherError> {
        cameras::validate_camera_name(&name)?;
        if device.is_empty() {
            return Err(PublisherError::Configuration(
                "camera device cannot be empty".into(),
            ));
        }
        Ok(Self { name, device })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn device(&self) -> &str {
        &self.device
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReopenPolicy {
    Stop,
    Retry,
}

impl ReopenPolicy {
    #[must_use]
    pub fn for_device(device: &str) -> Self {
        if !device.is_empty() && device.bytes().all(|character| character.is_ascii_digit()) {
            Self::Stop
        } else {
            Self::Retry
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_configuration_rejects_invalid_names_and_empty_devices() {
        assert!(CameraConfiguration::new("front".into(), "/dev/video0".into()).is_ok());
        assert!(CameraConfiguration::new("../front".into(), "0".into()).is_err());
        assert!(CameraConfiguration::new("front".into(), String::new()).is_err());
    }

    #[test]
    fn indices_stop_but_persistent_identifiers_retry() {
        for device in ["0", "12", "0002", "99999999999999999999"] {
            assert_eq!(ReopenPolicy::for_device(device), ReopenPolicy::Stop);
        }
        for device in [
            "/dev/video0",
            "/run/cameras/cam_chest",
            "/dev/v4l/by-id/usb-camera",
            "0x123macos",
        ] {
            assert_eq!(ReopenPolicy::for_device(device), ReopenPolicy::Retry);
        }
    }
}
