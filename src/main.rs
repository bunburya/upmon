use std::process::exit;
use clap::{crate_version, Parser};
use strum::VariantNames;
use zbus::Connection;
use crate::output::LineWriter;
use crate::upower::{DeviceConfig, listen_all, Property};

mod upower;
mod output;

/// Command line app to monitor UPower devices over DBus for changes to certain properties, and
/// output a summary of those changes in an easily parsable format.
#[derive(Parser)]
#[command(about, version = crate_version!())]
struct CliArgs {
    /// Specify a single device path to monitor. This can be specified multiple times. The path must
    /// be to a device that implements the org.freedesktop.UPower.Device interface. The first
    /// parameter is the path to the device and the second is a comma-delimited list of properties
    /// to monitor.
    #[arg(short, long, num_args = 2, value_names = ["PATH", "PROPERTIES"])]
    path: Vec<String>,
    /// Print the list of properties that upmon can monitor and exit.
    #[arg(short, long)]
    list_properties: bool,
    /// Path to file to write output to. If not provided, output is written to standard output.
    #[arg(short, long)]
    output_file: Option<String>,
    /// String used to separate each changed property from its new value in the output.
    #[arg(short, long, default_value = "=")]
    separator: String,
    #[arg(short, long, default_value = " ")]
    /// String used to delimit each changed property-value pair in the output.
    delimiter: String,
    /// Print the DBus rules generated for the given device paths and exit.
    #[arg(short, long)]
    rules: bool,
    /// Include an ISO 8601-formatted timestamp in the output.
    #[arg(short, long)]
    timestamp: bool
}

#[async_std::main]
async fn main() {
    let cli = CliArgs::parse();
    if cli.list_properties {
        for p in Property::VARIANTS {
            println!("{p}");
        }
        exit(0)
    }

    let path_confs = DeviceConfig::from_varargs(&cli.path)
        .unwrap_or_else(|e| {
            eprintln!("Error when reading device configuration: {e}");
            exit(1)
        });

    if cli.rules {
        for p in path_confs {
            println!("{}", p.rule().unwrap_or_else(|e| {
                eprintln!("Could not create DBus rule for path: {e}");
                exit(1)
            }).to_string());
        }
        exit(0)
    }

    let writer = LineWriter::new(
        cli.output_file.as_deref(),
        &cli.separator,
        &cli.delimiter,
        cli.timestamp
    ).unwrap_or_else(|e| {
        eprintln!("Error creating writer: {e}");
        exit(1)
    });

    let conn = Connection::system().await.unwrap_or_else(|e| {
        eprintln!("Error when reading path configuration: {e}");
        exit(1)
    });

    match DeviceConfig::from_varargs(&cli.path) {
        Ok(path_confs) => listen_all(&conn, &path_confs, &writer).await,
        Err(e) => {
            eprintln!("Error when reading path configuration: {e}");
            exit(1)
        }
    }
}
