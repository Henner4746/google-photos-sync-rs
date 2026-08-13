# Code signing policy

Supported Windows releases of Google Photos Sync, beginning with v2, are built from the public source repository by GitHub Actions. Release artifacts are accepted only after their Authenticode signatures and SHA-256 checksums have been verified by the release workflow. The older v1.1.0 development binary is unsigned and is not a supported public installer.

The in-app updater independently repeats these checks on the user's PC. It accepts an executable only when the GitHub release digest matches, Windows trusts its Authenticode chain, and its publisher matches the currently installed executable. The helper verifies both signatures and the publisher again immediately before replacing the running app.

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

## Team roles

- Committer and reviewer: [Henner4746](https://github.com/Henner4746)
- Release approver: [Henner4746](https://github.com/Henner4746)
- Multi-factor authentication is required for every repository and signing role.

Changes from outside contributors require maintainer review before merging. Every SignPath signing request requires an explicit release approval. Only artifacts built by the repository's public GitHub Actions workflow from this project's source code may be signed.

## Privacy and system changes

The app transfers selected media only when the user explicitly configures Google Photos synchronization. It has no telemetry, advertising, or own backend. See the [privacy policy](https://henner4746.github.io/google-photos-sync-rs/privacy.html).

The installer announces its installation path and autostart behavior, provides a normal Windows uninstaller, and preserves user data during uninstall unless the user removes it separately.

## Verification

Once signed v2 releases are available, users should download them only from the [official GitHub Releases page](https://github.com/Henner4746/google-photos-sync-rs/releases). Supported releases contain SHA-256 checksums. Windows signatures can be inspected in file properties or with:

```powershell
Get-AuthenticodeSignature .\Google-Photos-Sync-Setup.exe
```
