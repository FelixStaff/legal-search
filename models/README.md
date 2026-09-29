# Models

Model files are **not versioned in Git** (118 MB). They are downloaded **at build time**, never at install time:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\fetch-model.ps1            # e5-small (default)
powershell -ExecutionPolicy Bypass -File scripts\fetch-model.ps1 -Model base # e5-base (larger)
```

Result in `models/current/`:

| File | Source | SHA-256 (verified by the script) |
|---|---|---|
| `model.onnx` | `Xenova/multilingual-e5-small` → `onnx/model_quantized.onnx` (INT8 dynamic) | `f80102d3…c98c193` |
| `tokenizer.json` | `Xenova/multilingual-e5-small` | `0b44a9d7…c4bf39` |
| `model_id.txt` | written by the script | part of the cache key |

Original model: [intfloat/multilingual-e5-small](https://huggingface.co/intfloat/multilingual-e5-small), MIT license.
The installer packages `models/current` into `%LOCALAPPDATA%\LegalSearch\model`.
Nothing is downloaded at usage time.
