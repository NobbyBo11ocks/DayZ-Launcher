# 12 · Code signing (Windows)

Status 2026-09-21: **not signed** (Q11, D-059). Every fresh install shows Windows SmartScreen's
"Windows protected your PC" warning until the installer and the executable carry an
Authenticode signature with enough reputation. This page is the plan for when a certificate
exists; nothing here is wired yet.

## What signing changes

- The NSIS installer and `dayz-launcher.exe` get an Authenticode signature; Windows shows the
  publisher name instead of "Unknown publisher".
- SmartScreen reputation is built per certificate (and per file hash). A new OV certificate
  still triggers the warning for a while; an EV certificate and Azure Trusted Signing start
  with reputation and skip it.
- The updater's minisign signature (docs/09 D-055) is independent and stays as it is.

## Options

| Option | Cost (2026, approx.) | Identity check | SmartScreen | Notes |
|---|---|---|---|---|
| Azure Trusted Signing | ~10 USD/month (Basic) | Organisation or individual validation by Microsoft | Immediate reputation | Short-lived certs issued per signing; signing happens through Microsoft's `signtool` dlib; needs an Azure subscription and a validated identity |
| OV certificate (Sectigo, DigiCert, SSL.com, …) | 200–400 USD/year | Organisation validation | Reputation builds over weeks of downloads | Private key can be a PFX file or a hardware token |
| EV certificate | 300–700 USD/year | Extended validation | Immediate reputation | Hardware token or cloud HSM required since 2023 |

Recommendation: Azure Trusted Signing if the identity validation is possible; otherwise an OV
certificate and patience.

## Wiring it into the build

Tauri 2 signs Windows bundles itself when one of these is configured in `src-tauri/tauri.conf.json`
(`bundle.windows`, keys verified against the CLI's `config.schema.json`, S-63):

- `certificateThumbprint` (SHA-1 of a certificate in the current user's store), `digestAlgorithm`
  (`sha256`), `timestampUrl` (the CA's RFC 3161 server), `tsp` (`true` for RFC 3161).
- or `signCommand`: a custom command Tauri runs for every binary, with `%1` replaced by the file
  path. This is the route for Azure Trusted Signing (`signtool … /dlib Azure.CodeSigning.Dlib.dll
  /dmdf metadata.json %1`) and for hardware tokens.

Example for a certificate installed in the user store:

```json
"windows": {
  "certificateThumbprint": "<40 hex chars>",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com",
  "tsp": true
}
```

Example for Azure Trusted Signing (adapt paths; check Tauri's current Windows signing guide before use):

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

`metadata.json` names the Trusted Signing endpoint, account and certificate profile; the
signing machine authenticates to Azure with an identity that has the "Trusted Signing
Certificate Profile Signer" role.

## In the release workflow

`.github/workflows/release.yml` builds on `windows-latest`, so signing has to work there:

- Certificate file: store the PFX as a base64 secret plus its password; a step before
  `tauri-action` writes and imports it (`Import-PfxCertificate` into `Cert:\CurrentUser\My`),
  and `certificateThumbprint` is set through `--config` so the value is not committed.
- Azure Trusted Signing: log in with `azure/login` (OIDC federated credential, no stored
  secret), install the Trusted Signing client (`dotnet tool install --global
  Microsoft.Trusted.Signing.Client` or the official action), then let `signCommand` run.
- Keep the smoke-install and the `verify_update_sig.js` steps: a signed installer must still
  install silently and the updater manifest is unaffected.

## Verifying a signed build

```powershell
Get-AuthenticodeSignature "src-tauri\target\release\bundle\nsis\DZSA CrayZ Launcher_<version>_x64-setup.exe" | Format-List Status, SignerCertificate, TimeStamperCertificate
```

`Status` must be `Valid` and the timestamp present, otherwise the signature expires with the
certificate.

## Checklist when a certificate arrives

1. Decide Trusted Signing vs. file certificate; record it in docs/09.
2. Add the `bundle.windows` signing keys (or `signCommand`) and build once locally; verify with
   `Get-AuthenticodeSignature`.
3. Add the workflow step and secrets; dry-run the workflow (build only) and check the artifact's
   signature.
4. Tag a release; confirm SmartScreen behaviour on a clean machine.
5. Close Q11 in docs/10.
