# SignPath Foundation application handoff

The repository is prepared for SignPath Foundation's free open-source code-signing program. The repository owner must submit the application because the form includes terms acceptance and an anti-bot check.

Application: <https://signpath.org/apply>

## Suggested project details

- Project name: `Google Photos Sync`
- Source repository: <https://github.com/Henner4746/google-photos-sync-rs>
- Download page: <https://github.com/Henner4746/google-photos-sync-rs/releases>
- Homepage: <https://henner4746.github.io/google-photos-sync-rs/>
- Code signing policy: <https://henner4746.github.io/google-photos-sync-rs/code-signing.html>
- Privacy policy: <https://henner4746.github.io/google-photos-sync-rs/privacy.html>
- License: MIT
- Maintainer: <https://github.com/Henner4746>

Suggested description:

> Google Photos Sync is a native, open-source Rust application for Windows 10 and 11. Users explicitly select local photo and video folders, connect their own Google Photos account, and optionally enable Windows autostart. SHA-256 content identification, a local SQLite index, and an optional local Google Takeout scan prevent repeated uploads. The app has no telemetry, advertising, proprietary backend, bundled third-party binaries, or paid edition. Windows executables and the Inno Setup installer are built automatically and verifiably from the public repository by GitHub-hosted Actions.

Artifacts to sign:

- `gphotos-sync.exe`
- `Google-Photos-Sync-Setup.exe`

The app and installer now contain matching `2.0.0` product/version metadata. The workflow submits each unsigned PE artifact directly from the GitHub-hosted build, waits for manual approval, installs the returned signed artifact, verifies it with `signtool verify /pa`, creates SHA-256 checksums, and only then publishes the release.

## Post-approval values

After SignPath creates the project and artifact configuration, add these repository secrets:

- `SIGNPATH_API_TOKEN`
- `SIGNPATH_ORGANIZATION_ID`
- `SIGNPATH_PROJECT_SLUG`
- `SIGNPATH_SIGNING_POLICY_SLUG`
- `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG`

The artifact configuration should accept a raw Windows PE file and enforce:

- Product name: `Google Photos Sync`
- Product version: release parameter `version`
- File type: Windows executable
- SHA-256 Authenticode digest and trusted timestamp

The release workflow prefers SignPath when all five values are available. A conventional PFX remains a supported fallback.
