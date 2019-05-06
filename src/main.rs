use clap::{
    crate_authors, crate_description, crate_version, App, AppSettings, Arg,
    ArgMatches, SubCommand,
};
use log::{self, error, Level, Log};
use std::str::FromStr;

mod client;
mod messages;
mod net;
mod server;

fn configure_arguments() -> App<'static, 'static> {
    let help_msg = "display this help and exit";
    let version_msg = "output version information and exit";
    App::new("sidecar")
        .author(crate_authors!())
        .version(crate_version!())
        .about(crate_description!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::DisableHelpSubcommand)
        .setting(AppSettings::GlobalVersion)
        .setting(AppSettings::VersionlessSubcommands)
        .help_message(help_msg)
        .version_message(version_msg)
        .arg(
            clap::Arg::with_name("verbosity")
                .short("v")
                .long("verbose")
                .help("enable debug messages")
                .multiple(true)
                .global(true),
        )
        .subcommand(
            SubCommand::with_name("exec")
                .setting(AppSettings::TrailingVarArg)
                .about("Execute command on a server")
                .help_message(help_msg)
                .arg(
                    Arg::with_name("connect")
                        .short("c")
                        .long("connect")
                        .value_name("PATH")
                        .takes_value(true)
                        .help("server socket location"),
                )
                .arg(
                    Arg::with_name("env")
                        .short("e")
                        .long("env")
                        .value_name("NAME=VALUE")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple(true)
                        .help("set each NAME to VALUE in the environment"),
                )
                .arg(
                    Arg::with_name("workdir")
                        .help("change working directory to DIR")
                        .short("w")
                        .long("workdir")
                        .value_name("DIR")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("program")
                        .value_name("COMMAND")
                        .help("argv to execute")
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("serve")
                .about("Start server and wait for commands")
                .help_message(help_msg)
                .arg(
                    Arg::with_name("parents")
                        .short("p")
                        .long("parents")
                        .help("make parent directories as needed"),
                )
                .arg(
                    Arg::with_name("path")
                        .value_name("PATH")
                        .help("server socket location")
                        .required(true)
                        .index(1),
                ),
        )
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

fn command_exec(matches: &ArgMatches) -> Result<i32, std::io::Error> {
    let program: Vec<&str> = match matches.values_of("program") {
        Some(p) => p.collect(),
        None => {
            return Ok(0);
        }
    };

    let connect = matches.value_of("connect").expect("socket path is missing");
    let socketpath = std::path::PathBuf::from_str(connect).unwrap();
    let envs: Vec<_> = if let Some(values) = matches.values_of("env") {
        values.map(env_to_kv).collect()
    } else {
        Vec::new()
    };
    let cwd = matches.value_of("workdir");

    client::command(&client::Args {
        connect: socketpath.as_path(),
        program: program.as_slice(),
        env: &envs,
        cwd,
    })
}

fn command_server(matches: &ArgMatches) -> Result<(), std::io::Error> {
    let connect = matches.value_of("path").unwrap();
    let sockpath = std::path::PathBuf::from_str(connect).unwrap();
    if matches.is_present("parents") {
        if let Some(parent) = sockpath.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }
    server::command(&server::Args {
        server: sockpath.as_path(),
    })?;
    Ok(())
}

struct Logger {
    others: bool,
    level: Level,
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
            && (self.others || metadata.target().starts_with("sidecar"))
    }

    fn log(&self, record: &log::Record) {
        use std::io::Write;

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

fn configure_log(verbosity: u64) {
    let filter: (bool, Level) = match verbosity {
        0 => (false, Level::Info),
        1 => (false, Level::Debug),
        2 => (true, Level::Debug),
        _ => (true, Level::Trace),
    };

    let logger = Logger {
        others: filter.0,
        level: filter.1,
    };
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(filter.1.to_level_filter())
}

fn run() -> i32 {
    let matches = configure_arguments().get_matches();
    let verbosity = matches.occurrences_of("verbosity");
    configure_log(verbosity);

    match matches.subcommand() {
        ("exec", Some(rest)) => match command_exec(rest) {
            Ok(ret) => ret,
            Err(err) => {
                error!("failed to execute command {:?}", err);
                128
            }
        },
        ("serve", Some(rest)) => match command_server(rest) {
            Ok(()) => 0,
            Err(err) => {
                error!("failed to spawn server {:?}", err);
                1
            }
        },
        (some, maybe_rest) => {
            error!("unknown command {:?} -- {:?}", some, maybe_rest);
            -1
        }
    }
}

fn main() {
    std::process::exit(run());
}
