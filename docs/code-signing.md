# Code signing

```text
CODE_SIGNING = NOT_CONFIGURED
```

LegalSearch 1.1.0 RC2 is distributed **unsigned**. This page describes what should be signed, where signing belongs
in the release pipeline, and the rules for handling signing material.

## What must be signed

| Binary | Current state | Notes |
|---|---|---|
| `LegalSearch.exe` (engine) | Not signed | Launched by Chrome through Native Messaging. Sign after `cargo build --release`, **before** it is packaged into the installer. |
| `LegalSearchSetup.exe` (installer) | Not signed | What users download and run. |
| `unins000.exe` (uninstaller, written by the installer) | Not signed | Signed by Inno Setup itself when `SignTool` and `SignedUninstaller=yes` are configured. |
| `onnxruntime.dll` | Signed by Microsoft Corporation | Official ONNX Runtime release; do not re-sign. |
| `msvcp140*.dll`, `vcruntime140*.dll` | Signed by Microsoft | Visual C++ redistributable; do not re-sign. |

The Chrome extension is not Authenticode-signed; the Chrome Web Store signs published extensions.

## Where signing belongs in the pipeline

```
cargo build --release
      ↓
sign native/target/release/LegalSearch.exe        ← 1
      ↓
stage files (scripts/build-all.ps1)
      ↓
ISCC installer/LegalSearch.iss                    ← 2  SignTool=... / SignedUninstaller=yes
      ↓                                                (signs LegalSearchSetup.exe and unins000.exe)
verify: Get-AuthenticodeSignature → Valid
      ↓
generate dist/SHA256SUMS.txt                      ← checksums must be computed AFTER signing
      ↓
upload release assets
```

`.github/workflows/release.yml` contains a marked placeholder ("Code signing hook") at the point where step 2 and
the checksum regeneration belong. Use RFC 3161 timestamping (`/tr <timestamp URL> /td sha256 /fd sha256` with
`signtool`) so signatures stay valid after the certificate expires.

## Handling signing material

- The private key / certificate file (`.pfx`, `.p12`, `.pem`, `.key`, ...) must **never** be committed. `.gitignore`
  excludes these extensions, but do not rely on that: keep signing material outside the repository.
- Certificate passwords and key references must come from **GitHub Secrets** (or an equivalent secret store) and be
  passed to the signing step as environment variables. Never print them in logs.
- Prefer cloud or hardware-backed keys (HSM, key vault, signing service) so the private key never exists as a file
  on a build runner.
- Restrict who can trigger the release workflow; signing secrets should be available only to that workflow
  (use a protected GitHub *environment* with required reviewers).

## Options (not configured; the owner must choose)

- A standard (OV) code-signing certificate from a public certificate authority, used through `signtool`.
- A managed signing service with keys held in the cloud (for example Microsoft Azure Trusted Signing).
- Free signing programs for open-source projects (for example SignPath Foundation), subject to their eligibility
  rules.

Most options have a cost or an approval process; none is required to build LegalSearch.

## Current implications of shipping unsigned

- **SmartScreen**: Windows shows "Windows protected your PC" when the installer is run. Users must click
  **More info → Run anyway**. The warning disappears only once signed binaries accumulate reputation.
- **Smart App Control** (Windows 11): when it is *enforced*, it can block unsigned applications without reputation
  from running at all, including `LegalSearch.exe` when Chrome launches it. It was observed blocking freshly built,
  unsigned test binaries on the development machine. On such computers the side panel reports that the local engine
  is not installed.
- **Integrity**: without a signature, users can only verify downloads through `SHA256SUMS.txt` published next to the
  release assets.
