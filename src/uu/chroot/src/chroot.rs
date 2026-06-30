// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) NEWROOT Userspec pstatus chdir
mod error;

use crate::error::ChrootError;
use clap::{Arg, ArgAction, Command};
use std::ffi::OsStr;
use std::io::{Error, ErrorKind};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;
use uucore::entries::{Locate, Passwd, grp2gid, usr2gid, usr2uid};
use uucore::error::{UResult, UUsageError};
use uucore::fs::{MissingHandling, ResolveMode, canonicalize};
use uucore::libc::{self, setgid, setgroups, setuid};
use uucore::{format_usage, show};

use uucore::translate;

mod options {
    pub const NEWROOT: &str = "newroot";
    pub const GROUPS: &str = "groups";
    pub const USERSPEC: &str = "userspec";
    pub const COMMAND: &str = "command";
    pub const SKIP_CHDIR: &str = "skip-chdir";
}

/// A user and group specification, where each is optional.
enum UserSpec {
    NeitherGroupNorUser,
    UserOnly(String),
    GroupOnly(String),
    UserAndGroup(String, String),
}

struct Options {
    /// Path to the new root directory.
    newroot: PathBuf,
    /// Whether to change to the new root directory.
    skip_chdir: bool,
    /// List of groups under which the command will be run.
    groups: Option<Vec<String>>,
    /// The user and group (each optional) under which the command will be run.
    userspec: Option<UserSpec>,
}

/// Parse a user and group from the argument to `--userspec`.
///
/// The `spec` must be of the form `[USER][:[GROUP]]`, otherwise an
/// error is returned.
fn parse_userspec(spec: &str) -> UserSpec {
    match spec.split_once(':') {
        // ""
        None if spec.is_empty() => UserSpec::NeitherGroupNorUser,
        // "usr"
        None => UserSpec::UserOnly(spec.to_string()),
        // ":"
        Some(("", "")) => UserSpec::NeitherGroupNorUser,
        // ":grp"
        Some(("", grp)) => UserSpec::GroupOnly(grp.to_string()),
        // "usr:"
        Some((usr, "")) => UserSpec::UserOnly(usr.to_string()),
        // "usr:grp"
        Some((usr, grp)) => UserSpec::UserAndGroup(usr.to_string(), grp.to_string()),
    }
}

/// Pre-condition: `list_str` is non-empty.
fn parse_group_list(list_str: &str) -> Result<Vec<String>, ChrootError> {
    let split: Vec<&str> = list_str.split(',').collect();
    if split.len() == 1 {
        let name = split[0].trim();
        if name.is_empty() {
            Err(ChrootError::InvalidGroup(name.to_string()))
        } else {
            Ok(vec![name.to_string()])
        }
    } else if split.iter().all(|s| s.is_empty()) {
        Err(ChrootError::InvalidGroupList(list_str.to_string()))
    } else {
        let mut result = vec![];
        let mut err = false;
        for name in split {
            let trimmed_name = name.trim();
            if trimmed_name.is_empty() {
                if name.is_empty() {
                    continue;
                }
                show!(ChrootError::InvalidGroup(name.to_string()));
                err = true;
            } else {
                if trimmed_name.starts_with(char::is_numeric)
                    && trimmed_name.ends_with(|c: char| !c.is_numeric())
                {
                    show!(ChrootError::InvalidGroup(name.to_string()));
                    err = true;
                } else {
                    result.push(trimmed_name.to_string());
                }
            }
        }
        if err {
            Err(ChrootError::GroupsParsingFailed)
        } else {
            Ok(result)
        }
    }
}

impl Options {
    /// Parse parameters from the command-line arguments.
    fn from(matches: &clap::ArgMatches) -> UResult<Self> {
        let newroot = match matches.get_one::<String>(options::NEWROOT) {
            Some(v) => Path::new(v).to_path_buf(),
            None => return Err(ChrootError::MissingNewRoot.into()),
        };
        let groups = match matches.get_one::<String>(options::GROUPS) {
            None => None,
            Some(s) => {
                if s.is_empty() {
                    Some(vec![])
                } else {
                    Some(parse_group_list(s)?)
                }
            }
        };
        let skip_chdir = matches.get_flag(options::SKIP_CHDIR);
        let userspec = matches
            .get_one::<String>(options::USERSPEC)
            .map(|s| parse_userspec(s));
        Ok(Self {
            newroot,
            skip_chdir,
            groups,
            userspec,
        })
    }
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches =
        uucore::clap_localization::handle_clap_result_with_exit_code(uu_app(), args, 125)?;

    let default_shell: &'static OsStr = OsStr::new("/bin/sh");
    let default_option: &'static OsStr = OsStr::new("-i");
    let user_shell = std::env::var_os("SHELL");

    let options = Options::from(&matches)?;

    // We are resolving the path in case it is a symlink or /. or /../
    if options.skip_chdir
        && canonicalize(
            &options.newroot,
            MissingHandling::Normal,
            ResolveMode::Logical,
        )
        .ok()
        .as_deref()
        .and_then(|p| p.to_str())
            != Some("/")
    {
        return Err(UUsageError::new(
            125,
            translate!("chroot-error-skip-chdir-only-permitted"),
        ));
    }

    // Fixed: Read file metadata directly to preserve explicit filesystem errors like ENAMETOOLONG
    match std::fs::metadata(&options.newroot) {
        Ok(meta) => {
            if !meta.is_dir() {
                return Err(ChrootError::NoSuchDirectory(options.newroot).into());
            }
        }
        Err(e) => {
            return Err(if e.kind() == ErrorKind::NotFound {
                ChrootError::NoSuchDirectory(options.newroot)
            } else {
                ChrootError::NewRootMetadataFailed(options.newroot, e)
            }
            .into());
        }
    }

    let commands: Vec<&OsStr> = matches
        .get_many::<String>(options::COMMAND)
        .map_or_else(Vec::new, |v| v.map(OsStr::new).collect());

    let command = if commands.is_empty() {
        vec![
            user_shell.as_deref().unwrap_or(default_shell),
            default_option,
        ]
    } else {
        commands
    };

    assert!(!command.is_empty());
    let chroot_command = command[0];

    set_context(&options)?;

    let err = process::Command::new(chroot_command)
        .args(&command[1..])
        .exec();

    Err(if err.kind() == ErrorKind::NotFound {
        ChrootError::CommandNotFound(chroot_command.to_owned(), err)
    } else {
        ChrootError::CommandFailed(chroot_command.to_owned(), err)
    }
    .into())
}

pub fn uu_app() -> Command {
    let cmd = Command::new("chroot")
        .version(uucore::crate_version!())
        .about(translate!("chroot-about"))
        .override_usage(format_usage(&translate!("chroot-usage")))
        .infer_long_args(true)
        .trailing_var_arg(true);
    uucore::clap_localization::configure_localized_command(cmd)
        .arg(
            Arg::new(options::NEWROOT)
                .value_hint(clap::ValueHint::DirPath)
                .hide(true)
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new(options::GROUPS)
                .long(options::GROUPS)
                .overrides_with(options::GROUPS)
                .help(translate!("chroot-help-groups"))
                .value_name("GROUP1,GROUP2..."),
        )
        .arg(
            Arg::new(options::USERSPEC)
                .long(options::USERSPEC)
                .help(translate!("chroot-help-userspec"))
                .value_name("USER:GROUP"),
        )
        .arg(
            Arg::new(options::SKIP_CHDIR)
                .long(options::SKIP_CHDIR)
                .help(translate!("chroot-help-skip-chdir"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::COMMAND)
                .action(ArgAction::Append)
                .value_hint(clap::ValueHint::CommandName)
                .hide(true)
                .index(2),
        )
}

fn name_to_uid(name: &str) -> Result<libc::uid_t, ChrootError> {
    match usr2uid(name) {
        Ok(uid) => Ok(uid),
        Err(_) => name
            .parse::<libc::uid_t>()
            .map_err(|_| ChrootError::NoSuchUser),
    }
}

fn name_to_gid(name: &str) -> Result<libc::gid_t, ChrootError> {
    match grp2gid(name) {
        Ok(gid) => Ok(gid),
        Err(_) => name
            .parse::<libc::gid_t>()
            .map_err(|_| ChrootError::NoSuchGroup),
    }
}

fn supplemental_gids(uid: libc::uid_t) -> Vec<libc::gid_t> {
    match Passwd::locate(uid) {
        Err(_) => vec![],
        Ok(passwd) => passwd.belongs_to(),
    }
}

fn set_supplemental_gids(gids: &[libc::gid_t]) -> std::io::Result<()> {
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "cygwin",
        target_os = "netbsd"
    ))]
    let n = gids.len() as libc::c_int;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let n = gids.len() as libc::size_t;
    let err = unsafe { setgroups(n, gids.as_ptr()) };
    if err == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

fn set_gid(gid: libc::gid_t) -> std::io::Result<()> {
    let err = unsafe { setgid(gid) };
    if err == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

fn set_uid(uid: libc::uid_t) -> std::io::Result<()> {
    let err = unsafe { setuid(uid) };
    if err == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

enum Strategy {
    Nothing,
    FromUID(libc::uid_t, bool),
}

fn handle_missing_groups(strategy: Strategy) -> Result<(), ChrootError> {
    match strategy {
        Strategy::Nothing => Ok(()),
        Strategy::FromUID(uid, false) => {
            let gids = supplemental_gids(uid);
            if gids.is_empty() {
                Err(ChrootError::NoGroupSpecified(uid))
            } else {
                set_supplemental_gids(&gids).map_err(ChrootError::SetGroupsFailed)
            }
        }
        Strategy::FromUID(uid, true) => {
            let gids = supplemental_gids(uid);
            set_supplemental_gids(&gids).map_err(ChrootError::SetGroupsFailed)
        }
    }
}

fn set_supplemental_gids_with_strategy(
    strategy: Strategy,
    groups: Option<&Vec<String>>,
) -> Result<(), ChrootError> {
    match groups {
        None => handle_missing_groups(strategy),
        Some(groups) => {
            let mut gids = vec![];
            for group in groups {
                gids.push(name_to_gid(group)?);
            }
            set_supplemental_gids(&gids).map_err(ChrootError::SetGroupsFailed)
        }
    }
}

fn set_context(options: &Options) -> UResult<()> {
    match &options.userspec {
        None | Some(UserSpec::NeitherGroupNorUser) => {
            let strategy = Strategy::Nothing;
            set_supplemental_gids_with_strategy(strategy, options.groups.as_ref())?;
            enter_chroot(&options.newroot, options.skip_chdir)?;
        }
        Some(UserSpec::UserOnly(user)) => {
            let uid = name_to_uid(user)?;
            let gid = usr2gid(user).map_err(|_| ChrootError::NoGroupSpecified(uid))?;
            let strategy = Strategy::FromUID(uid, false);
            set_supplemental_gids_with_strategy(strategy, options.groups.as_ref())?;
            enter_chroot(&options.newroot, options.skip_chdir)?;
            set_gid(gid).map_err(|e| ChrootError::SetGidFailed(user.to_owned(), e))?;
            set_uid(uid).map_err(|e| ChrootError::SetUserFailed(user.to_owned(), e))?;
        }
        Some(UserSpec::GroupOnly(group)) => {
            let gid = name_to_gid(group)?;
            let strategy = Strategy::Nothing;
            set_supplemental_gids_with_strategy(strategy, options.groups.as_ref())?;
            enter_chroot(&options.newroot, options.skip_chdir)?;
            set_gid(gid).map_err(|e| ChrootError::SetGidFailed(group.to_owned(), e))?;
        }
        Some(UserSpec::UserAndGroup(user, group)) => {
            let uid = name_to_uid(user)?;
            let gid = name_to_gid(group)?;
            let strategy = Strategy::FromUID(uid, true);
            set_supplemental_gids_with_strategy(strategy, options.groups.as_ref())?;
            enter_chroot(&options.newroot, options.skip_chdir)?;
            set_gid(gid).map_err(|e| ChrootError::SetGidFailed(group.to_owned(), e))?;
            set_uid(uid).map_err(|e| ChrootError::SetUserFailed(user.to_owned(), e))?;
        }
    }
    Ok(())
}

fn enter_chroot(root: &Path, skip_chdir: bool) -> UResult<()> {
    rustix::process::chroot(root).map_err(|e| ChrootError::CannotEnter(root.into(), e.into()))?;
    if !skip_chdir {
        std::env::set_current_dir("/")?;
    }
    Ok(())
}
