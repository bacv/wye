use std::{env, fs};

use crate::WYE_SESSION_VAR;

pub struct Config {
    pub session_number: u32,
    pub shell: Option<String>,
    pub program: Option<String>,
    pub in_session: bool,
}

pub fn parse_config() -> Result<Config, Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut session_number: Option<u32> = None;
    let mut program: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            arg if arg.starts_with("-s") => {
                let num = if arg.len() > 2 {
                    &arg[2..]
                } else {
                    i += 1;
                    args.get(i).ok_or("Missing session number after -s")?
                };
                session_number = Some(num.parse()?);
            }
            arg => {
                program = Some(arg.to_string());
            }
        }
        i += 1;
    }

    let session_number = match session_number {
        Some(n) => n,
        None => {
            let max = fs::read_dir("/tmp")
                .ok()
                .into_iter()
                .flat_map(|entries| entries.flatten())
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .and_then(|name| name.strip_prefix("wye-"))
                        .and_then(|num_str| num_str.parse::<u32>().ok())
                })
                .max();

            max.map_or(0, |n| n + 1)
        }
    };

    let shell = env::var("SHELL").ok();
    let in_session = env::var(WYE_SESSION_VAR).is_ok();

    Ok(Config {
        session_number,
        shell,
        program,
        in_session,
    })
}
