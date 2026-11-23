use crate::error::PublisherError;

/// A named native camera and the operating-system identifier used to open it.
pub struct CameraConfiguration {
    name: String,
    device: CameraDevice,
    pub publishing: PublishingOptions,
}

/// The camera identity passed to the native capture process.
#[derive(Clone)]
pub enum CameraDevice {
    Selector(String),
    AvFoundationId(u64),
}

impl CameraDevice {
    pub(crate) fn reopen_policy(&self) -> ReopenPolicy {
        match self {
            Self::Selector(device) => ReopenPolicy::for_device(device),
            // WHY: The outer supervisor can resolve the USB identity again after replugging.
            Self::AvFoundationId(_) => ReopenPolicy::Stop,
        }
    }
}

impl std::fmt::Display for CameraDevice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Selector(device) => formatter.write_str(device),
            Self::AvFoundationId(id) => write!(formatter, "{id:#018x}"),
        }
    }
}

impl CameraConfiguration {
    /// Parse a camera name and device identifier at the input boundary.
    ///
    /// # Errors
    /// Returns an error if the name is invalid or the device identifier is empty.
    pub fn new(name: String, device: String) -> Result<Self, PublisherError> {
        Self::for_device(name, CameraDevice::Selector(device))
    }

    /// Configure a named camera using a native device selector.
    ///
    /// # Errors
    /// Returns an error for an invalid name or an empty explicit selector.
    pub fn for_device(name: String, device: CameraDevice) -> Result<Self, PublisherError> {
        cameras::validate_camera_name(&name)?;
        if matches!(&device, CameraDevice::Selector(selector) if selector.is_empty()) {
            return Err(PublisherError::Configuration(
                "camera device cannot be empty".into(),
            ));
        }
        Ok(Self {
            name,
            device,
            publishing: PublishingOptions::default(),
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn device(&self) -> &CameraDevice {
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
    fn resolved_devices_stop_but_persistent_selectors_retry() {
        assert_eq!(
            CameraDevice::AvFoundationId(1).reopen_policy(),
            ReopenPolicy::Stop
        );
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

/// Video publishing choices shared by native integrations and the CLI.
#[derive(Clone, Copy, Debug, Default, clap::Args)]
pub struct PublishingOptions {
    /// Requested video codec; the SDK falls back if unavailable.
    #[arg(
        long = "video-codec",
        env = "LIVEKIT_CAMERA_CODEC",
        value_enum,
        default_value = "vp8"
    )]
    pub codec: CameraCodec,
    /// Report publishing performance every five seconds.
    #[arg(long = "camera-diagnostics", env = "LIVEKIT_CAMERA_DIAGNOSTICS")]
    pub diagnostics: bool,
}

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum CameraCodec {
    #[default]
    Vp8,
    H264,
}

impl CameraCodec {
    #[must_use]
    pub fn livekit_codec(self) -> livekit::options::VideoCodec {
        match self {
            Self::Vp8 => livekit::options::VideoCodec::VP8,
            Self::H264 => livekit::options::VideoCodec::H264,
        }
    }
}
