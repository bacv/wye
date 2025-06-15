pub mod child;
pub mod parent;
pub mod term;

use std::os::fd::RawFd;

use mio::Token;

const STDIN_TOKEN: Token = Token(0);
const PTY_TOKEN: Token = Token(1);
const PIPE_TOKEN: Token = Token(2);
const SIGNAL_TOKEN: Token = Token(3);

static mut SIGNAL_OUT: RawFd = -1;
