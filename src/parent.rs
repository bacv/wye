use std::{
    cmp::Ordering,
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd, OwnedFd},
    path::Path,
};

use mio::{Events, Interest, Poll, unix::SourceFd};
use nix::{
    fcntl::{F_SETFL, OFlag, fcntl, open},
    libc::{STDIN_FILENO, c_int},
    sys::{signal, stat::Mode},
    unistd::mkfifo,
};

use crate::{
    PIPE_TOKEN, PTY_TOKEN, RESIZE_OUT, RESIZE_TOKEN, STDIN_TOKEN, WYE_PIPE_DIR, WYE_PIPE_PREFIX,
    config::Config,
    term::{get_winsize, update_pty_size},
};

extern "C" fn handle_sigwinch(_: c_int) {
    if let Some(fd) = RESIZE_OUT.get() {
        let _ = nix::unistd::write(fd, &[0u8]);
    }
}

fn prepare_pipe(path: &str) -> io::Result<OwnedFd> {
    if Path::new(path).exists() {
        std::fs::remove_file(path)?;
    }
    mkfifo(path, Mode::S_IRWXU)?;
    let pipe_fd = open(path, OFlag::O_RDONLY | OFlag::O_NONBLOCK, Mode::empty())?;
    Ok(pipe_fd)
}

fn setup_master(poll: &mut Poll, master_fd: OwnedFd) -> io::Result<File> {
    let master_raw = master_fd.as_raw_fd();
    let mut master_source = SourceFd(&master_raw);
    poll.registry()
        .register(&mut master_source, PTY_TOKEN, Interest::READABLE)?;
    Ok(File::from(master_fd))
}

fn setup_stdin(poll: &mut Poll) -> io::Result<io::Stdin> {
    let mut stdin_source = SourceFd(&STDIN_FILENO);
    poll.registry()
        .register(&mut stdin_source, STDIN_TOKEN, Interest::READABLE)?;
    Ok(io::stdin())
}

fn setup_pipe(poll: &mut Poll, pipe_fd: OwnedFd) -> io::Result<File> {
    let pipe_raw = pipe_fd.as_raw_fd();
    let mut pipe_source = SourceFd(&pipe_raw);
    poll.registry()
        .register(&mut pipe_source, PIPE_TOKEN, Interest::READABLE)?;
    Ok(File::from(pipe_fd))
}

fn setup_resize(poll: &mut Poll, signal_in: OwnedFd) -> io::Result<File> {
    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigwinch),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe { signal::sigaction(signal::Signal::SIGWINCH, &sig_action)? };
    fcntl(&signal_in, F_SETFL(OFlag::O_NONBLOCK))?;

    let signal_fd = signal_in.as_raw_fd();
    let mut signal_source = SourceFd(&signal_fd);
    poll.registry()
        .register(&mut signal_source, RESIZE_TOKEN, Interest::READABLE)?;
    Ok(File::from(signal_in))
}

fn event_loop(
    poll: &mut Poll,
    stdin: &mut io::Stdin,
    master_file: &mut File,
    pipe_file: &mut File,
    resize_file: &mut File,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin_buf = [0u8; 1024];
    let mut pty_buf = [0u8; 1024];
    let mut pipe_buf = [0u8; 1024];

    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    let mut events = Events::with_capacity(128);

    loop {
        match poll.poll(&mut events, None) {
            Ok(_) => {}
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
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
                        Ordering::Greater => {
                            master_file.write_all(&stdin_buf[..n])?;
                        }
                        Ordering::Equal => {
                            return Ok(());
                        }
                        Ordering::Less => {
                            return Err(io::Error::last_os_error().into());
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
                RESIZE_TOKEN => {
                    let mut drain_buf = [0; 1];
                    _ = resize_file.read(&mut drain_buf);

                    if let Ok(new_size) = get_winsize() {
                        let _ = update_pty_size(master_file, &new_size);
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn process(config: Config, master_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    let mut poll = Poll::new()?;

    let mut master_file = setup_master(&mut poll, master_fd)?;
    let mut stdin = setup_stdin(&mut poll)?;

    let pipe_path = format!("{WYE_PIPE_DIR}/{WYE_PIPE_PREFIX}-{}", config.session_number);
    let pipe_fd = prepare_pipe(&pipe_path)?;
    let mut pipe_file = setup_pipe(&mut poll, pipe_fd)?;

    let (resize_in, resize_out) = nix::unistd::pipe()?;
    RESIZE_OUT.get_or_init(|| resize_out);
    let mut resize_file = setup_resize(&mut poll, resize_in)?;

    println!("Wye session {}, {pipe_path}", config.session_number);
    let res = event_loop(
        &mut poll,
        &mut stdin,
        &mut master_file,
        &mut pipe_file,
        &mut resize_file,
    );

    std::fs::remove_file(pipe_path)?;
    println!("Wye session {} closed", config.session_number);

    res
}
