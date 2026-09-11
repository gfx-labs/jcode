# Jcode for Android

A native Kotlin / Jetpack Compose companion for the Jcode daemon. Android 8.0 or newer. No cloud account, embedded model credentials, or separate bridge service is required.

## What it does

- Lists live sessions across projects, including sessions outside Git repositories. Each card shows its working directory, with the full selectable path in session details.
- Orders sessions by most recent actual activity, not by name, status or parent grouping. Stale and unknown-activity sessions sort last.
- Shows running/ready/failed state, current work, todo progress, model, and agent lineage.
- Opens parent or subagent conversation output and tool details. Threads open at the newest content at the bottom while retaining chronological reading order. Scrolling up pauses automatic following; the down-arrow returns to the latest messages.
- Sends a message to one session, or explicitly selected recipients in a confirmed broadcast. Each recipient has its own accepted/rejected/unknown result.
- Uses a compact phone layout and expanded list/detail layout, plus AndroidX WindowManager hinge and tabletop information.
- Keeps the terminal clients attached. Monitoring never subscribes as a session owner, resumes a session, starts a new agent, or takes over a terminal.

Snapshots refresh every three seconds while the app is foregrounded. This is a live **polling monitor**, not a background notification service. Persisted conversation history can lag in-progress text, which is separately shown when the daemon provides an output tail. Only sessions in the connected daemon are listed, not other daemons or archived sessions.

## Install and pair

1. Install `app/build/outputs/apk/debug/app-debug.apk` on your Android device. Android will ask you to allow installation from the app used to open the APK. The development APK is debug-signed, not a Play Store release.
2. Run the Jcode daemon built from this checkout. Older daemons do not implement the observer protocol, even if they already support the iOS gateway. Building a binary is not enough: update/reload the daemon serving your existing sessions.
3. In Jcode, run `/remote on`, then `/remote pair`. `/remote status` displays the address and paired devices. `jcode pair` is also available from the CLI. Gateway configuration changes may require a daemon reload.
4. In the Android connection screen, enter the host and six-digit code. A bare host defaults to port `7643`. `host:port`, `http://host:port`, and `https://host:port` are supported. Explicit HTTP(S) URLs retain their default ports.
5. Connect through a private encrypted VPN or a TLS reverse proxy. The stock gateway serves HTTP/WebSocket, not TLS. Do not expose it on the public internet. Ordinary LAN HTTP is not encrypted, and the pairing code and subsequent bearer token travel over that connection.

The phone must be able to reach the computer. `localhost` on the phone refers to the phone, not your computer. For an emulator, the host alias is `10.0.2.2`. Ensure any host firewall permits the chosen private interface and port only.

Pairing grants control of all sessions on that daemon, not read-only access. To revoke a lost device, use `/remote revoke <device-name-or-id>` on the computer. Forgetting a connection in the app removes its locally stored credential but does not revoke the server's record.

## Build

Install Java 17, Android command-line tools, Android platform 35 and build-tools 35.0.0. Accept the Android SDK license terms for your installation, then:

```sh
export JAVA_HOME=/path/to/jdk-17
export ANDROID_HOME=/path/to/android-sdk
cd android
./gradlew assembleDebug testDebugUnitTest lintDebug
```

The checked-in Gradle wrapper downloads Gradle 8.9 and verifies its distribution checksum. Maven dependencies are resolved from Google's and Maven Central's repositories.

APK: `app/build/outputs/apk/debug/app-debug.apk`

On the development machine used for the first APK:

```sh
export JAVA_HOME="$HOME/.local/share/mise/installs/java/temurin-17"
export ANDROID_HOME="$HOME/.jcode/scratch/android-tooling/sdk"
cd android
./gradlew assembleDebug testDebugUnitTest lintDebug
```

For a distributable production release, configure your own signing key and build a release APK/AAB. Do not check signing keys, passwords, tokens, `local.properties`, build outputs, or SDK installations into Git.

## Safety and delivery semantics

Credentials are AES-GCM encrypted using an Android Keystore key. Backups are disabled. Tokens are sent in the WebSocket Authorization header, not URL query strings. Redirects and automatic request retries are disabled.

Broadcasting is a set of individual deliveries, not an atomic transaction. Accepted means the daemon accepted the message for a target, not that an agent completed the requested work. A timeout or connection loss after send can leave the result unknown. The app never automatically resends a message, because doing so could duplicate agent work. Check the session before retrying an unknown result.

Demo mode is explicitly selected and visibly labeled. Its sample sessions are not real work and cannot send messages to your daemon.

## Architecture

```
app/src/main/java/dev/jcode/mobile/
  MainActivity.kt       activity, lifecycle, window posture
  ui/                   Compose screens and visual tokens
  data/
    MobileViewModel.kt  foreground polling, selection, delivery state
    MobileState.kt      immutable UI models
    GatewayClient.kt    pairing and authenticated one-shot WebSocket RPCs
    WireCodec.kt        protocol decoding and pure state reducer
    CredentialStore.kt  Android Keystore credential storage
```

The observer protocol is additive to the existing gateway:

- `list_sessions {id}` → `sessions_list {id, sessions, server_name}`
- `comm_read_context {id, session_id, target_session}` → `comm_context_history`; the mobile app uses the same target for both session fields and does not claim an agent identity.
- `mobile_message {id, session_id, content}` → `mobile_delivery {id, session_id, status, message}`

Each lightweight request uses its own authenticated socket. These requests do not initialize a provisional agent. The server owns busy-turn injection and idle-session wake-up. Existing terminal subscriptions remain intact.

## Verification

- Android codec, state reducer, and address tests: `./gradlew testDebugUnitTest`
- Android packaging/static checks: `./gradlew assembleDebug lintDebug`
- Real server/gateway test, from repository root: `cargo test --profile selfdev -p jcode --test e2e mobile_observer -- --test-threads=1`
- UI validation should cover folded phone, unfolded list/detail, tabletop, keyboard, large font, reconnect, empty list, agent selection, and broadcast confirmation. An emulator run is not physical hinge/device certification.

See `VALIDATION.md` for observed results and remaining device-validation boundaries for this build.
