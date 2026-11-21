# Browser camera integration

The browser page connects directly to your LiveKit server. Your application owns
token issuance, identity, room access and expiry. For general authentication and
publishing behavior, use LiveKit's [token reference](https://docs.livekit.io/frontends/reference/tokens-grants/)
and [camera publishing guide](https://docs.livekit.io/transport/media/publish/).

## Invite URL

Any backend can construct this interface; TypeScript is not required:

```text
https://your-camera-page.example/#url=<encoded-livekit-url>&token=<encoded-jwt>
```

URL-encode the fragment values. A camera invite opens **Stream** and must permit
room join and camera publication. The example disables subscription and data
publishing. A viewer invite adds `view=room`, opens **Room viewer**, and needs
subscription permission. Both invite types populate **Setup**. The token's
participant identity names the camera. Give each camera a distinct identity.

Use a token with both grants to share one connection setting between pages. To
stream and view at the same time from this app, use a separate viewer token
with a distinct participant identity. Saving updated credentials ends any
connection that used the credentials you changed.

The page displays decoded identity/expiry information; LiveKit verifies access.
API secrets belong on your trusted backend, never in the page or invite.

## Recording indicator

Set room metadata to `{"recording": true}` to display the recording indicator and
lock orientation, flip, lens and zoom. Set it to `false` or omit it to unlock.
This convention does not detect Egress recordings or enforce access control.
Other metadata fields are ignored.

## Token and camera handling

- Treat invite URLs and QR codes as bearer credentials. The app removes the
  credential fragment from the address bar after loading the invite. The browser
  sends the page's HTTP request without the fragment.
- Manually entered tokens stay in memory. Lens selection is stored locally.
  The app includes no analytics, third-party runtime scripts or microphone capture.
- The hosting operator's JavaScript can read the token. Trust the host or deploy
  your own copy. Video goes through the configured LiveKit service.
- Local device selection is part of the Stream view. A camera used exclusively by
  another application cannot be inspected through Room viewer until it publishes
  to LiveKit; opening that device again in a browser may conflict with capture.
- Keep mobile devices unlocked and the page visible; background capture can be
  suspended. Lens, zoom and wake-lock availability depend on the browser/device.

Return to the [quick start](../README.md#browser-camera).
