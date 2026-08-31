//! The console-mode log store: a capped ring buffer fed by a dedicated
//! tracing layer (see `main.rs`), read by the status bar's console view.
//!
//! The layer runs at `vireo=debug` regardless of `RUST_LOG`, so the console
//! is verbose even when stderr is quiet. Lines are sequence-numbered so the
//! UI can poll cheaply ("everything after seq N") without copying the whole
//! buffer each tick.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// How many lines the console keeps; older ones fall off the top.
const CAP: usize = 2000;

static BUF: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
static SEQ: AtomicU64 = AtomicU64::new(0);

fn push_line(line: &str) {
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let mut buf = BUF.lock().unwrap();
    if buf.len() >= CAP {
        buf.pop_front();
    }
    buf.push_back((seq, line.to_string()));
}

/// Every buffered line newer than `after`, plus the newest sequence number
/// (pass it back next call). Cheap when nothing is new.
pub fn lines_since(after: u64) -> (u64, Vec<String>) {
    let newest = SEQ.load(Ordering::Relaxed);
    if newest == after {
        return (newest, Vec::new());
    }
    let buf = BUF.lock().unwrap();
    let lines = buf.iter().filter(|(s, _)| *s > after).map(|(_, l)| l.clone()).collect();
    (newest, lines)
}

/// `MakeWriter` for the console's fmt layer: collects the layer's output and
/// buffers it line by line.
#[derive(Clone, Default)]
pub struct ConsoleWriter;

impl std::io::Write for ConsoleWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for line in String::from_utf8_lossy(bytes).split('\n') {
            push_line(line);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ConsoleWriter {
    type Writer = ConsoleWriter;
    fn make_writer(&'a self) -> Self::Writer {
        ConsoleWriter
    }
}
