# upmon

`upmon` is a simple command line utility to monitor and summarise changes to certain properties of power devices (such
as battery percentage, whether an AC cable is plugged in, etc). It does this by looking for messages sent over
[D-Bus](https://www.freedesktop.org/wiki/Software/dbus/) by the [UPower](https://upower.freedesktop.org/) service.
Detected changes are then written to standard output (or a specified file) in a simple, line-based format.

I wrote `upmon` because I wanted a very simple way for scripts to react to power-related events on my laptop (such as
changes to battery level) without having to poll frequently or parse the more complex output returned by some other
tools.

## Installation

Because `upmon` works by interacting with UPower, you need to be running a Linux system with the `upowerd` service
running. For now, the easiest way to install (assuming you have `cargo` installed) is to clone this repository and use
`cargo install`:

```shell
git clone https://github.com/bunburya/upmon.git
cd upmon
cargo install --path .
```

`upmon` should then appear in your `$HOME/.cargo/bin` (or wherever `cargo install` places binaries).

## Usage

`upmon` called with no arguments will do nothing. Typical usage is to provide one or more `--path` arguments, telling it
which power devices to monitor. `--path` takes two parameters: the path to the device, and a comma-seperated list of
properties to monitor.

```shell
upmon --path /org/freedesktop/UPower/devices/battery_BAT0 State,Percentage \
      --path /org/freedesktop/UPower/devices/line_power_AC Online
```

This will output lines to your terminal as changes to the specified properties are observed. For example, after
disconnecting your AC cable, you might see something like:

```
/org/freedesktop/UPower/devices/battery_BAT0 State=Discharging Percentage=81
/org/freedesktop/UPower/devices/line_power_AC Online=false
/org/freedesktop/UPower/devices/battery_BAT0 Percentage=80
```

Currently only a handful of properties are supported.  You can see a list of supported properties by passing the 
`--list-properties` argument. A full list of UPower device properties and their descriptions can be found
[here](https://upower.freedesktop.org/docs/Device.html#id-1.2.4.8.2). If there are additional properties you would like
`upmon` to support, feel free to open an issue or submit a pull request.

### Configuring output

You can configure the separator between property name and value using the `--separator` argument, and the delimiter
between different property-value pairs using the `--delimiter` argument. For example, the following command:

```shell
upmon --path /org/freedesktop/UPower/devices/battery_BAT0 UpdateTime,State,Percentage \
      --separator "::" --delimiter "||" 
```

would produce output like the following:

```
/org/freedesktop/UPower/devices/battery_BAT0 UpdateTime::2024-02-11T20:42:26Z||State::Discharging
```

You need to be careful that any separators or delimiters do not conflict with strings used in the output itself. `upmon`
does not provide any guarantees in that respect.

You can tell `upmon` to add an ISO 8601-formatted timestamp to the output with the `--timestamp` argument.

```shell
upmon --path /org/freedesktop/UPower/devices/battery_BAT0 State,Percentage \
      --timestamp 
```

will output something like:

```
2024-02-11T20:39:49.559Z /org/freedesktop/UPower/devices/battery_BAT0 State=Charging
```

Finally, you can tell `upmon` to write to a specific file, rather than standard output, by providing the `--output-file`
argument. This will open any file (whether or not it already exists) and append new lines to the end of the file.

### Other options

`upmon` has some other options not discussed here. Pass the `--help` argument for a summary of all the available
options.

## Development

`upmon` was written in Rust. It relies on a handful of well-known dependencies to handle command line argument parsing,
interaction with D-Bus and timestamp formatting. You can see these in `Cargo.toml`.

If you encounter any bugs or have any (reasonable) feature requests, feel free to file an issue.