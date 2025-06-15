pub mod child;
pub mod parent;
pub mod term;

use std::os::fd::{BorrowedFd, RawFd};

use mio::Token;
use nix::libc::c_int;

const STDIN_TOKEN: Token = Token(0);
const PTY_TOKEN: Token = Token(1);
const PIPE_TOKEN: Token = Token(2);
const SIGNAL_TOKEN: Token = Token(3);

static mut SIGNAL_OUT: RawFd = -1;

extern "C" fn handle_sigwinch(_: c_int) {
    let signal_fd = unsafe { BorrowedFd::borrow_raw(SIGNAL_OUT) };
    let _ = nix::unistd::write(signal_fd, &[0u8]);
}
