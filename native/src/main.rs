//! LegalSearch.exe — Chrome Native Messaging host.
//!
//! Chrome starts this process when the side panel connects and closes stdin when
//! the panel goes away; the process then exits. No service, no open ports, no
//! background work, no network access.

use legalsearch::cache::Cache;
use legalsearch::embed::{model_version, Embed, OnnxEmbedder};
use legalsearch::engine::Engine;
use legalsearch::hybrid::{CANDIDATE_HYBRID, DEFAULT_HYBRID};
use legalsearch::protocol::{self, ErrorCode, ReadError, Response};
use legalsearch::{error, info, logging, paths};
use std::io::{self, IsTerminal};
use std::panic::{self, AssertUnwindSafe};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("LegalSearch {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if io::stdin().is_terminal() {
        println!("LegalSearch is used from Google Chrome (the extension's side panel).");
        println!("There is no need to open this program directly.");
        return;
    }

    logging::init(paths::log_dir());
    panic::set_hook(Box::new(|p| error!("panic: {p}")));
    // Chrome passes the caller origin as the first argument. allowed_origins in the
    // host manifest already restricts callers; we additionally refuse anything that
    // is not a Chrome extension origin (e.g. a stray manual launch with a pipe).
    let origin = args.get(1).cloned().unwrap_or_default();
    if !origin.starts_with("chrome-extension://") {
        error!("refusing caller with unexpected origin argument");
        std::process::exit(2);
    }
    info!("start v{} origin={origin}", env!("CARGO_PKG_VERSION"));

    let code = run();
    info!("exit");
    std::process::exit(code);
}

fn open_cache() -> Cache {
    if let Some(dir) = paths::data_dir() {
        if std::fs::create_dir_all(&dir).is_ok() {
            match Cache::open(&dir.join("index.sqlite")) {
                Ok(c) => return c,
                Err(e) => error!("cannot open cache, using memory: {e}"),
            }
        }
    }
    Cache::open_in_memory().expect("in-memory sqlite")
}

fn run() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    let model_dir = paths::model_dir();
    let version = model_version(model_dir.as_deref());
    let mut engine = Engine::new(
        open_cache(),
        Box::new(move || {
            let dir = model_dir.clone().ok_or("no model dir")?;
            OnnxEmbedder::load(&dir).map(|e| Box::new(e) as Box<dyn Embed>)
        }),
        &version,
    )
    // Hybrid dense+BM25 retrieval is off by default (see hybrid.rs); opt-in for evaluation.
    .with_hybrid(if std::env::var("LEGALSEARCH_HYBRID").as_deref() == Ok("1") {
        CANDIDATE_HYBRID
    } else {
        DEFAULT_HYBRID
    });

    loop {
        let req = match protocol::read_request(&mut input) {
            Ok(r) => r,
            Err(ReadError::Eof) => return 0,
            Err(ReadError::Json(e)) => {
                error!("invalid message: {e}");
                let _ =
                    protocol::write_response(&mut output, &Response::Error { id: 0, code: ErrorCode::InvalidRequest });
                continue;
            }
            Err(e) => {
                // Framing is lost; nothing sensible to do but stop.
                error!("read: {e}");
                let _ =
                    protocol::write_response(&mut output, &Response::Error { id: 0, code: ErrorCode::InvalidRequest });
                return 1;
            }
        };
        if let Err(why) = protocol::validate(&req) {
            error!("rejected request: {why}");
            let _ = protocol::write_response(
                &mut output,
                &Response::Error { id: req.id(), code: ErrorCode::InvalidRequest },
            );
            continue;
        }
        let id = req.id();
        let mut write_failed = false;
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            engine.handle(req, &mut |resp| {
                if let Err(e) = protocol::write_response(&mut output, &resp) {
                    error!("write: {e}");
                    write_failed = true;
                }
            })
        }));
        if result.is_err() {
            let _ = protocol::write_response(&mut output, &Response::Error { id, code: ErrorCode::Internal });
        }
        if write_failed {
            return 1;
        }
    }
}
