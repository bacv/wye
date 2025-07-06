use std::{
    cmp::Ordering,
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsFd, OwnedFd},
    path::Path,
};

use nix::{
    fcntl::{F_SETFL, OFlag, fcntl, open},
    libc::c_int,
    sys::{signal, stat::Mode},
    unistd::mkfifo,
};
use rustix::{
    event::{PollFd, PollFlags, pause, poll},
    io::dup,
};

use crate::{
    RESIZE_OUT, WYE_PIPE_DIR, WYE_PIPE_PREFIX,
    config::Config,
    log::{log_closed_session, log_opened_session},
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

fn setup_resize(signal_in: &OwnedFd) -> io::Result<()> {
    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigwinch),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe { signal::sigaction(signal::Signal::SIGWINCH, &sig_action)? };
    fcntl(signal_in, F_SETFL(OFlag::O_NONBLOCK))?;
    Ok(())
}

fn event_loop(
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

    let poll_stdin = dup(stdin.as_fd())?;
    let poll_master = dup(master_file.as_fd())?;
    let poll_pipe = dup(pipe_file.as_fd())?;
    let poll_resize = dup(resize_file.as_fd())?;

    let mut poll_fds = [
        PollFd::new(&poll_stdin, PollFlags::IN),
        PollFd::new(&poll_master, PollFlags::IN),
        PollFd::new(&poll_pipe, PollFlags::IN),
        PollFd::new(&poll_resize, PollFlags::IN),
    ];

    loop {
        match poll(&mut poll_fds, None) {
            Ok(0) => pause(), // No new events, will wake up.
            Ok(_) => {}       // Got something, proceed to event loop.
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        if poll_fds[0].revents().contains(PollFlags::IN) {
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

        if poll_fds[1].revents().contains(PollFlags::IN) {
            let n = master_file.read(&mut pty_buf)?;
            if n > 0 {
                stdout_lock.write_all(&pty_buf[..n])?;
                stdout_lock.flush()?;
            } else {
                return Ok(());
            }
        }
        if poll_fds[2].revents().contains(PollFlags::IN) {
            let n = pipe_file.read(&mut pipe_buf)?;
            if n > 0 {
                master_file.write_all(&pipe_buf[..n])?;
            }
        }
        if poll_fds[2].revents().contains(PollFlags::IN) {
            let mut drain_buf = [0; 1];
            _ = resize_file.read(&mut drain_buf);

            if let Ok(new_size) = get_winsize() {
                let _ = update_pty_size(master_file, &new_size);
            }
        }
    }
}

pub fn process(config: Config, master_fd: OwnedFd) -> Result<(), Box<dyn std::error::Error>> {
    let mut master_file = File::from(master_fd);

    let pipe_path = format!("{WYE_PIPE_DIR}/{WYE_PIPE_PREFIX}-{}", config.session_number);
    let pipe_fd = prepare_pipe(&pipe_path)?;
    let mut pipe_file = File::from(pipe_fd);

    let (resize_in, resize_out) = nix::unistd::pipe()?;
    RESIZE_OUT.get_or_init(|| resize_out);
    setup_resize(&resize_in)?;
    let mut resize_file = File::from(resize_in);

    log_opened_session(config.session_number, &pipe_path)?;
    let res = event_loop(
        &mut io::stdin(),
        &mut master_file,
        &mut pipe_file,
        &mut resize_file,
    );

    std::fs::remove_file(pipe_path)?;
    log_closed_session(config.session_number)?;

    res
}
