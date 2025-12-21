// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) ttyname filedesc

use clap::{Arg, ArgAction, Command};
use std::io::{IsTerminal, Write};
use uucore::error::{UResult, set_exit_code};

use uucore::translate;

mod options {
    pub const SILENT: &str = "silent";
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result_with_exit_code(uu_app(), args, 2)?;

    let silent = matches.get_flag(options::SILENT);

    // If silent, we don't need the name, only whether or not stdin is a tty.
    if silent {
        return if std::io::stdin().is_terminal() {
            Ok(())
        } else {
            Err(1.into())
        };
    }

    let mut stdout = std::io::stdout();

    let name = nix::unistd::ttyname(std::io::stdin());

    let write_result = match name {
        Ok(name) => writeln!(stdout, "{}", name.display()),
        Err(_) => {
            set_exit_code(1);
            writeln!(stdout, "{}", translate!("tty-not-a-tty"))
        }
    };

    if write_result.is_err() || stdout.flush().is_err() {
        // Don't return to prevent a panic later when another flush is attempted
        // because the `uucore_procs::main` macro inserts a flush after execution for every utility.
        std::process::exit(3);
    }

    Ok(())
}

pub fn uu_app() -> Command {
    uucore::util_app("tty").arg(
        Arg::new(options::SILENT)
            .long(options::SILENT)
            .visible_alias("quiet")
            .short('s')
            .help(translate!("tty-help-silent"))
            .action(ArgAction::SetTrue),
    )
}
