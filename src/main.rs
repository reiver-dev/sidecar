//! Main

mod child;
mod child_watcher;
mod debug;
mod guards;
mod messages;
mod pipe;
mod raw;
mod runtime;
mod signals;
mod socket;
mod system;
mod tty;

mod client;
mod server;
mod stop;

use std::io::{Result, Write};
use std::path::PathBuf;

use crate::system::{signal_from_str, Signal};
use gumdrop::{Options, ParsingStyle};
use log::{self, error, Level, Log};

const NAME: &str = env!("CARGO_PKG_NAME");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Options)]
struct Cli {
    #[options(help = "print help message and exit")]
    help: bool,

    #[options(count, help = "enable debug messages (up to 3)")]
    verbose: u32,

    #[options(help = "output version information and exit")]
    version: bool,

    #[options(command)]
    command: Option<Command>,
}

#[derive(Debug, Options)]
enum Command {
    /// Start server and wait for commands
    Start(StartCommand),

    /// Stop running server
    Stop(StopCommand),

    /// Execute command on server
    Exec(ExecCommand),
}

/// Start server and wait for commands
#[derive(Debug, Options)]
struct StartCommand {
    #[options(help = "print help message and exit")]
    help: bool,

    #[options(count, help = "enable debug messages (up to 3)")]
    verbose: u32,

    #[options(help = "make parent directories as needed")]
    parents: bool,

    #[options(help = "set the effective user ID", meta = "UID", no_short)]
    setuid: Option<u32>,

    #[options(help = "sets the effective group ID", meta = "GID", no_short)]
    setgid: Option<u32>,

    #[options(help = "start server as a new session", no_short)]
    setsid: bool,

    #[options(
        help = "assign server to a process group (0 for leader)",
        meta = "PGID",
        no_short
    )]
    setpgid: Option<i32>,

    #[options(help = "detach process from /dev/tty", no_short)]
    notty: bool,

    #[options(help = "server socket location", free)]
    path: PathBuf,
}

/// Stop running server
#[derive(Debug, Options)]
struct StopCommand {
    #[options(help = "print help message and exit")]
    help: bool,

    #[options(count, help = "enable debug messages (up to 3)")]
    verbose: u32,

    #[options(help = "server socket location", free)]
    path: PathBuf,
}

/// Execute command on server
#[derive(Debug, Options)]
struct ExecCommand {
    #[options(help = "print help message and exit")]
    help: bool,

    #[options(count, help = "enable debug messages (up to 3)")]
    verbose: u32,

    #[options(help = "server socket location")]
    connect: PathBuf,

    #[options(
        help = "set each NAME to VALUE in the environment",
        meta = "NAME=VALUE"
    )]
    env: Vec<String>,

    #[options(help = "change working directory to DIR", meta = "DIR")]
    workdir: String,

    #[options(
        help = "set user id",
        default_expr = "-1",
        meta = "UID",
        no_short
    )]
    setuid: i32,

    #[options(
        help = "set group id",
        default_expr = "-1",
        meta = "GID",
        no_short
    )]
    setgid: i32,

    #[options(
        help = "set process group (0 to become leader)",
        meta = "PGID",
        no_short
    )]
    setpgid: Option<i32>,

    #[options(help = "run program in a new session", no_short)]
    setsid: bool,

    #[options(help = "detach from /dev/tty", no_short)]
    notty: bool,

    #[options(
        help = "deliver the signal when parent process exits",
        default_expr = "Signal::SIGKILL",
        no_short,
        parse(try_from_str = "signal_from_str")
    )]
    deathsig: Signal,

    #[options(help = "program arguments to execute", free)]
    program: Vec<String>,
}

fn env_to_kv(arg: &str) -> (&str, &str) {
    for (i, val) in arg.bytes().enumerate() {
        if val == b'=' {
            return (
                std::str::from_utf8(&arg.as_bytes()[..i]).unwrap(),
                std::str::from_utf8(&arg.as_bytes()[i + 1..]).unwrap(),
            );
        }
    }
    (arg, &arg[arg.len()..arg.len()])
}

fn command_exec(arg: &ExecCommand) -> Result<i32> {
    if arg.program.is_empty() {
        return Ok(0);
    }

    if arg.connect.as_os_str().is_empty() {
        return command_exec_local(&arg);
    }

    system::disable_inherit_stdio()?;

    // let program: &str = &arg.program[0];
    let args: Vec<&str> =
        arg.program[1..].iter().map(|s| s.as_ref()).collect();
    let envs: Vec<_> = arg.env.iter().map(|s| env_to_kv(&s)).collect();

    client::command(&client::Args {
        program: &arg.program[0],
        args: args.as_slice(),
        env: envs.as_slice(),
        cwd: &arg.workdir,
        connect: arg.connect.as_path(),
        uid: arg.setuid,
        gid: arg.setgid,
        deathsig: arg.deathsig as i32,
        setpgid: arg.setpgid,
        setsid: arg.setsid,
        notty: arg.notty,
    })
}

fn command_exec_local(arg: &ExecCommand) -> Result<i32> {
    use crate::messages::{Files, ProcessRequest, StartMode};

    if arg.program.is_empty() {
        return Ok(0);
    }

    let args: Vec<&str> =
        arg.program[1..].iter().map(|s| s.as_ref()).collect();
    let envs: Vec<_> = arg.env.iter().map(|s| env_to_kv(&s)).collect();

    let mut startup = StartMode::empty();
    let pgid = match arg.setpgid {
        Some(id) => {
            startup |= StartMode::PROCESS_GROUP;
            id
        }
        None => 0,
    };

    if arg.setsid {
        startup |= StartMode::SESSION;
    }

    if arg.notty {
        startup |= StartMode::DETACH_TERMINAL;
    }

    let req = ProcessRequest {
        program: &arg.program[0],
        argv: &args,
        cwd: &arg.workdir,
        env: &envs,
        startup: startup,
        io: Files::all(),
        pgid: pgid,
        uid: arg.setuid,
        gid: arg.setgid,
        deathsig: arg.deathsig as i32,
    };

    Err(child::execute_into(&req))
}

fn command_start(arg: &StartCommand) -> i32 {
    if let Err(e) = system::disable_inherit_stdio() {
        error!("stdio CLOEXEC: {}", e);
        return 1;
    }

    if arg.notty {
        if let Err(e) = tty::disconnect_controlling_terminal() {
            error!("notty(): {}", e);
            return 1;
        }
    }

    if let Some(pgid) = arg.setpgid {
        let id = system::Pid::from_raw(pgid);
        if let Err(e) = system::new_process_group(id) {
            error!("setpgid({}): {}", pgid, e);
            return 1;
        }
    }

    if arg.setsid {
        if let Err(e) = system::new_session() {
            error!("setsid() {}", e);
            return 1;
        }
    }

    if arg.path.as_os_str().is_empty() {
        return 0;
    }

    if arg.parents {
        if let Some(parent) = arg.path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("mkdir({:?}) {}", parent, e);
                return 1;
            }
        }
    }

    if let Some(uid) = arg.setuid {
        if let Err(e) = nix::unistd::setuid(nix::unistd::Uid::from_raw(uid)) {
            error!("setuid({}) {}", uid, raw::nixerror(e));
            return 1;
        }
    }

    if let Some(gid) = arg.setgid {
        if let Err(e) = nix::unistd::setgid(nix::unistd::Gid::from_raw(gid)) {
            error!("setgid({}) {}", gid, raw::nixerror(e));
            return 1;
        }
    }

    match server::command(&server::Args {
        server: arg.path.as_path(),
    }) {
        Ok(code) => code,
        Err(e) => {
            error!("start() {}", e);
            1
        }
    }
}

fn command_stop(arg: &StopCommand) -> Result<i32> {
    if arg.path.as_os_str().is_empty() {
        return Ok(0);
    }
    stop::command(&stop::Args {
        connect: arg.path.as_path(),
    })
}

struct Logger {
    own: Level,
    others: Level,
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        if metadata.target().starts_with("sidecar") {
            metadata.level() <= self.own
        } else {
            metadata.level() <= self.others
        }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let lch = match record.level() {
            Level::Error => "[E ",
            Level::Warn => "[W ",
            Level::Info => "[I ",
            Level::Debug => "[D ",
            Level::Trace => "[T ",
        };

        let lout = std::io::stderr();
        let mut out = lout.lock();
        writeln!(&mut out, "{}{}] {}", lch, record.target(), record.args())
            .unwrap();
    }

    fn flush(&self) {
        //
    }
}

fn configure_log(verbosity: u32) {
    let filter: (Level, Level) = match verbosity {
        0 => (Level::Warn, Level::Warn),
        1 => (Level::Info, Level::Warn),
        2 => (Level::Debug, Level::Info),
        3 => (Level::Debug, Level::Debug),
        _ => (Level::Trace, Level::Trace),
    };

    let logger = Logger {
        own: filter.0,
        others: filter.1,
    };
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(filter.0.to_level_filter())
}

fn usage_line(dest: &mut impl Write, name: &str, command: &str) -> Result<()> {
    let line = match command {
        "start" => "[OPTIONS] PATH",
        "stop" => "PATH",
        "exec" => "[OPTIONS] [PROGRAM [ARG]...]",
        _ => "[OPTIONS] COMMAND",
    };
    writeln!(dest, "Usage: {} {}", name, line)
}

fn header_line(dest: &mut impl Write, command: &str) -> Result<()> {
    if command.is_empty() {
        write!(dest, "{} {}\n{}\n\n", NAME, VERSION, AUTHORS)
    } else {
        write!(dest, "{}-{} {}\n{}\n\n", NAME, command, VERSION, AUTHORS)
    }
}

fn help(dest: &mut impl Write, name: &str, cli: &Cli) -> Result<()> {
    match cli.command_name() {
        None => {
            usage_line(dest, name, "")?;
            header_line(dest, "")?;
            write!(dest, "{}\n\n{}\n", DESCRIPTION, Cli::usage())?;
            if let Some(cmds) = Cli::command_list() {
                writeln!(dest, "\nCommands:\n{}", cmds)
            } else {
                writeln!(dest)
            }
        }
        Some(cmd) => {
            usage_line(dest, name, cmd)?;
            header_line(dest, cmd)?;
            writeln!(dest, "{}", Cli::command_usage(cmd).unwrap_or_default())
        }
    }
}

fn run() -> i32 {
    let args = std::env::args().collect::<Vec<_>>();
    let arg0 = &args[0];

    let cli = {
        match Cli::parse_args(&args[1..], ParsingStyle::default()) {
            Ok(val) => val,
            Err(err) => {
                eprintln!("{}: {}", arg0, err);
                return 2;
            }
        }
    };

    if cli.version {
        println!("{}", VERSION);
        return 0;
    }

    if cli.help_requested() {
        let _ = help(&mut std::io::stdout().lock(), &arg0, &cli);
        return 0;
    }

    let mut verbose = cli.verbose;
    match cli.command {
        Some(cmd) => match cmd {
            Command::Start(ref arg) => {
                verbose += arg.verbose;
                configure_log(verbose);
                command_start(arg)
            }
            Command::Stop(ref arg) => {
                verbose += arg.verbose;
                configure_log(verbose);
                match command_stop(arg) {
                    Ok(code) => code,
                    Err(err) => {
                        error!("{}: failed to stop server\n{}", arg0, err);
                        1
                    }
                }
            }
            Command::Exec(ref arg) => {
                verbose += arg.verbose;
                configure_log(verbose);
                match command_exec(arg) {
                    Ok(ret) => ret,
                    Err(err) => {
                        error!(
                            "{}: failed to execute command: \"{}\"\n{}",
                            arg0,
                            arg.program
                                .get(0)
                                .map(|s| s.as_str())
                                .unwrap_or(""),
                            err
                        );
                        128
                    }
                }
            }
        },
        None => {
            let stream = std::io::stderr();
            let mut stderr = stream.lock();
            let _ = writeln!(&mut stderr, "{}: missing command", arg0);
            0
        }
    }
}

fn main() {
    std::process::exit(run());
}
