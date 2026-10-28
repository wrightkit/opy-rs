//! Shared LPP session harness for provider integration tests.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

pub(crate) struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Session {
    pub(crate) fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_opy-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("provider spawns");
        Self {
            input: child.stdin.take().expect("provider stdin"),
            output: BufReader::new(child.stdout.take().expect("provider stdout")),
            child,
        }
    }

    pub(crate) fn request(&mut self, request: Value) -> Value {
        writeln!(self.input, "{request}").expect("write request");
        self.input.flush().expect("flush request");
        let mut line = String::new();
        self.output.read_line(&mut line).expect("read response");
        serde_json::from_str(&line).expect("response is JSON")
    }

    pub(crate) fn initialize(&mut self) -> Value {
        self.initialize_version("1.1")
    }

    pub(crate) fn initialize_version(&mut self, version: &str) -> Value {
        self.request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "lpp/initialize",
            "params": { "protocolVersion": version },
        }))
    }

    pub(crate) fn shutdown(mut self) {
        let response = self.request(json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "lpp/shutdown",
            "params": {},
        }));
        assert_eq!(response["result"], Value::Null);
        assert_eq!(self.child.wait().expect("provider exits").code(), Some(0));
    }
}

pub(crate) fn file_uri(path: &str) -> String {
    let path = Path::new(path).canonicalize().expect("fixture path");
    format!("file://{}", path.to_string_lossy())
}
