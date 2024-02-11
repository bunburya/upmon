use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{stdout, Write};
use async_std::sync::Mutex;
use chrono::{SecondsFormat, Utc};
use crate::upower::Property;

/// A trait for writing changed properties in some way.
pub(crate) trait Writer {
    /// Write the given changes.
    async fn write(&self, device_path: &str, changes: &HashMap<&str, Property>)
        -> Result<(), std::io::Error>;
}

/// A [`Writer`] that outputs details of all changed properties on a single line, per DBus message
/// per device.
pub struct LineWriter {
    /// File (or other struct implementing Write) to write to.
    out: Mutex<Box<dyn Write>>,
    /// String used to separate each property name from its value in the output.
    separator: String,
    /// String used to separate property-value pairs in the output.
    delimiter: String,
    /// Whether to include a timestamp in the output.
    timestamp: bool
}

impl LineWriter {
    /// Create a new [`LineWriter`] with the given configuration.
    pub(crate) fn new(
        out_path: Option<&str>,
        separator: &str,
        delimiter: &str,
        timestamp: bool
    ) -> Result<Self, std::io::Error> {
        let out: Box<dyn Write> = match out_path {
            Some(p) => Box::new(OpenOptions::new().create(true).append(true).open(p)?),
            None => Box::new(stdout())
        };
        Ok(Self {
            out: Mutex::new(out),
            separator: String::from(separator),
            delimiter: String::from(delimiter),
            timestamp
        })
    }
}

impl Writer for LineWriter {
    async fn write(&self, device_path: &str, changes: &HashMap<&str, Property>)
        -> Result<(), std::io::Error> {
        let mut out = self.out.lock().await;
        let prop_string = changes.iter()
            .map(|(k, v)| {
                format!("{k}{}{v}", self.separator)
            })
            .collect::<Vec<String>>()
            .join(&self.delimiter);
        let mut t_str = String::new();
        if self.timestamp {
            t_str = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            t_str.push(' ');
        }
        write!(out, "{t_str}{device_path} {prop_string}\n")?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use futures::executor::block_on;
    use crate::output::{LineWriter, Writer};
    use crate::upower;
    use crate::upower::Property::*;

    fn get_device_path() -> String {
        String::from("/org/freedesktop/UPower/devices/DisplayDevice")
    }

    /// Return a [`HashMap`] that mocks the kind returned by
    /// [`upower::DeviceConfig::collect_changes`].
    fn get_mock_changes<'s>() -> HashMap<&'s str, upower::Property> {
        let mut hm = HashMap::new();
        hm.insert("UpdateTime", UpdateTime(1707671976));
        hm.insert("Online", Online(true));
        hm.insert("TimeToEmpty", TimeToEmpty(12345));
        hm.insert("TimeToFull", TimeToFull(54321));
        hm.insert("Percentage", Percentage(54.22));
        hm.insert("IsPresent", IsPresent(false));
        hm.insert("State", State(2));
        hm
    }

    /// Test creation and basic usage of a [`LineWriter`] struct.
    #[test]
    fn test_line_writer() {
        let stdout_writer = LineWriter::new(None, "xx", "yy", true);
        assert!(stdout_writer.is_ok());
        let dev_path = get_device_path();
        let changed = get_mock_changes();
        if Path::new("/dev/null").exists() {
            let null_r = LineWriter::new(Some("/dev/null"), "a", "b", false);
            assert!(null_r.is_ok());
            let null_writer = null_r.unwrap();
            let write_result = block_on(null_writer.write(&dev_path, &changed));
            assert!(write_result.is_ok());
        }
        if Path::new("/dev/full").exists() {
            let full_r = LineWriter::new(Some("/dev/full"), "foo", "bar", true);
            assert!(full_r.is_ok());
            let full_writer = full_r.unwrap();
            let write_result = block_on(full_writer.write(&dev_path, &changed));
            assert!(write_result.is_err());
        }
    }
}
