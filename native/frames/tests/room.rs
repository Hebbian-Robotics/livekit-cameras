use livekit::{
    Room, RoomOptions,
    options::TrackPublishOptions,
    prelude::{LocalTrack, LocalVideoTrack},
    webrtc::{
        video_frame::{I420Buffer, VideoFrame, VideoRotation},
        video_source::{RtcVideoSource, VideoResolution, native::NativeVideoSource},
    },
};
use livekit_camera_frames::{CameraFrames, LatestFrame, subscribe};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

// Requires a real local LiveKit development server and room-scoped test tokens.
#[tokio::test]
#[ignore = "requires TEST_VIEWER_TOKEN and TEST_PUBLISHER_TOKEN with a local LiveKit server"]
async fn multiple_tracks_and_late_arrivals_produce_images_and_cancel_cleanly()
-> Result<(), Box<dyn std::error::Error>> {
    let frames = CameraFrames::default();
    let stopped = CancellationToken::new();
    let subscriber = tokio::spawn({
        let frames = frames.clone();
        let stopped = stopped.clone();
        async move {
            subscribe(
                "ws://127.0.0.1:7880",
                &std::env::var("TEST_VIEWER_TOKEN").unwrap(),
                frames,
                stopped,
            )
            .await
        }
    });
    let (room, _) = Room::connect(
        "ws://127.0.0.1:7880",
        &std::env::var("TEST_PUBLISHER_TOKEN")?,
        RoomOptions::default(),
    )
    .await?;
    let mut sources = Vec::new();
    let mut publications = Vec::new();
    for index in 0..2 {
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 64,
                height: 32,
            },
            false,
        );
        let track = LocalVideoTrack::create_video_track(
            &format!("camera_{index}"),
            RtcVideoSource::Native(source.clone()),
        );
        publications.push(
            room.local_participant()
                .publish_track(LocalTrack::Video(track), TrackPublishOptions::default())
                .await?,
        );
        sources.push(source);
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                for source in &sources {
                    source.capture_frame(&VideoFrame {
                        rotation: VideoRotation::VideoRotation0,
                        timestamp_us: 0,
                        frame_metadata: None,
                        buffer: I420Buffer::new(64, 32),
                    });
                }
                if frames.tracks().len() == sources.len()
                    && frames.tracks().iter().all(|track| {
                        matches!(frames.latest(&track.track_sid), LatestFrame::Available(_))
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await?;
    }
    let tracks = frames.tracks();
    assert_eq!(tracks.len(), 2);
    assert_ne!(tracks[0].track_sid, tracks[1].track_sid);
    for track in tracks {
        let LatestFrame::Available(frame) = frames.latest(&track.track_sid) else {
            panic!("missing image")
        };
        assert_eq!((frame.width, frame.height), (64, 32));
        assert!(frame.captured_at_unix_ns > 0);
        assert!(frame.jpeg.starts_with(&[0xff, 0xd8]));
    }
    let removed_sid = publications[0].sid();
    room.local_participant()
        .unpublish_track(&removed_sid)
        .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !matches!(
            frames.latest(&removed_sid.to_string()),
            LatestFrame::UnknownTrack
        ) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    assert_eq!(frames.tracks().len(), 1);
    stopped.cancel();
    tokio::time::timeout(Duration::from_secs(10), subscriber).await???;
    assert_eq!(frames.tracks(), Vec::new());
    room.close().await?;
    Ok(())
}
