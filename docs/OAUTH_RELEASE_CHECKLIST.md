# Google OAuth production checklist

The application is ready to embed a desktop OAuth client at release build time through the `GPHOTOS_SYNC_OAUTH_CLIENT_JSON` secret. The JSON is compiled into the executable and is never committed to the repository.

Before publishing OAuth access to everyone:

1. Create a production Google Cloud project and enable the Photos Library API.
2. Configure the OAuth consent screen with the public homepage and privacy URLs from GitHub Pages.
3. Add a Desktop app OAuth client and store its complete JSON as the GitHub Actions secret `GPHOTOS_SYNC_OAUTH_CLIENT_JSON`.
4. Declare the two exact scopes used by the app: `photoslibrary.appendonly` and `photoslibrary.readonly.appcreateddata`.
5. Move the audience from Testing to Production and submit the requested verification material to Google.
6. Record a short verification video showing sign-in, folder selection, upload, Takeout explanation, and access revocation.
7. Keep domain ownership, homepage, privacy statement, product name, and consent-screen branding consistent.

Without the release secret, the first-run assistant safely falls back to a file picker for a user's own Desktop OAuth JSON. This is useful for development but is not the intended public onboarding path.
