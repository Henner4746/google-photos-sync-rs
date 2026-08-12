# Google OAuth production checklist

The application is ready to embed a desktop OAuth client at release build time through the `GPHOTOS_SYNC_OAUTH_CLIENT_JSON` secret. The JSON is compiled into the executable and is never committed to the repository.

Exact public URLs, scope justifications, the in-product disclosure, and the verification-video outline are maintained in [GOOGLE_OAUTH_VERIFICATION.md](GOOGLE_OAUTH_VERIFICATION.md).

Before publishing OAuth access to everyone:

1. Complete the verified custom-domain steps in [CUSTOM_DOMAIN.md](CUSTOM_DOMAIN.md).
2. Create a production Google Cloud project and enable the Photos Library API.
3. Configure the OAuth consent screen with the custom-domain homepage, privacy, and terms URLs.
4. Add a Desktop app OAuth client and store its complete JSON as the GitHub Actions secret `GPHOTOS_SYNC_OAUTH_CLIENT_JSON`.
5. Declare the two exact scopes used by the app: `photoslibrary.appendonly` and `photoslibrary.readonly.appcreateddata`.
6. Move the audience from Testing to Production and submit the requested verification material to Google.
7. Record the verification video described in [GOOGLE_OAUTH_VERIFICATION.md](GOOGLE_OAUTH_VERIFICATION.md).
8. Keep domain ownership, homepage, privacy statement, product name, and consent-screen branding consistent.

Without the release secret, the first-run assistant safely falls back to a file picker for a user's own Desktop OAuth JSON. This is useful for development but is not the intended public onboarding path.
