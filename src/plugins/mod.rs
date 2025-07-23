use std::{fs, path::Path};

use mlua::Lua;

pub mod stdout;

pub trait 

pub fn load_plugin(rt: &Lua, name: &str, directory: &Path) -> Result<(), mlua::Error> {
    let plugin_path = directory.join(format!("{name}.lua"));
    let script = fs::read_to_string(plugin_path)?;
    rt.load(&script).exec()?;
    Ok(())
}
