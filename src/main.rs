use nix::sys::termios::{self, LocalFlags, SetArg, tcgetattr};
use nix::{
    pty::openpty,
    unistd::{ForkResult, close},
};
use wye::{child_process, get_winsize, parent_process};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let initial_winsize = get_winsize()?;
    let mut termios_settings = tcgetattr(std::io::stdin())?;

    let pty = openpty(Some(&initial_winsize), Some(&termios_settings))?;

    termios_settings
        .local_flags
        .remove(LocalFlags::ICANON | LocalFlags::ECHO);
    termios::tcsetattr(std::io::stdin(), SetArg::TCSANOW, &termios_settings)?;

    match unsafe { nix::unistd::fork() } {
        Ok(ForkResult::Parent { .. }) => {
            close(pty.slave)?;
            parent_process(pty.master)
        }
        Ok(ForkResult::Child) => {
            close(pty.master)?;
            child_process(pty.slave)
        }
        Err(e) => {
            std::process::exit(e as i32);
        }
    }
}
