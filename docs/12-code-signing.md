# 12 · Code signing (Windows)

Status 2026-09-27: **not signed** (Q11, D-059). The user asked for SmartScreen to be fixed on
2026-09-27; the facts below were re-checked that day against Microsoft's own pages (D-294,
S-100–S-104) and the choice, the purchase and the identity check are the user's. Every fresh
install shows Windows SmartScreen's "Windows protected your PC" warning until the installer
and the executable carry an Authenticode signature with enough reputation; nothing here is
wired yet.

## What signing changes

- The NSIS installer, `dayz-launcher.exe` and the uninstaller get an Authenticode signature;
  Windows shows the validated publisher instead of "Unknown publisher" — for a certificate
  issued to an individual, that is the person's legal name (S-104).
- SmartScreen weighs two things: the publisher's certificate and the file's hash (S-100).
  **No certificate skips the warning any more.** An EV certificate used to start with
  reputation; Microsoft's page now says "EV certificates no longer bypass SmartScreen" and
  lists OV and EV alike as "flagged as unrecognized until reputation accumulates", and
  Artifact Signing's reputation "accumulates over time" too (S-100, S-102). This page said
  until D-294 that EV and Trusted Signing skip it.
- What signing buys is continuity: reputation built by one certificate covers later files
  signed with it. Unsigned, "reputation must build for each new version … starting with zero
  reputation" (S-100) — at this project's release rate, the warning never goes away.
- Reputation grows from downloads only ("several weeks and hundreds of clean installs from a
  wide audience"); there is no way to submit a file for SmartScreen review on consumer
  machines. A Microsoft Defender *detection* — a virus verdict, not the SmartScreen prompt — is
  disputed through the Microsoft Security Intelligence file submission
  (microsoft.com/wdsi/filesubmission), which the Artifact Signing FAQ also points to for
  prompts that persist on signed files (S-100, S-102).
- On Windows 11, Smart App Control, where it is on, blocks unsigned executables without a
  positive reputation, downloaded or not (S-100).
- Only the Microsoft Store avoids the warning outright: Microsoft re-signs Store packages
  (S-100). For this app that means an MSIX build without its own updater; not planned.
- The updater's minisign signature (docs/09 D-055) is independent and stays as it is.

## Options

Checked 2026-09-27. Each release signs about three files (the executable, the uninstaller and
the setup).

| Option | Cost | Who can get it | Key and CI | Publisher shown |
|---|---|---|---|---|
| **Artifact Signing** (Microsoft; "Trusted Signing" until 2025) | From 9.99 USD/month (S-100); a paid Azure subscription, never a free or trial one (S-102) | Organisations in the US, Canada, the EU, the UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway and Israel; **individual developers only in the US and Canada** (S-101). Validation takes 1–20 business days | Keys never leave Microsoft; signs from GitHub Actions (Tauri documents `artifact-signing-cli` as `signCommand`, S-103) | The organisation's validated name |
| **IV certificate** from a CA, e.g. SSL.com | 129 USD/year, plus cloud signing from 20 USD/month for 20 signatures (1 USD each after) or a 379 USD FIPS token (S-104) | Individuals, no business needed; government ID; 3–5 days (S-104) | Hardware only since the CA/Browser Forum's June 2023 rule: a USB token, or the CA's cloud signing, which works from CI (S-104) | The individual's legal name |
| OV certificate | Similar to IV or more | Registered organisations | The same hardware rule | The organisation's name |
| EV certificate | More than OV | Registered organisations | The same hardware rule | No SmartScreen advantage any more (S-100) |
| Free signing for open-source projects (SignPath Foundation) | Free | Projects under an OSI-approved licence with no proprietary part (S-105) | — | **Not available**: this project is all rights reserved (D-293) |

Recommendation (D-294): for the user as a UK individual, an IV certificate with the CA's cloud
signing, which the release workflow can call; Artifact Signing instead if the launcher is ever
published by a registered company, which is also the only way to show a company name rather
than a legal name. Neither removes the first-download warning at once; both let it fade and
stay gone across releases.

## Wiring it into the build

Tauri 2 signs Windows bundles itself when one of these is configured in `src-tauri/tauri.conf.json`
(`bundle.windows`, keys verified against the CLI's `config.schema.json`, S-63):

- `certificateThumbprint` (SHA-1 of a certificate in the current user's store), `digestAlgorithm`
  (`sha256`), `timestampUrl` (the CA's RFC 3161 server), `tsp` (`true` for RFC 3161).
- or `signCommand`: a custom command Tauri runs for every binary, with `%1` replaced by the file
  path. This is the route for Artifact Signing, for a CA's cloud signing and for hardware
  tokens. Tauri's guide (S-103) gives `artifact-signing-cli -e https://wus2.codesigning.azure.net
  -a MyAccount -c MyProfile -d MyApp %1` for Artifact Signing, and keeps the thumbprint route
  for OV certificates issued before June 2023, whose keys could still be exported to a PFX.

Example for a certificate installed in the user store (a pre-June-2023 PFX or a token):

```json
"windows": {
  "certificateThumbprint": "<40 hex chars>",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com",
  "tsp": true
}
```

Example for Artifact Signing through Microsoft's `signtool` dlib, the older route (adapt paths;
S-103's `artifact-signing-cli` line above is the one Tauri documents now):

```json
"windows": {
  "signCommand": {
    "cmd": "signtool",
    "args": ["sign", "/v", "/fd", "SHA256", "/tr", "http://timestamp.acs.microsoft.com", "/td", "SHA256",
             "/dlib", "C:\\tools\\Microsoft.Trusted.Signing.Client\\bin\\x64\\Azure.CodeSigning.Dlib.dll",
             "/dmdf", "C:\\tools\\trusted-signing-metadata.json", "%1"]
  }
}
```

`metadata.json` names the Artifact Signing endpoint, account and certificate profile; the
signing machine authenticates to Azure with an identity that has the "Artifact Signing
Certificate Profile Signer" role (S-102; it was "Trusted Signing Certificate Profile Signer").

## In the release workflow

`.github/workflows/release.yml` builds on `windows-latest`, so signing has to work there:

- Certificate file (only a certificate issued before June 2023; newer keys cannot leave their
  hardware): store the PFX as a base64 secret plus its password; a step before `tauri-action`
  writes and imports it (`Import-PfxCertificate` into `Cert:\CurrentUser\My`), and
  `certificateThumbprint` is set through `--config` so the value is not committed.
- A CA's cloud signing (an IV or OV certificate bought now): the CA's signing tool runs as
  `signCommand`, with its credentials in repository secrets the user sets; follow the CA's
  current CI guide.
- Artifact Signing: log in with `azure/login` (OIDC federated credential, no stored secret),
  install the signing client, then let `signCommand` run.
- Keep the smoke-install and the `verify_update_sig.js` steps: a signed installer must still
  install silently and the updater manifest is unaffected.

## Verifying a signed build

```powershell
Get-AuthenticodeSignature "src-tauri\target\release\bundle\nsis\DZSA CrayZ Launcher_<version>_x64-setup.exe" | Format-List Status, SignerCertificate, TimeStamperCertificate
```

`Status` must be `Valid` and the timestamp present, otherwise the signature expires with the
certificate.

## Checklist when a certificate arrives

1. Decide Artifact Signing vs. a CA certificate with cloud signing (D-294 recommends the
   second for an individual in the UK); record it in docs/09.
2. Add the `bundle.windows` signing keys (or `signCommand`) and build once locally; verify with
   `Get-AuthenticodeSignature`.
3. **Check that the uninstaller is signed too.** The NSIS template takes `UNINSTALLERSIGNCOMMAND`
   separately, and an unsigned `uninstall.exe` in the install folder would be the binary Windows
   runs from Add/Remove Programs and the one an upgrade runs mid-install. The CLI (2.11.5) has no
   setting for it — `bundle.windows.nsis` has no `uninstallerSignCommand` key; it fills the
   template's `{{uninstaller_sign_cmd}}` itself, and its message "Skipping signing for NSIS
   uninstaller due to --no-sign flag" says it signs the uninstaller with the configured signing
   unless told not to. Confirm on the first signed build: verify the shipped `uninstall.exe` with
   `Get-AuthenticodeSignature`, not just the setup exe (D-214, D-287).
4. Add the workflow step and secrets; dry-run the workflow (build only) and check the artifact's
   signature.
5. Tag a release; confirm SmartScreen behaviour on a clean machine.
6. Close Q11 in docs/10.
