//! Chrome Native Messaging framing and the LegalSearch message schema.
//!
//! Framing: each message is a UTF-8 JSON document preceded by its length as a
//! 32-bit little-endian unsigned integer (native byte order on x86/x64 Windows).
//! Chrome caps host->extension messages at 1 MB; extension->host at 64 MB.
//! We apply our own, stricter, limit on incoming messages.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Hard cap for a single incoming message. The extension batches page text so
/// real messages stay far below this.
pub const MAX_INCOMING_BYTES: usize = 8 * 1024 * 1024;
/// Chrome refuses host->extension messages above 1 MB.
pub const MAX_OUTGOING_BYTES: usize = 1024 * 1024;
pub const PROTOCOL_VERSION: u32 = 1;

pub const MAX_QUERY_CHARS: usize = 1000;
pub const MAX_PAGE_CHARS: usize = 200_000;
pub const MAX_PAGES: u32 = 20_000;
pub const MAX_TOTAL_CHARS: usize = 60_000_000;
pub const MAX_FILENAME_CHARS: usize = 512;
pub const MAX_K: usize = 50;
pub const DEFAULT_K: usize = 8;

#[derive(Debug)]
pub enum ReadError {
    /// Clean EOF before a new message: Chrome closed the port.
    Eof,
    TooLarge(usize),
    Io(io::Error),
    Json(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Eof => write!(f, "eof"),
            ReadError::TooLarge(n) => write!(f, "message too large ({n} bytes)"),
            ReadError::Io(e) => write!(f, "io: {e}"),
            ReadError::Json(e) => write!(f, "invalid json: {e}"),
        }
    }
}

/// Reads one raw frame. Returns `Eof` if the stream ends before a header.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Vec<u8>, ReadError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Err(ReadError::Eof),
            Ok(0) => return Err(ReadError::Io(io::ErrorKind::UnexpectedEof.into())),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(ReadError::Io(e)),
        }
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_INCOMING_BYTES {
        return Err(ReadError::TooLarge(n));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).map_err(ReadError::Io)?;
    Ok(buf)
}

pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_OUTGOING_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "outgoing message too large"));
    }
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

pub fn read_request<R: Read>(r: &mut R) -> Result<Request, ReadError> {
    let buf = read_frame(r)?;
    // serde_json's messages can quote values from the input (e.g. part of a query);
    // keep only the error category and position so logs never contain user text.
    serde_json::from_slice(&buf)
        .map_err(|e| ReadError::Json(format!("{:?} error at line {} column {}", e.classify(), e.line(), e.column())))
}

pub fn write_response<W: Write>(w: &mut W, resp: &Response) -> io::Result<()> {
    let bytes = serde_json::to_vec(resp).map_err(io::Error::other)?;
    write_frame(w, &bytes)
}

// ---------------------------------------------------------------- requests

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PageText {
    pub page: u32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        id: u32,
    },
    /// Is this document (by SHA-256 of its bytes) already indexed?
    Open {
        id: u32,
        sha256: String,
        #[serde(default)]
        filename: String,
        #[serde(rename = "pageCount")]
        page_count: u32,
    },
    IndexBegin {
        id: u32,
        sha256: String,
        #[serde(default)]
        filename: String,
        #[serde(rename = "pageCount")]
        page_count: u32,
    },
    IndexPages {
        id: u32,
        sha256: String,
        pages: Vec<PageText>,
    },
    IndexEnd {
        id: u32,
        sha256: String,
    },
    Search {
        id: u32,
        sha256: String,
        query: String,
        #[serde(default)]
        k: Option<usize>,
    },
}

impl Request {
    pub fn id(&self) -> u32 {
        match self {
            Request::Hello { id }
            | Request::Open { id, .. }
            | Request::IndexBegin { id, .. }
            | Request::IndexPages { id, .. }
            | Request::IndexEnd { id, .. }
            | Request::Search { id, .. } => *id,
        }
    }
}

// --------------------------------------------------------------- responses

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub page: u32,
    #[serde(rename = "chunkIndex")]
    pub chunk_index: u32,
    pub text: String,
    pub score: f32,
}

/// Stable, machine-readable error codes. The extension maps them to simple
/// Spanish messages; technical detail stays in the local log.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    NotIndexed,
    NoText,
    IndexingInProgress,
    ModelMissing,
    Internal,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Hello {
        id: u32,
        version: String,
        protocol: u32,
    },
    Opened {
        id: u32,
        sha256: String,
        /// "ready" (cache hit) or "needs_index" (cache miss)
        status: &'static str,
        #[serde(rename = "chunkCount")]
        chunk_count: u32,
    },
    Ack {
        id: u32,
    },
    Progress {
        id: u32,
        done: u32,
        total: u32,
        pct: u32,
    },
    Ready {
        id: u32,
        sha256: String,
        #[serde(rename = "chunkCount")]
        chunk_count: u32,
    },
    Results {
        id: u32,
        results: Vec<SearchHit>,
        /// "dense" or "hybrid": which retrieval produced this ranking (for local feedback).
        mode: &'static str,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u32,
    },
    Error {
        id: u32,
        code: ErrorCode,
    },
}

// -------------------------------------------------------------- validation

pub fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Semantic validation that serde cannot express. Returns a short reason for the log.
pub fn validate(req: &Request) -> Result<(), &'static str> {
    match req {
        Request::Hello { .. } => Ok(()),
        Request::Open { sha256, filename, page_count, .. }
        | Request::IndexBegin { sha256, filename, page_count, .. } => {
            if !is_valid_sha256(sha256) {
                return Err("bad sha256");
            }
            if filename.chars().count() > MAX_FILENAME_CHARS {
                return Err("filename too long");
            }
            if *page_count == 0 || *page_count > MAX_PAGES {
                return Err("bad page count");
            }
            Ok(())
        }
        Request::IndexPages { sha256, pages, .. } => {
            if !is_valid_sha256(sha256) {
                return Err("bad sha256");
            }
            for p in pages {
                if p.page == 0 || p.page > MAX_PAGES {
                    return Err("bad page number");
                }
                if p.text.chars().count() > MAX_PAGE_CHARS {
                    return Err("page text too long");
                }
            }
            Ok(())
        }
        Request::IndexEnd { sha256, .. } => {
            if !is_valid_sha256(sha256) {
                return Err("bad sha256");
            }
            Ok(())
        }
        Request::Search { sha256, query, k, .. } => {
            if !is_valid_sha256(sha256) {
                return Err("bad sha256");
            }
            let n = query.trim().chars().count();
            if n == 0 || n > MAX_QUERY_CHARS {
                return Err("bad query length");
            }
            if let Some(k) = k {
                if *k == 0 || *k > MAX_K {
                    return Err("bad k");
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn frame(json: &str) -> Vec<u8> {
        let mut v = (json.len() as u32).to_le_bytes().to_vec();
        v.extend_from_slice(json.as_bytes());
        v
    }

    #[test]
    fn roundtrip_frame() {
        let mut out = Vec::new();
        write_frame(&mut out, b"{\"a\":1}").unwrap();
        assert_eq!(&out[..4], &7u32.to_le_bytes());
        let got = read_frame(&mut Cursor::new(out)).unwrap();
        assert_eq!(got, b"{\"a\":1}");
    }

    #[test]
    fn eof_is_clean() {
        assert!(matches!(read_frame(&mut Cursor::new(Vec::new())), Err(ReadError::Eof)));
    }

    #[test]
    fn truncated_header_is_error_not_eof() {
        assert!(matches!(read_frame(&mut Cursor::new(vec![1u8, 0])), Err(ReadError::Io(_))));
    }

    #[test]
    fn truncated_body_is_error() {
        let mut v = 10u32.to_le_bytes().to_vec();
        v.extend_from_slice(b"{}");
        assert!(matches!(read_frame(&mut Cursor::new(v)), Err(ReadError::Io(_))));
    }

    #[test]
    fn oversized_rejected_without_allocating() {
        let v = (u32::MAX).to_le_bytes().to_vec();
        assert!(matches!(read_frame(&mut Cursor::new(v)), Err(ReadError::TooLarge(_))));
    }

    #[test]
    fn outgoing_limit_enforced() {
        let big = vec![b'x'; MAX_OUTGOING_BYTES + 1];
        assert!(write_frame(&mut Vec::new(), &big).is_err());
    }

    #[test]
    fn parses_search() {
        let j = format!(r#"{{"type":"search","id":7,"sha256":"{SHA}","query":"riesgo","k":5}}"#);
        let r = read_request(&mut Cursor::new(frame(&j))).unwrap();
        assert_eq!(r.id(), 7);
        assert!(validate(&r).is_ok());
    }

    #[test]
    fn rejects_unknown_type_and_fields() {
        let j = r#"{"type":"exec","id":1,"cmd":"calc.exe"}"#;
        assert!(matches!(read_request(&mut Cursor::new(frame(j))), Err(ReadError::Json(_))));
        let j = format!(r#"{{"type":"hello","id":1,"path":"..\\..\\x"}}"#);
        assert!(matches!(read_request(&mut Cursor::new(frame(&j))), Err(ReadError::Json(_))));
    }

    #[test]
    fn json_errors_never_echo_message_content() {
        let j = format!(r#"{{"type":"search","id":"confidential query text","sha256":"{SHA}","query":"x"}}"#);
        let Err(ReadError::Json(msg)) = read_request(&mut Cursor::new(frame(&j))) else { panic!() };
        assert!(!msg.contains("confidential"), "{msg}");
        let j = r#"{"type":"search secret words","id":1}"#;
        let Err(ReadError::Json(msg)) = read_request(&mut Cursor::new(frame(j))) else { panic!() };
        assert!(!msg.contains("secret"), "{msg}");
    }

    #[test]
    fn rejects_non_json_and_bad_utf8() {
        assert!(matches!(read_request(&mut Cursor::new(frame("not json"))), Err(ReadError::Json(_))));
        let mut v = 2u32.to_le_bytes().to_vec();
        v.extend_from_slice(&[0xff, 0xfe]);
        assert!(matches!(read_request(&mut Cursor::new(v)), Err(ReadError::Json(_))));
    }

    #[test]
    fn validation_rules() {
        let bad_sha = Request::Search { id: 1, sha256: "../../etc".into(), query: "x".into(), k: None };
        assert!(validate(&bad_sha).is_err());
        let upper = Request::Search { id: 1, sha256: SHA.to_uppercase(), query: "x".into(), k: None };
        assert!(validate(&upper).is_err());
        let empty_q = Request::Search { id: 1, sha256: SHA.into(), query: "   ".into(), k: None };
        assert!(validate(&empty_q).is_err());
        let long_q = Request::Search { id: 1, sha256: SHA.into(), query: "a".repeat(MAX_QUERY_CHARS + 1), k: None };
        assert!(validate(&long_q).is_err());
        let bad_k = Request::Search { id: 1, sha256: SHA.into(), query: "x".into(), k: Some(0) };
        assert!(validate(&bad_k).is_err());
        let zero_pages = Request::Open { id: 1, sha256: SHA.into(), filename: "a.pdf".into(), page_count: 0 };
        assert!(validate(&zero_pages).is_err());
        let page0 =
            Request::IndexPages { id: 1, sha256: SHA.into(), pages: vec![PageText { page: 0, text: "x".into() }] };
        assert!(validate(&page0).is_err());
        let ok = Request::IndexPages { id: 1, sha256: SHA.into(), pages: vec![PageText { page: 1, text: "x".into() }] };
        assert!(validate(&ok).is_ok());
    }

    #[test]
    fn response_shape() {
        let r = Response::Error { id: 3, code: ErrorCode::NoText };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"type":"error","id":3,"code":"no_text"}"#);
        let r = Response::Opened { id: 1, sha256: SHA.into(), status: "ready", chunk_count: 4 };
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v["chunkCount"], 4);
    }
}
