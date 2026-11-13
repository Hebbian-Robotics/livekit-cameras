# Browser camera invites

`livekit-camera-invite` builds a browser camera page link and a QR code from an
already-issued LiveKit token. It does not issue tokens or choose permissions.

```rust
let invite = livekit_camera_invite::camera_invite(
    "https://hebbian-robotics.github.io/livekit-cameras/",
    "wss://your-livekit-server.example",
    "your-publish-token",
)?;
// invite.url opens the browser publisher; invite.qr_code_image is an SVG data URL.
# Ok::<(), livekit_camera_invite::InviteError>(())
```

The page reads `url` and `token` from the URL fragment. Existing query parameters
are preserved and any old fragment is replaced. Both returned fields contain the
bearer token: keep them private and avoid logging them. The caller owns token
issuance, expiry, identity and permissions. For camera publishing, issue a
room-scoped, camera-only publish token with subscription and data publishing disabled.
