# Security policy

Do not report vulnerabilities in a public issue. Please use
[GitHub's private vulnerability reporting](https://github.com/Hebbian-Robotics/livekit-cameras/security/advisories/new)
and include the affected revision, steps to reproduce, and potential impact.
Only the latest commit on the default branch receives security fixes.

Camera invite links contain a bearer token. Anyone who has a link can use its
token while it is valid, so do not share links or QR codes publicly. Never put
LiveKit API keys or secrets in the browser page or an invite URL. Use a trusted
host for the page: its JavaScript can read the token. See the
[privacy and access guidance](README.md#privacy-and-access) for details.
