// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

use platform_info::*;

use clap::Command;
use uucore::error::{UResult, USimpleError};
use uucore::translate;

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    let uts =
        PlatformInfo::new().map_err(|_e| USimpleError::new(1, translate!("cannot-get-system")))?;

    println!("{}", uts.machine().to_string_lossy().trim());
    Ok(())
}

pub fn uu_app() -> Command {
    uucore::util_app("arch").after_help(translate!("arch-after-help"))
}
