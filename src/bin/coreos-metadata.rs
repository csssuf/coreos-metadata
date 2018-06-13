// Copyright 2017 CoreOS, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg_attr(feature="clippy", feature(plugin))]
#![cfg_attr(feature="clippy", plugin(clippy))]

#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate slog;
extern crate slog_term;
extern crate slog_async;
#[macro_use]
extern crate slog_scope;
#[macro_use]
extern crate structopt;

extern crate coreos_metadata;

use std::env;
use std::fs::File;
use std::io::prelude::*;
use slog::Drain;
use structopt::StructOpt;

use coreos_metadata::fetch_metadata;
use coreos_metadata::errors::*;

const CMDLINE_PATH: &'static str = "/proc/cmdline";
const CMDLINE_OEM_FLAG:&'static str = "coreos.oem.id";

#[derive(Debug, StructOpt)]
#[structopt(name = "coreos-metadata")]
struct Config {
    #[structopt(long = "provider")]
    /// The name of the cloud provider
    provider: Option<String>,
    #[structopt(long = "attributes")]
    /// The file into which the metadata attributes are written
    attributes_file: Option<String>,
    #[structopt(long = "ssh-keys")]
    /// Update SSH keys for the given user
    ssh_keys_user: Option<String>,
    #[structopt(long = "hostname")]
    /// The file into which the hostname should be written
    hostname_file: Option<String>,
    #[structopt(long = "network-units")]
    /// The directory into which network units are written
    network_units_dir: Option<String>,
    #[structopt(long = "cmdline")]
    /// Read the cloud provider from the kernel cmdline
    cmdline: bool,
}

quick_main!(run);

fn run() -> Result<()> {
    // setup logging
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, slog_o!());
    let _guard = slog_scope::set_global_logger(log);

    debug!("Logging initialized");

    // initialize program
    let config = init()
        .chain_err(|| "initialization")?;

    trace!("cli configuration - {:?}", config);

    // fetch the metadata from the configured provider
    let metadata = match config.provider {
        Some(provider) => fetch_metadata(&provider)
            .chain_err(|| "fetching metadata from provider")?,
        None => bail!("Must set either --provider or --cmdline"),
    };

    // write attributes if configured to do so
    config.attributes_file
        .map_or(Ok(()), |x| metadata.write_attributes(x))
        .chain_err(|| "writing metadata attributes")?;

    // write ssh keys if configured to do so
    config.ssh_keys_user
        .map_or(Ok(()), |x| metadata.write_ssh_keys(x))
        .chain_err(|| "writing ssh keys")?;

    // write hostname if configured to do so
    config.hostname_file
        .map_or(Ok(()), |x| metadata.write_hostname(x))
        .chain_err(|| "writing hostname")?;

    // write network units if configured to do so
    config.network_units_dir
        .map_or(Ok(()), |x| metadata.write_network_units(x))
        .chain_err(|| "writing network units")?;

    debug!("Done!");

    Ok(())
}

fn init() -> Result<Config> {
    // do some pre-processing on the command line arguments so that we support
    // golang-style arguments for backwards compatibility. since we have a
    // rather restricted set of flags, all without short options, we can make
    // a lot of assumptions about what we are seeing.
    let args = env::args().map(|arg| {
        if arg.starts_with("-") && !arg.starts_with("--") && arg.len() > 2 {
            format!("-{}", arg)
        } else {
            arg
        }
    });

    // setup cli
    // WARNING: if additional arguments are added, one of two things needs to
    // happen:
    //   1. don't add a shortflag
    //   2. modify the preprocessing logic above to be smarter about where it
    //      prepends the hyphens
    // the preprocessing will probably convert any short flags it finds into
    // long ones
    let mut config = Config::from_iter(args);

    if config.provider.is_none() && config.cmdline {
        config.provider = Some(get_oem()?);
    }

    Ok(config)
}

fn get_oem() -> Result<String> {
    // open the cmdline file
    let mut file = File::open(CMDLINE_PATH)
        .chain_err(|| format!("Failed to open cmdline file ({})", CMDLINE_PATH))?;

    // read the contents
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .chain_err(|| format!("Failed to read cmdline file ({})", CMDLINE_PATH))?;

    // split the contents into elements
    let params: Vec<Vec<&str>> = contents.split(' ')
        .map(|s| s.split('=').collect())
        .collect();

    // find the oem flag
    for p in params {
        if p.len() > 1 && p[0] == CMDLINE_OEM_FLAG {
            return Ok(String::from(p[1]));
        }
    }

    Err(format!("Couldn't find '{}' flag in cmdline file ({})", CMDLINE_OEM_FLAG, CMDLINE_PATH).into())
}
