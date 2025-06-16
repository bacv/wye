pub mod child;
pub mod config;
pub mod log;
pub mod parent;
pub mod term;

use std::{os::fd::OwnedFd, sync::OnceLock};

use mio::Token;

const STDIN_TOKEN: Token = Token(0);
const PTY_TOKEN: Token = Token(1);
const PIPE_TOKEN: Token = Token(2);
const RESIZE_TOKEN: Token = Token(3);

pub const WYE_SESSION_VAR: &str = "WYE";
const WYE_PIPE_DIR: &str = "/tmp";
const WYE_PIPE_PREFIX: &str = "wye";

static RESIZE_OUT: OnceLock<OwnedFd> = OnceLock::new();

const FALLBACK_SHELL: &str = "/bin/sh";
