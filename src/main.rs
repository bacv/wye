use mio::{Events, Interest, Poll, Token, unix::SourceFd};
use nix::libc;
use nix::sys::termios::{self, LocalFlags, SetArg, tcgetattr};
use nix::{
    fcntl::{OFlag, open},
    libc::{TIOCGWINSZ, TIOCSWINSZ},
    pty::{Winsize, openpty},
    sys::signal::{SaFlags, SigAction, SigHandler, SigSet},
    unistd::{ForkResult, close, dup2_stderr, dup2_stdin, dup2_stdout, execvp},
};
use std::io::Stdin;
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

static mut SIGNAL_OUT: RawFd = -1;

nix::ioctl_read_bad!(tiocgwinsz, TIOCGWINSZ, Winsize);
nix::ioctl_write_ptr_bad!(tiocswinsz, TIOCSWINSZ, Winsize);
nix::ioctl_none_bad!(tiocsctty, libc::TIOCSCTTY);

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

fn update_pty_size(fd: &impl AsRawFd, size: &Winsize) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        tiocswinsz(fd.as_raw_fd(), size)?;
    }
    Ok(())
}

extern "C" fn handle_sigwinch(_: libc::c_int) {
    let signal_fd = unsafe { BorrowedFd::borrow_raw(SIGNAL_OUT) };
    let _ = nix::unistd::write(signal_fd, &[0u8]);
}

fn child_process(slave_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    nix::unistd::setsid()?;
    unsafe {
        tiocsctty(slave_fd.as_raw_fd())?;
    }

    dup2_stdin(&slave_fd)?;
    dup2_stdout(&slave_fd)?;
    dup2_stderr(&slave_fd)?;

    close(slave_fd)?;

    let shell_path = CString::new("/bin/sh").unwrap();
    let shell_args = [shell_path.clone()];
    let Err(e) = execvp(&shell_path, &shell_args);
    std::process::exit(e as i32);
}

fn prepare_pipe(path: &str) -> std::io::Result<OwnedFd> {
    if std::path::Path::new(path).exists() {
        std::fs::remove_file(path)?;
    }
    nix::unistd::mkfifo(path, nix::sys::stat::Mode::S_IRWXU)?;
    let pipe_fd = open(
        path,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        nix::sys::stat::Mode::empty(),
    )?;
    Ok(pipe_fd)
}

fn setup_master(poll: &mut Poll, master_fd: OwnedFd) -> std::io::Result<File> {
    let master_raw = master_fd.as_raw_fd();
    let mut master_source = SourceFd(&master_raw);
    poll.registry()
        .register(&mut master_source, PTY_TOKEN, Interest::READABLE)?;
    Ok(File::from(master_fd))
}

fn setup_stdin(poll: &mut Poll) -> std::io::Result<Stdin> {
    let mut stdin_source = SourceFd(&libc::STDIN_FILENO);
    poll.registry()
        .register(&mut stdin_source, STDIN_TOKEN, Interest::READABLE)?;
    Ok(std::io::stdin())
}

fn setup_pipe(poll: &mut Poll, pipe_fd: OwnedFd) -> std::io::Result<File> {
    let pipe_raw = pipe_fd.as_raw_fd();
    let mut pipe_source = SourceFd(&pipe_raw);
    poll.registry()
        .register(&mut pipe_source, PIPE_TOKEN, Interest::READABLE)?;
    Ok(File::from(pipe_fd))
}

fn setup_signal(poll: &mut Poll, signal_in: OwnedFd) -> std::io::Result<File> {
    let sig_action = SigAction::new(
        SigHandler::Handler(handle_sigwinch),
        SaFlags::empty(),
        SigSet::empty(),
    );
    unsafe { nix::sys::signal::sigaction(nix::sys::signal::SIGWINCH, &sig_action)? };
    nix::fcntl::fcntl(&signal_in, nix::fcntl::F_SETFL(OFlag::O_NONBLOCK))?;

    let signal_fd = signal_in.as_raw_fd();
    let mut signal_source = SourceFd(&signal_fd);
    poll.registry()
        .register(&mut signal_source, SIGNAL_TOKEN, Interest::READABLE)?;
    Ok(File::from(signal_in))
}

fn parent_process(master_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    let mut master_file = setup_master(&mut poll, master_fd)?;
    let mut stdin = setup_stdin(&mut poll)?;

    let pipe_path = "/tmp/wye.pipe";
    let pipe_fd = prepare_pipe(pipe_path)?;
    let mut pipe_file = setup_pipe(&mut poll, pipe_fd)?;

    let (signal_in, signal_out) = nix::unistd::pipe()?;
    unsafe { SIGNAL_OUT = signal_out.as_raw_fd() };
    let mut signal_file = setup_signal(&mut poll, signal_in)?;

    let mut stdin_buf = [0u8; 1024];
    let mut pty_buf = [0u8; 1024];
    let mut pipe_buf = [0u8; 1024];

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
                    let n = stdin.read(&mut stdin_buf)?;
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
                    let n = pipe_file.read(&mut pipe_buf)?;
                    if n > 0 {
                        master_file.write_all(&pipe_buf[..n as usize])?;
                    }
                }
                SIGNAL_TOKEN => {
                    let mut drain_buf = [0; 1];
                    _ = signal_file.read(&mut drain_buf);

                    if let Ok(new_size) = get_winsize() {
                        let _ = update_pty_size(&master_file, &new_size);
                    }
                }
                _ => {}
            }
        }
    }
}

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
