use std::os::fd::{AsRawFd, OwnedFd};

use nix::{
    libc::{STDOUT_FILENO, TIOCGWINSZ, TIOCSCTTY, TIOCSWINSZ},
    pty::Winsize,
    sys::termios::{SetArg, Termios, tcgetattr, tcsetattr},
};

nix::ioctl_read_bad!(tiocgwinsz, TIOCGWINSZ, Winsize);
nix::ioctl_write_ptr_bad!(tiocswinsz, TIOCSWINSZ, Winsize);
nix::ioctl_none_bad!(tiocsctty, TIOCSCTTY);

pub fn get_winsize() -> Result<Winsize, Box<dyn std::error::Error>> {
    let mut winsize = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    unsafe {
        tiocgwinsz(STDOUT_FILENO, &mut winsize)?;
    }
    Ok(winsize)
}

pub fn update_pty_size(
    fd: &impl AsRawFd,
    size: &Winsize,
) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        tiocswinsz(fd.as_raw_fd(), size)?;
    }
    Ok(())
}

struct TerminalModeGuard {
    original: Termios,
    fd: OwnedFd,
}

impl TerminalModeGuard {
    fn new(fd: OwnedFd) -> nix::Result<Self> {
        let original = tcgetattr(&fd)?;
        Ok(Self { original, fd })
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _ = tcsetattr(&self.fd, SetArg::TCSANOW, &self.original);
    }
}
