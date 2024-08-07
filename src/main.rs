use std::ffi::{OsStr, OsString};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use clap::{Args, Parser};
use users::os::unix::UserExt;

#[derive(Args, Debug)]
#[group(required = false, multiple = false)]
struct ShellArgs {
    #[arg(short = 'i', long)]
    login: bool,

    #[arg(short, long)]
    shell: bool,
}

impl ShellArgs {
    fn no_shell(&self) -> bool {
        !self.login && !self.shell
    }
}

#[derive(Parser, Debug)]
#[command(version, about)]
struct SudoArgs {
    #[arg(short = 'A', long)]
    askpass: bool,

    #[arg(short = 'B', long)]
    bell: bool,

    #[arg(short = 'b', long)]
    background: bool,

    #[arg(short = 'C', long, default_value = "3", value_name = "FD")]
    close_from: u64,

    #[arg(short = 'E')]
    preserve_all_env: bool,

    #[arg(long, action = clap::ArgAction::Append, value_name = "VAR", value_delimiter=',')]
    preserve_env: Vec<String>,

    #[arg(short, long)]
    edit: bool,

    #[arg(short, long)]
    group: Option<String>,

    #[arg(short = 'H', long)]
    set_home: bool,

    #[arg(long)]
    host: Option<String>,

    #[arg(short = 'K', long)]
    remove_timestamp: bool,

    #[arg(short = 'k', long)]
    reset_timestamp: bool,

    #[arg(short, long)]
    list: bool,

    #[arg(short = 'N', long)]
    no_update: bool,

    #[arg(short, long)]
    non_interactive: bool,

    #[arg(short = 'P', long)]
    preserve_groups: bool,

    #[arg(short, long)]
    prompt: Option<String>,

    #[arg(short = 'R', long)]
    chroot: Option<PathBuf>,

    #[arg(short = 'S', long)]
    stdin: bool,

    #[arg(short = 'U', long)]
    other_user: Option<String>,

    #[arg(short = 'T', long)]
    command_timeout: Option<String>,

    #[arg(short, long)]
    user: Option<String>,

    #[arg(short = 'D', long)]
    chdir: Option<PathBuf>,

    #[arg(short, long)]
    validate: bool,

    #[command(flatten)]
    shell: ShellArgs,
}

macro_rules! exit {
    ($($tok:tt)*) => {{
        eprintln!($($tok)*);
        std::process::exit(1)
    }}
}

fn main() {
    let mut cmd = std::process::Command::new("run0");
    cmd.arg("--background=");

    let mut raw_args = std::env::args_os().peekable();
    let arg0 = raw_args.next().unwrap_or_else(|| OsString::from("sudo"));
    let args = SudoArgs::parse_from(
        std::iter::once(arg0).chain(
            std::iter::from_fn(|| {
                raw_args.next_if(|s| {
                    let prefix = OsStr::new("-");
                    s.as_encoded_bytes().starts_with(prefix.as_encoded_bytes())
                })
            })
            .take_while(|s| s != "--"),
        ),
    );

    let Ok(command): Result<Vec<String>, _> = raw_args.map(|a| a.into_string()).collect() else {
        exit!("failed to parse arguments as utf8");
    };

    // Unsupported/validation

    if args.askpass && std::env::var_os("SUDO_ASKPASS").is_some() {
        exit!("custom askpass programs are unsupported")
    }

    if args.close_from != 3 {
        exit!(
            "close-from must be exactly 3 or unspecified, was {}",
            args.close_from
        )
    }

    if args.edit {
        exit!("editing is not supported")
    }

    if args.list {
        exit!("listing privileges is unsupported")
    }

    if args.other_user.is_some() {
        exit!("listing privileges of other users is unsupported")
    }

    if args.no_update {
        exit!("cached credentials are always updated")
    }

    if args.preserve_groups {
        exit!("cannot preserve groups")
    }

    if args.stdin {
        exit!("cannot use stdin/stderr for the password prompt")
    }

    if args.prompt.is_some() || std::env::var_os("SUDO_PROMPT").is_some() {
        exit!("password prompt cannot be overridden")
    }

    if args.validate {
        exit!("cannot validate credentials")
    }

    if args.preserve_all_env {
        exit!("you may not preserve the entire environment, you cretin!")
    }

    // Unimplemented

    if args.background {
        exit!("cannot run commands in the background")
    }

    if args.remove_timestamp || args.reset_timestamp {
        exit!("cannot alter sudo timestamps")
    }

    if args.chroot.is_some() {
        exit!("chroot is unimplemented")
    }

    if args.command_timeout.is_some() {
        exit!("command timeouts are unimplemented")
    }

    // Flags

    if let Some(dir) = &args.chdir {
        cmd.arg("-D").arg(dir);
    }

    for var in &args.preserve_env {
        cmd.arg(format!("--setenv={var}"));
    }

    // XXX: parse GID/UID!
    if let Some(group) = &args.group {
        cmd.arg("-g").arg(group);
    }

    if let Some(user) = &args.user {
        cmd.arg("-u").arg(user);
    }

    if let Some(host) = &args.host {
        cmd.arg(format!("--machine={host}"));
    }

    if args.non_interactive {
        cmd.arg("--no-ask-password");
    }

    if args.preserve_groups {
        cmd.arg("--no-ask-password");
    }

    cmd.arg("--");

    if args.shell.no_shell() {
        if command.is_empty() {
            exit!("must specify --login, --shell, or a COMMAND")
        }

        cmd.args(command);
    } else {
        let Some(user_shell) = (if args.shell.login {
            args.user
                .as_deref()
                .map(users::get_user_by_name)
                .unwrap_or_else(|| users::get_user_by_uid(0))
                .map(|u| u.shell().as_os_str().to_owned())
        } else if args.shell.shell {
            std::env::var_os("SHELL").or_else(|| {
                users::get_user_by_uid(users::get_current_uid())
                    .map(|u| u.shell().as_os_str().to_owned())
            })
        } else {
            panic!("BUG")
        }) else {
            exit!("failed to lookup the target user's shell")
        };
        cmd.arg(user_shell);
        if args.shell.login {
            cmd.arg("--login");
        }

        if !command.is_empty() {
            cmd.arg("-c").arg(shell_escape(&command));
        }
    }

    let err = cmd.exec();
    exit!("failed to execute command: {err}")
}

fn shell_escape_arg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if !matches!(c, '_' | '-' | '$') && !c.is_ascii_alphanumeric() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn shell_escape(cmd: impl IntoIterator<Item: AsRef<str>>) -> String {
    cmd.into_iter()
        .map(|s| shell_escape_arg(s.as_ref()))
        .collect::<Vec<_>>()
        .join(" ".as_ref())
}
