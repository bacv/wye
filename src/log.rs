use std::io::{self, Write};

pub fn log_already_in_session(session: String) -> Result<(), io::Error> {
    let mut stdout = io::stdout().lock();

    stdout.write_all(b"\rWye in session: ")?;
    stdout.write_all(session.as_bytes())?;
    stdout.write_all(b"\r\n")
}

pub fn log_opened_session(session: u32, pipe: &str) -> Result<(), io::Error> {
    let mut stdout = io::stdout().lock();

    stdout.write_all(b"\rWye new session: ")?;
    stdout.write_all(session.to_string().as_bytes())?;
    stdout.write_all(b", ")?;
    stdout.write_all(pipe.as_bytes())?;
    stdout.write_all(b"\r\n")
}

pub fn log_closed_session(session: u32) -> Result<(), io::Error> {
    let mut stdout = io::stdout().lock();

    stdout.write_all(b"\rWye closed session: ")?;
    stdout.write_all(session.to_string().as_bytes())?;
    stdout.write_all(b"\r\n")
}
