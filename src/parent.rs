use std::{
    fs::File,
    io::{Read, Stdin, Write},
    os::fd::{AsRawFd, OwnedFd},
};

use mio::{Events, Interest, Poll, unix::SourceFd};
use nix::{
    fcntl::{OFlag, open},
    libc::STDIN_FILENO,
    sys::signal::{SaFlags, SigAction, SigHandler, SigSet},
};

use crate::{
    PIPE_TOKEN, PTY_TOKEN, SIGNAL_OUT, SIGNAL_TOKEN, STDIN_TOKEN, handle_sigwinch,
    term::{get_winsize, update_pty_size},
};

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
    let mut stdin_source = SourceFd(&STDIN_FILENO);
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

fn merge_loop(
    poll: &mut Poll,
    stdin: &mut Stdin,
    master_file: &mut File,
    pipe_file: &mut File,
    signal_file: &mut File,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin_buf = [0u8; 1024];
    let mut pty_buf = [0u8; 1024];
    let mut pipe_buf = [0u8; 1024];

    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();

    let mut events = Events::with_capacity(128);

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
                            master_file.write_all(&stdin_buf[..n])?;
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
                        master_file.write_all(&pipe_buf[..n])?;
                    }
                }
                SIGNAL_TOKEN => {
                    let mut drain_buf = [0; 1];
                    _ = signal_file.read(&mut drain_buf);

                    if let Ok(new_size) = get_winsize() {
                        let _ = update_pty_size(master_file, &new_size);
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn process(master_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    let mut poll = Poll::new()?;

    let mut master_file = setup_master(&mut poll, master_fd)?;
    let mut stdin = setup_stdin(&mut poll)?;

    let pipe_path = "/tmp/wye.pipe";
    let pipe_fd = prepare_pipe(pipe_path)?;
    let mut pipe_file = setup_pipe(&mut poll, pipe_fd)?;

    let (signal_in, signal_out) = nix::unistd::pipe()?;
    unsafe { SIGNAL_OUT = signal_out.as_raw_fd() };
    let mut signal_file = setup_signal(&mut poll, signal_in)?;

    merge_loop(
        &mut poll,
        &mut stdin,
        &mut master_file,
        &mut pipe_file,
        &mut signal_file,
    )
}
