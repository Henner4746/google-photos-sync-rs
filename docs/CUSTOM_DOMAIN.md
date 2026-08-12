# Verified production domain handoff

The GitHub Pages staging site is live, but Google OAuth verification requires a domain controlled by the project owner. The reserved production hostname is:

`photos-sync.henrik.click`

It was unassigned in the DNS read check on 12 August 2026. Do not replace or modify any existing `henrik.click` records.

## Cloudflare DNS

Create exactly one record in the `henrik.click` zone:

- Type: `CNAME`
- Name: `photos-sync`
- Target: `henner4746.github.io`
- Proxy status: `DNS only` while GitHub validates the domain and provisions TLS
- TTL: `Auto`

Do not add an `A` or `AAAA` record for the same hostname.

## GitHub Pages

After DNS resolves, add a `docs/CNAME` file containing only `photos-sync.henrik.click`, push it, then open repository settings for `Henner4746/google-photos-sync-rs`, Pages, and set the custom domain to:

`photos-sync.henrik.click`

Wait until GitHub reports the DNS check and certificate as successful, then enable `Enforce HTTPS`. The workflow publishes the complete `docs` directory, including that `CNAME` file.

Verify all of these URLs return HTTPS status 200:

- `https://photos-sync.henrik.click/`
- `https://photos-sync.henrik.click/privacy.html`
- `https://photos-sync.henrik.click/terms.html`
- `https://photos-sync.henrik.click/data-management.html`
- `https://photos-sync.henrik.click/code-signing.html`

## Google Search Console

Add the **Domain property** `henrik.click`. Google will provide a unique TXT value. Add only that exact TXT value to Cloudflare, wait for DNS propagation, and verify ownership. Never commit the verification value to this repository.

## Repository links

After the custom domain is active, replace the absolute staging URLs `https://henner4746.github.io/google-photos-sync-rs/` in README and verification documents with `https://photos-sync.henrik.click/`. Relative links inside the website need no change.

Only then submit OAuth Branding and Data Access for production verification.
