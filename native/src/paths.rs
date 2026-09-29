//! Filesystem locations. Nothing here is ever derived from message content, so
//! the extension cannot influence which files the host touches (no path traversal).
//!
//! Installed layout (per-user, no admin):
//!   %LOCALAPPDATA%\LegalSearch\bin\LegalSearch.exe
//!   %LOCALAPPDATA%\LegalSearch\model\{model.onnx,tokenizer.json}
//!   %LOCALAPPDATA%\LegalSearch\data\index.sqlite      (regenerable cache)
//!   %LOCALAPPDATA%\LegalSearch\logs\legalsearch.log   (rotated)
//!
//! Developer/test overrides: LEGALSEARCH_MODEL_DIR, LEGALSEARCH_DATA_DIR.

use std::path::PathBuf;

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

/// Application root: parent of `bin\`.
pub fn app_root() -> Option<PathBuf> {
    exe_dir()?.parent().map(|p| p.to_path_buf())
}

/// Absolute path of onnxruntime.dll. Loaded by absolute path only (never via the
/// DLL search order), from the folder that contains LegalSearch.exe.
pub fn ort_dll() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LEGALSEARCH_ORT_DLL") {
        return Some(PathBuf::from(p));
    }
    let installed = exe_dir()?.join("onnxruntime.dll");
    if installed.is_file() {
        return Some(installed);
    }
    // Development builds and `cargo test`: native/runtime (scripts/fetch-runtime.ps1).
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime").join("onnxruntime.dll");
        if dev.is_file() {
            return Some(dev);
        }
    }
    Some(installed)
}

pub fn model_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGALSEARCH_MODEL_DIR") {
        return Some(PathBuf::from(d));
    }
    Some(app_root()?.join("model"))
}

fn local_root() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(d).join("LegalSearch"));
    }
    app_root()
}

pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGALSEARCH_DATA_DIR") {
        return Some(PathBuf::from(d));
    }
    Some(local_root()?.join("data"))
}

pub fn log_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGALSEARCH_DATA_DIR") {
        return Some(PathBuf::from(d).join("logs"));
    }
    Some(local_root()?.join("logs"))
}
