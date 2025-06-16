use std::{
    env, error,
    ffi::CString,
    os::fd::{AsRawFd, OwnedFd},
};

use nix::unistd::{close, dup2_stderr, dup2_stdin, dup2_stdout, execvp};

use crate::{FALLBACK_SHELL, WYE_SESSION_VAR, config::Config, term::tiocsctty};

pub fn process(config: Config, slave_fd: OwnedFd) -> Result<(), Box<dyn error::Error>> {
    nix::unistd::setsid()?;
    unsafe {
        tiocsctty(slave_fd.as_raw_fd())?;
    }

    dup2_stdin(&slave_fd)?;
    dup2_stdout(&slave_fd)?;
    dup2_stderr(&slave_fd)?;

    close(slave_fd)?;

    unsafe { env::set_var(WYE_SESSION_VAR, config.session_number.to_string()) };
    let program = config
        .program
        .or(config.shell)
        .unwrap_or(FALLBACK_SHELL.to_string());

    let shell_path = CString::new(program).unwrap();
    let shell_args = [shell_path.clone()];
    let Err(e) = execvp(&shell_path, &shell_args);
    std::process::exit(e as i32);
}
