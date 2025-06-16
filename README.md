🫲 wye 🫱

> Wye is reverse tee.

## What is wye?

Wye is a simple terminal tool that works like a reverse tee. Instead of splitting one input into multiple outputs, wye combines multiple inputs into one terminal session. It reads commands from your keyboard and also from a special file called a named pipe.

It gives you terminal sessions like `tmux`, but in a much simpler way without a background server.

## Wye use wye?

The main goal of wye is to work together with programs like `tmux`. It gives you a simple way to send commands and text to a terminal session from a different window or from a script.

## How to Use

#### Start a new session

To start a new session with your default shell, just run:
```sh
wye
```

#### Run a specific program

To run a different program inside the session, pass it as an argument:
```sh
wye lua
```

#### Use a specific session number

This creates a session with a number that you can easily find later.
```sh
wye -s123
```

#### Check your current session

If you are already inside a `wye` session, running `wye` again will not start a new one. It will just show you the current session number.

## How It Works

Wye manages sessions using special files called named pipes. Each session has its own pipe file located in `/tmp/wye-<session_number>`.

To send a command to a running session, you just need to write to its pipe file. For example, to send the `ls -l` command to session `123`, you can run this from another terminal:

```sh
echo "ls -l" > /tmp/wye-123
```
The command will appear in the `wye` session as if you typed it.

## Building from Source

```sh
cargo build --release
```

The final program will be located at `target/release/wye`.
