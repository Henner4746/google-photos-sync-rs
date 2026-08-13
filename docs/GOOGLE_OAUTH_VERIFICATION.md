# Google OAuth production verification handoff

This repository is prepared for a separate production Google Cloud project. The project owner must create and submit it because Google requires account ownership, contact details, Search Console domain verification, declarations, and acceptance of platform terms.

## Public identity

- App name: `Google Photos Sync`
- Production homepage: `https://photos-sync.henrik.click/`
- Production privacy policy: `https://photos-sync.henrik.click/privacy.html`
- Production terms of service: `https://photos-sync.henrik.click/terms.html`
- Production data management and deletion: `https://photos-sync.henrik.click/data-management.html`
- Current staging site: <https://henner4746.github.io/google-photos-sync-rs/>
- Source: <https://github.com/Henner4746/google-photos-sync-rs>

Google requires a domain the project owner can verify. `github.io` belongs to GitHub and must not be submitted as the production authorized domain. Complete [CUSTOM_DOMAIN.md](CUSTOM_DOMAIN.md), then use `photos-sync.henrik.click` in OAuth Branding and verify `henrik.click` in Google Search Console with the same Google account that owns or edits the Cloud project.

Use a dedicated production project, enable the Google Photos Library API, configure an External audience, publish the branding configuration, and create a **Desktop app** OAuth client.

## Exact scopes and justifications

### `https://www.googleapis.com/auth/photoslibrary.appendonly`

The user explicitly selects local image and video folders in the first-run UI. The app uploads only new media from those folders to the user's own Google Photos account. It cannot modify or delete existing Google Photos media and does not operate a proxy server.

The desktop authorization flow opens the system browser and returns through a random `127.0.0.1` loopback port. Every request uses an independent state value and PKCE S256 verifier; the verifier is included only during the code exchange. An abandoned browser flow times out without leaving the app permanently busy.

### `https://www.googleapis.com/auth/photoslibrary.readonly.appcreateddata`

The app reads metadata only for Google Photos media created by this same application. This narrow access is used to reconcile confirmed uploads after a local database loss or interrupted request and to prevent repeated uploads. It cannot enumerate the user's wider Google Photos library.

## In-product disclosure

Immediately before opening Google's authorization page, the first-run screen states:

- selected new media goes directly to the user's Google Photos account;
- the app can read only media it created and cannot see other existing Google Photos media;
- OAuth credentials remain DPAPI-encrypted on the local Windows PC;
- the app has no own server;
- clicking `Verstanden · Mit Google verbinden` is the affirmative consent action.

Settings provide `Google trennen`. After confirmation, the app calls Google's token revocation endpoint and deletes the local encrypted credential. It never deletes the user's photos.

## Verification video outline

Record one continuous, unedited video with non-personal sample media:

1. Show the public homepage, privacy policy, terms, and data-management page.
2. Start a clean installation and show the complete in-product disclosure.
3. Click `Verstanden · Mit Google verbinden` and show the OAuth consent screen, app name, and both requested scopes.
4. Complete the local loopback callback and show `Google verbunden`.
5. Select a sample folder, explain autostart, and finish setup.
6. Run a test sync, then a real upload of one sample image and show the confirmed result in Google Photos.
7. Run the same sync again and show that the known file is skipped.
8. Show the March 2025 API limitation, the recommended local Takeout import, and the upload block shown when neither Takeout nor an explicit no-older-copies confirmation exists.
9. Open Settings, choose `Google trennen`, confirm, and show that the setup screen returns while the uploaded sample remains in Google Photos.

Do not show OAuth JSON, client secrets, refresh tokens, private media, personal folder paths, browser cookies, or unrelated Google account data.

## Submission checklist

1. Add owner/editor and developer contact emails that are actively monitored.
2. Verify the homepage domain in Search Console.
3. Enter the homepage, privacy, and terms URLs in OAuth Branding.
4. Declare only the two scopes above in Data Access.
5. Create the Desktop app client and download its JSON.
6. Submit the scope justifications and video through the OAuth Verification Center.
7. After approval, store the complete Desktop client JSON as the GitHub Actions secret `GPHOTOS_SYNC_OAUTH_CLIENT_JSON`.
8. Never commit the OAuth JSON or token credentials to the repository.
