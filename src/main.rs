use std::os::fd::AsRawFd;

use nix::{
    pty::openpty,
    sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr},
    unistd::{ForkResult, close},
};
use wye::{
    child, config, parent,
    term::{TerminalModeGuard, get_winsize},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::parse_config()?;
    let fd = std::io::stdin().as_raw_fd();
    let _guard = TerminalModeGuard::new(fd)?;

    let initial_winsize = get_winsize()?;
    let mut termios_settings = tcgetattr(std::io::stdin())?;

    let pty = openpty(Some(&initial_winsize), Some(&termios_settings))?;

    termios_settings
        .local_flags
        .remove(LocalFlags::ICANON | LocalFlags::ECHO | LocalFlags::ISIG);
    tcsetattr(std::io::stdin(), SetArg::TCSANOW, &termios_settings)?;

    match unsafe { nix::unistd::fork() } {
        Ok(ForkResult::Parent { .. }) => {
            close(pty.slave)?;
            parent::process(&config, pty.master)
        }
        Ok(ForkResult::Child) => {
            close(pty.master)?;
            child::process(&config, pty.slave)
        }
        Err(e) => {
            std::process::exit(e as i32);
        }
    }
}
