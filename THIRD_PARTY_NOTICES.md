# Third-party notices

LegalSearch's own source code is licensed under the [MIT License](LICENSE). It distributes, or is built with, the
third-party components below. All of them run locally; none of them contacts a server during use.

Licenses were taken from each project's own metadata or license files (Hugging Face model cards, `Cargo.toml`
metadata of the exact crate versions in `native/Cargo.lock`, `package.json` of the exact npm versions in
`extension/package-lock.json`, and the license files shipped inside the ONNX Runtime and PDF.js distributions).

## Distributed with the installer or the extension

### multilingual-e5-small (embedding model)

| | |
|---|---|
| Project | https://huggingface.co/intfloat/multilingual-e5-small |
| ONNX export used | https://huggingface.co/Xenova/multilingual-e5-small — file `onnx/model_quantized.onnx` (INT8) and `tokenizer.json` |
| Version | Pinned by SHA-256 in `scripts/fetch-model.ps1` (model `f80102d3…c98c193`, tokenizer `0b44a9d7…c4bf39`) |
| License | MIT (declared in the model card metadata of `intfloat/multilingual-e5-small`) |
| Attribution | Liang Wang, Nan Yang, Xiaolong Huang, Linjun Yang, Rangan Majumder, Furu Wei — *Multilingual E5 Text Embeddings: A Technical Report*, arXiv:2402.05672 (2024). The model card publishes no separate copyright line. |
| Use in LegalSearch | Computes the passage and query embeddings, locally, on the CPU. |

Notes:
- The `Xenova/multilingual-e5-small` repository is a format conversion (ONNX weights) of the MIT-licensed model above
  and declares no license of its own; LegalSearch treats the MIT terms of the original model as governing.
- The tokenizer derives from the XLM-RoBERTa SentencePiece vocabulary (`FacebookAI/xlm-roberta-base`, MIT).
- The installer ships the MIT license text for the model in `licenses/multilingual-e5-small-LICENSE.txt`.

### ONNX Runtime

| | |
|---|---|
| Project | https://github.com/microsoft/onnxruntime |
| Version | 1.28.2, official Windows x64 CPU build (`onnxruntime-win-x64-1.28.2.zip`, SHA-256 pinned in `scripts/fetch-runtime.ps1`) |
| License | MIT — Copyright (c) Microsoft Corporation |
| Use in LegalSearch | Runs the embedding model (`onnxruntime.dll`, loaded by absolute path). |

The installer ships `licenses/LICENSE-onnxruntime.txt` and `licenses/ThirdPartyNotices-onnxruntime.txt`, copied
unmodified from the official release.

### PDF.js

| | |
|---|---|
| Project | https://github.com/mozilla/pdf.js (npm package `pdfjs-dist`) |
| Version | 6.3.289 (legacy build) |
| License | Apache License 2.0 — Copyright Mozilla Foundation |
| Use in LegalSearch | Extracts the text of each PDF page inside the Chrome extension. |

`LICENSE-pdfjs.txt` (unmodified) is included in the extension package and in the installer's `licenses/` folder.
`pdfjs-dist` ships no `NOTICE` file.

### Rust crates linked into `LegalSearch.exe`

33 crates, all under permissive licenses: MIT, Apache-2.0, MIT OR Apache-2.0, ISC (`libloading`), Zlib (`foldhash`),
Unlicense OR MIT (`memchr`). Main ones:

| Crate | Version | License | Use |
|---|---|---|---|
| `ort`, `ort-sys` | 2.0.0-rc.13 | MIT OR Apache-2.0 | Rust bindings to ONNX Runtime |
| `rusqlite`, `libsqlite3-sys` | 0.40.2, 0.38.2 | MIT | Local index cache (SQLite) |
| SQLite (bundled by `libsqlite3-sys`) | — | Public domain | Database engine |
| `spm_precompiled` | 0.1.4 | Apache-2.0 | SentencePiece text normalization |
| `serde`, `serde_json` | 1.0.229, 1.0.151 | MIT OR Apache-2.0 | Message and file parsing |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 | SHA-256 |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 | Grapheme segmentation |

The full list with every license text is generated at build time by `scripts/gen-rust-licenses.mjs` and shipped as
`licenses/THIRD_PARTY_RUST_CRATES.txt`. Build-only and test-only crates (for example `cc`, `syn`, and the
Hugging Face `tokenizers` crate used as a test reference) are not distributed.

### Microsoft Visual C++ Runtime

| | |
|---|---|
| Files | `msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140.dll`, `vcruntime140_1.dll` |
| Version | From the Visual Studio 2026 Build Tools redistributable folder (`Microsoft.VC145.CRT`) |
| License | Microsoft Visual Studio license terms, "Distributable Code" (app-local deployment permitted) |
| Use in LegalSearch | Required by `onnxruntime.dll`. `LegalSearch.exe` itself uses a static C runtime. |

### Inno Setup (installer engine)

| | |
|---|---|
| Project | https://jrsoftware.org/isinfo.php |
| Version | 6.7.3 |
| License | Inno Setup License — Copyright (C) 1997-2026 Jordan Russell, portions Copyright (C) 2000-2026 Martijn Laan |
| Use in LegalSearch | Builds `LegalSearchSetup.exe`. The installer stub retains Inno Setup's own copyright notices as its license requires. |

## Used only to build or test (not distributed)

| Component | Version | License |
|---|---|---|
| esbuild | 0.28.2 | MIT |
| TypeScript | 7.0.2 | Apache-2.0 |
| @types/chrome | 0.3.2 | MIT |
| Hugging Face `tokenizers` (Rust, dev-dependency) | 0.22 | Apache-2.0 |
| Rust toolchain, Cargo | stable | MIT OR Apache-2.0 |
| Chrome for Testing (UI end-to-end tests) | — | Chromium BSD-style licenses |

`@napi-rs/canvas` (MIT) is installed as an optional dependency of `pdfjs-dist` for Node.js and is not included in the
extension bundle.
