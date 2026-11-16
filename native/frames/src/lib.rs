//! Shared access to the latest received image from each video track.

mod conversion;
mod subscriber;
pub use conversion::{FrameError, camera_frame};
pub use subscriber::subscribe;

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Instant,
};

/// `LiveKit` identity information, independent of application camera naming.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackIdentity {
    pub participant: String,
    pub track_sid: String,
}

/// A JPEG image with source time and local receipt time.
#[derive(Clone, Debug)]
pub struct CameraFrame {
    pub jpeg: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
    pub captured_at_unix_ns: u128,
    pub received_at: Instant,
}

/// The observable state of a known or unknown track.
#[derive(Clone, Debug)]
pub enum LatestFrame {
    UnknownTrack,
    Waiting,
    Available(CameraFrame),
}

#[derive(Clone)]
struct TrackFrames {
    identity: TrackIdentity,
    latest: Option<CameraFrame>,
}

/// Retains at most one image per registered track. Readers share image bytes.
#[derive(Clone, Default)]
pub struct CameraFrames(Arc<RwLock<HashMap<String, TrackFrames>>>);

impl CameraFrames {
    /// Register a track, replacing any prior generation's image.
    pub fn register(&self, identity: TrackIdentity) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                identity.track_sid.clone(),
                TrackFrames {
                    identity,
                    latest: None,
                },
            );
    }

    /// Replace a registered track's latest image. Returns false for an unknown track.
    pub fn insert(&self, track_sid: &str, frame: CameraFrame) -> bool {
        let mut tracks = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(track) = tracks.get_mut(track_sid) {
            track.latest = Some(frame);
            true
        } else {
            false
        }
    }

    /// Remove a disconnected track and release its retained image.
    pub fn remove(&self, track_sid: &str) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(track_sid);
    }

    /// Inspect the latest image without copying its JPEG bytes.
    #[must_use]
    pub fn latest(&self, track_sid: &str) -> LatestFrame {
        match self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(track_sid)
        {
            None => LatestFrame::UnknownTrack,
            Some(track) => match &track.latest {
                None => LatestFrame::Waiting,
                Some(frame) => LatestFrame::Available(frame.clone()),
            },
        }
    }

    /// List registered tracks, including tracks still waiting for an image.
    #[must_use]
    pub fn tracks(&self) -> Vec<TrackIdentity> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|track| track.identity.clone())
            .collect()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Test assertions fail immediately on unexpected outcomes"
)]
mod tests {
    use super::*;

    fn frame(timestamp: u128) -> CameraFrame {
        CameraFrame {
            jpeg: Arc::from([1_u8, 2, 3]),
            width: 1,
            height: 1,
            captured_at_unix_ns: timestamp,
            received_at: Instant::now(),
        }
    }

    #[test]
    fn readers_get_the_newest_shared_image_until_the_track_is_removed() {
        let frames = CameraFrames::default();
        let identity = TrackIdentity {
            participant: "camera".into(),
            track_sid: "track_1".into(),
        };
        assert!(matches!(
            frames.latest("track_1"),
            LatestFrame::UnknownTrack
        ));
        assert!(!frames.insert("track_1", frame(1)));
        frames.register(identity.clone());
        assert!(matches!(frames.latest("track_1"), LatestFrame::Waiting));
        assert!(frames.insert("track_1", frame(1)));
        let newest = frame(2);
        assert!(frames.insert("track_1", newest.clone()));
        let LatestFrame::Available(first_reader) = frames.latest("track_1") else {
            panic!("missing latest frame")
        };
        let LatestFrame::Available(second_reader) = frames.clone().latest("track_1") else {
            panic!("missing shared frame")
        };
        assert_eq!(first_reader.captured_at_unix_ns, 2);
        assert!(Arc::ptr_eq(&first_reader.jpeg, &newest.jpeg));
        assert!(Arc::ptr_eq(&second_reader.jpeg, &first_reader.jpeg));
        frames.register(TrackIdentity {
            participant: identity.participant.clone(),
            track_sid: "track_2".into(),
        });
        frames.remove("track_1");
        assert!(matches!(
            frames.latest("track_1"),
            LatestFrame::UnknownTrack
        ));
        assert!(matches!(frames.latest("track_2"), LatestFrame::Waiting));
        assert_eq!(frames.tracks().len(), 1);
    }
}
