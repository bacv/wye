use mio::{Events, Interest, Poll, Token, unix::SourceFd};
use nix::libc;
use nix::{
    fcntl::{OFlag, open},
    libc::{TIOCGWINSZ, TIOCSWINSZ},
    pty::{Winsize, openpty},
    sys::{
        signal::{SaFlags, SigAction, SigHandler, SigSet},
        termios::Termios,
    },
    unistd::{ForkResult, close, dup2_stderr, dup2_stdin, dup2_stdout, execvp},
};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd},
};

const STDIN_TOKEN: Token = Token(0);
const PTY_TOKEN: Token = Token(1);
const PIPE_TOKEN: Token = Token(2);
const SIGNAL_TOKEN: Token = Token(3);

static mut SIGNAL_PIPE: RawFd = -1;

nix::ioctl_read_bad!(tiocgwinsz, TIOCGWINSZ, Winsize);
nix::ioctl_write_ptr_bad!(tiocswinsz, TIOCSWINSZ, Winsize);

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

fn update_pty_size(
    pty_master_fd: &impl AsRawFd,
    new_size: &Winsize,
) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        tiocswinsz(pty_master_fd.as_raw_fd(), new_size)?;
    }
    Ok(())
}

extern "C" fn handle_sigwinch(_: libc::c_int) {
    unsafe {
        let signal_fd = BorrowedFd::borrow_raw(SIGNAL_PIPE);
        let _ = nix::unistd::write(signal_fd, &[0u8]);
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

    let master_binding = master_fd.as_raw_fd();
    let mut master_file = File::from(master_fd);
    let mut master_fd_source = SourceFd(&master_binding);
    let mut stdin_source = SourceFd(&stdin_fd);

    let pipe_path = "/tmp/wye.pipe";
    if std::path::Path::new(pipe_path).exists() {
        std::fs::remove_file(pipe_path)?;
    }
    nix::unistd::mkfifo(pipe_path, nix::sys::stat::Mode::S_IRWXU)?;
    let pipe_fd = open(
        pipe_path,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        nix::sys::stat::Mode::empty(),
    )?;
    let pipe_binding = pipe_fd.as_raw_fd();
    let mut pipe_source = SourceFd(&pipe_binding);
    let mut pipe_buf = [0u8; 1024];

    let signal_pipe = nix::unistd::pipe()?;
    unsafe { SIGNAL_PIPE = signal_pipe.1.as_raw_fd() };
    let signal_binding = signal_pipe.0.as_raw_fd();
    nix::fcntl::fcntl(&signal_pipe.0, nix::fcntl::F_SETFL(OFlag::O_NONBLOCK))?;
    let sig_action = SigAction::new(
        SigHandler::Handler(handle_sigwinch),
        SaFlags::empty(),
        SigSet::empty(),
    );

    unsafe { nix::sys::signal::sigaction(nix::sys::signal::SIGWINCH, &sig_action)? };
    let mut signal_source = SourceFd(&signal_binding);

    poll.registry()
        .register(&mut stdin_source, STDIN_TOKEN, Interest::READABLE)?;
    poll.registry()
        .register(&mut master_fd_source, PTY_TOKEN, Interest::READABLE)?;
    poll.registry()
        .register(&mut pipe_source, PIPE_TOKEN, Interest::READABLE)?;
    poll.registry()
        .register(&mut signal_source, SIGNAL_TOKEN, Interest::READABLE)?;

    let mut stdin_buf = [0u8; 1024];
    let mut pty_buf = [0u8; 1024];

    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();

    loop {
        match poll.poll(&mut events, None) {
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        for event in events.iter() {
            match event.token() {
                STDIN_TOKEN => {
                    let n = unsafe {
                        libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut _, stdin_buf.len())
                    };
                    match n.cmp(&0) {
                        std::cmp::Ordering::Greater => {
                            master_file.write_all(&stdin_buf[..n as usize])?;
                        }
                        std::cmp::Ordering::Equal => {
                            return Ok(());
                        }
                        std::cmp::Ordering::Less => {
                            return Err(std::io::Error::last_os_error().into());
                        }
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
                    let n = nix::unistd::read(&pipe_fd, &mut pipe_buf)?;
                    if n > 0 {
                        master_file.write_all(&pipe_buf[..n as usize])?;
                    }
                }
                SIGNAL_TOKEN => {
                    let mut drain = [0u8; 32];
                    let signal_fd = unsafe { BorrowedFd::borrow_raw(signal_binding) };
                    while nix::unistd::read(signal_fd, &mut drain).is_ok() {
                        // Keep reading until it's empty.
                    }

                    if let Ok(new_size) = get_winsize() {
                        let _ = update_pty_size(&master_binding, &new_size);
                    }
                }
                _ => {}
            }
        }
    }
}

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
