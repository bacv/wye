use std::os::fd::AsRawFd;

use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
use nix::{
    pty::openpty,
    unistd::{ForkResult, close},
};
use wye::term::{TerminalModeGuard, get_winsize};
use wye::{child, parent};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fd = std::io::stdin().as_raw_fd();
    let _guard = TerminalModeGuard::new(fd)?;

    let initial_winsize = get_winsize()?;
    let mut termios_settings = tcgetattr(std::io::stdin())?;

    let pty = openpty(Some(&initial_winsize), Some(&termios_settings))?;

    termios_settings
        .local_flags
        .remove(LocalFlags::ICANON | LocalFlags::ECHO);
    tcsetattr(std::io::stdin(), SetArg::TCSANOW, &termios_settings)?;

    match unsafe { nix::unistd::fork() } {
        Ok(ForkResult::Parent { .. }) => {
            close(pty.slave)?;
            parent::process(pty.master)
        }
        Ok(ForkResult::Child) => {
            close(pty.master)?;
            child::process(pty.slave)
        }
        Err(e) => {
            std::process::exit(e as i32);
        }
    }
}
