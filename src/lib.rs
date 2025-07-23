pub mod child;
pub mod config;
pub mod log;
pub mod parent;
pub mod plugins;
pub mod term;

use std::{os::fd::OwnedFd, sync::OnceLock};

use regex::Regex;

pub const WYE_SESSION_VAR: &str = "WYE";
const WYE_PIPE_DIR: &str = "/tmp";
const WYE_PIPE_PREFIX: &str = "wye";

static RESIZE_OUT: OnceLock<OwnedFd> = OnceLock::new();

const FALLBACK_SHELL: &str = "/bin/sh";

pub struct StdoutProcessor {
    buf: [u8; 1024],
    file_path_regex: Regex,
}

impl StdoutProcessor {
    pub fn new() -> Self {
        Self {
            buf: [0u8; 1024],
            file_path_regex: Regex::new(r"(/[^/\s]+\.\w+)")
                .expect("Failed to compile file path regex"),
        }
    }

    pub fn process(&mut self, src: &[u8], dst: &mut [u8]) -> usize {
        let input_str = String::from_utf8_lossy(src);
        let mut processed_output = String::new();
        let mut last_match_end = 0;

        for mat in self.file_path_regex.find_iter(&input_str) {
            processed_output.push_str(&input_str[last_match_end..mat.start()]);
            processed_output.push_str(mat.as_str());
            processed_output.push_str("[1]");
            last_match_end = mat.end();
        }
        processed_output.push_str(&input_str[last_match_end..]);

        let processed_bytes = processed_output.as_bytes();

        let bytes_to_write = std::cmp::min(processed_bytes.len(), dst.len());
        dst[..bytes_to_write].copy_from_slice(&processed_bytes[..bytes_to_write]);

        bytes_to_write
    }
}

impl Default for StdoutProcessor {
    fn default() -> Self {
        Self::new()
    }
}
