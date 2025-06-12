use mio::{Events, Interest, Poll, Token, unix::SourceFd};
use nix::{
    libc::{self, TIOCGWINSZ, TIOCSWINSZ},
    pty::{Winsize, openpty},
    sys::termios::Termios,
    unistd::{ForkResult, close, execvp},
};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, OwnedFd},
};

use nix::unistd::{dup2_stderr, dup2_stdin, dup2_stdout};

const STDIN_TOKEN: Token = Token(0);
const PTY_TOKEN: Token = Token(1);
const PIPE_TOKEN: Token = Token(2);

nix::ioctl_read_bad!(tiocgwinsz, TIOCGWINSZ, Winsize);
nix::ioctl_write_ptr_bad!(tiocswinsz, TIOCSWINSZ, Winsize);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let initial_winsize = get_winsize()?;
    let termios_settings: Termios = unsafe { std::mem::zeroed() };
    let pty = openpty(Some(&initial_winsize), Some(&termios_settings))?;

    match unsafe { nix::unistd::fork() } {
        Ok(ForkResult::Parent { .. }) => parent_process(pty.master),
        Ok(ForkResult::Child) => child_process(pty.slave),
        Err(e) => {
            std::process::exit(e as i32);
        }
    }
}

pub fn child_process(slave_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    nix::unistd::setsid()?;

    dup2_stdin(&slave_fd)?;
    dup2_stdout(&slave_fd)?;
    dup2_stderr(&slave_fd)?;

    close(slave_fd)?;

    let shell_path = CString::new("/bin/sh").unwrap();
    let shell_args = [shell_path.clone()];
    let Err(e) = execvp(&shell_path, &shell_args);
    std::process::exit(e as i32);
}

pub fn parent_process(master_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    let stdin_fd = libc::STDIN_FILENO;

    let binding = master_fd.as_raw_fd();
    let mut master_file = File::from(master_fd);
    let mut master_fd_source = SourceFd(&binding);
    let mut stdin_source = SourceFd(&stdin_fd);

    let pipe_path = "/tmp/wye.pipe";
    if std::path::Path::new(pipe_path).exists() {
        std::fs::remove_file(pipe_path)?;
    }
    nix::unistd::mkfifo(pipe_path, nix::sys::stat::Mode::S_IRWXU)?;
    use nix::fcntl::{OFlag, open};
    let pipe_fd = open(
        pipe_path,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        nix::sys::stat::Mode::empty(),
    )?;
    let binding = pipe_fd.as_raw_fd();
    let mut pipe_source = SourceFd(&binding);
    let mut pipe_buf = [0u8; 1024];

    poll.registry()
        .register(&mut stdin_source, STDIN_TOKEN, Interest::READABLE)?;
    poll.registry()
        .register(&mut master_fd_source, PTY_TOKEN, Interest::READABLE)?;
    poll.registry()
        .register(&mut pipe_source, PIPE_TOKEN, Interest::READABLE)?;

    let mut stdin_buf = [0u8; 1024];
    let mut pty_buf = [0u8; 1024];

    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                STDIN_TOKEN => {
                    let n = unsafe {
                        libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut _, stdin_buf.len())
                    };
                    if n > 0 {
                        master_file.write_all(&stdin_buf[..n as usize])?;
                    } else {
                        return Ok(());
                    }
                }
                PTY_TOKEN => {
                    let n = master_file.read(&mut pty_buf)?;
                    if n > 0 {
                        stdout_lock.write_all(&pty_buf[..n])?;
                        stdout_lock.flush()?;
                    } else {
                        return Ok(());
                    }
                }
                PIPE_TOKEN => {
                    let n = unsafe {
                        libc::read(
                            pipe_fd.as_raw_fd(),
                            pipe_buf.as_mut_ptr() as *mut _,
                            pipe_buf.len(),
                        )
                    };
                    if n > 0 {
                        master_file.write_all(&pipe_buf[..n as usize])?;
                    }
                }
                _ => {}
            }
        }
    }
}

fn get_winsize() -> Result<Winsize, Box<dyn std::error::Error>> {
    let mut winsize = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    unsafe {
        tiocgwinsz(libc::STDOUT_FILENO, &mut winsize)?;
    }
    Ok(winsize)
}
