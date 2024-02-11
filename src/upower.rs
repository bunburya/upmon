use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use chrono::{NaiveDateTime, SecondsFormat};
use futures::future::join_all;
use zbus::{
    Connection, MatchRule, MessageStream, MessageType, Result as zbus_Result,
    export::futures_util::TryStreamExt,
    fdo::PropertiesChanged,
    zvariant::Value::{self, F64, I64, U32, U64, Bool}
};

use Property::*;
use strum::VariantNames;
use crate::output::Writer;

/// Convert seconds to a string in the format HH:MM:SS.
fn secs_to_hhmmss(mut s: i64) -> String {
    if s <= 0 {
        return String::from("00:00:00")
    }
    let h = s / 3600;
    if h > 0 {
        s %= h * 3600;
    }
    let m = s / 60;
    if m > 0 {
        s %= m * 60;
    }
    format!("{h:02}:{m:02}:{s:02}")
}

/// Properties of the `org.freedesktop.UPower.Device` interface which can be monitored.
///
/// Only a small number of properties are currently supported; support for additional properties can
/// be implemented by adding them to this enum (and the associated functions and methods).
///
/// See https://upower.freedesktop.org/docs/Device.html#id-1.2.4.8.2 for all available properties
/// and their descriptions.
#[derive(Debug, PartialEq, VariantNames)]
pub enum Property {
    UpdateTime(u64),
    Online(bool),
    TimeToEmpty(i64),
    TimeToFull(i64),
    Percentage(f64),
    IsPresent(bool),
    State(u32)
}

impl Property {
    /// Create a ['Property'] variant from a key and value which may be returned from
    /// [`zbus::fdo::PropertiesChangedArgs::changed_properties`].
    fn from_key_value(k: &str, v: &Value) -> Result<Self, ()> {
        match (k, v) {
            ("UpdateTime", U64(t)) => Ok(UpdateTime(*t)),
            ("Online", Bool(b)) => Ok(Online(*b)),
            ("TimeToEmpty", I64(t)) => Ok(TimeToEmpty(*t)),
            ("TimeToFull", I64(t)) => Ok(TimeToFull(*t)),
            ("Percentage", F64(p)) => Ok(Percentage(*p)),
            ("IsPresent", Bool(b)) => Ok(IsPresent(*b)),
            ("State", U32(s)) => Ok(State(*s)),
            _ => Err(())
        }
    }
}

impl Display for Property {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            UpdateTime(t) => NaiveDateTime::from_timestamp_opt(*t as i64, 0)
                .expect("Could not parse datetime from UpdateTime value.")
                .and_utc()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            State(n) => match n {
                0 => String::from("Unknown"),
                1 => String::from("Charging"),
                2 => String::from("Discharging"),
                3 => String::from("Empty"),
                4 => String::from("FullyCharged"),
                5 => String::from("PendingCharge"),
                6 => String::from("PendingDischarge"),
                _ => panic!("Unexpected value for State: {n}")
            },
            TimeToEmpty(t) | TimeToFull(t) => secs_to_hhmmss(*t),
            Online(b) | IsPresent(b) => b.to_string(),
            Percentage(p) => p.to_string()
        })
    }
}

/// A single configured device path.
#[derive(Debug)]
pub struct DeviceConfig {
    /// The device's DBus object path.
    path: String,
    /// A list of properties that should be monitored for this device.
    targets: Vec<String>
}

impl DeviceConfig {
    /// Produce a single [`DeviceConfig`] from two string arguments. `path` should be the device
    /// path and `targets` should be a comma-delimited list of properties to target.
    fn new(path: &str, targets: &str) -> Result<Self, String> {
        if targets.is_empty() {
            return Err(String::from("Must specify one or more target properties to monitor."))
        }
        let targs = targets.split(",")
            .map(|s| {
                if Property::VARIANTS.contains(&s) {
                    Ok(String::from(s))
                } else {
                    Err(format!("Unexpected target property: {}", s))
                }
            })
            .collect::<Result<Vec<String>, String>>()?;
        Ok(DeviceConfig {
            path: String::from(path),
            targets: targs
        })
    }

    /// Produce a vector of [`DeviceConfig`] structs from a vector of string arguments. The vector
    /// must have an even number of items. Each pair of items will be passed to
    /// [`DeviceConfig::new`].
    pub(crate) fn from_varargs(args: &[String]) -> Result<Vec<DeviceConfig>, String> {
        let n_args = args.len();
        if n_args % 2 != 0 {
            return Err(format!("Invalid aggregate number of path arguments: {n_args}"))
        }
        let mut v: Vec<DeviceConfig> = vec!();
        let iter = args.chunks(2);
        for c in iter {
            v.push(DeviceConfig::new(&c[0], &c[1])?)
        }
        Ok(v)
    }

    /// Collect the relevant changes into a `HashMap`.
    fn collect_changes(&self, properties: &HashMap<&str, Value>) -> HashMap<&str, Property> {
        let mut changes: HashMap<&str, Property> = HashMap::new();
        for k in &self.targets {
            if let Some(v) = properties.get(k.as_str()) {
                if let Ok(p) = Property::from_key_value(k, v) {
                    changes.insert(k, p);
                }
            }
        }
        changes
    }

    /// Build and return a `MatchRule` object for this path.
    pub(crate) fn rule(&self) -> zbus_Result<MatchRule<'_>> {
        Ok(MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .path(self.path.as_str())?
            .build())
    }

    /// Listen for relevant changes to properties for this device, and write any detected changes.
    async fn listen(&self, conn: &Connection, writer: &impl Writer) -> zbus_Result<()> {
        let rule = self.rule()?;
        let mut stream = MessageStream::for_match_rule(
            rule,
            conn,
            None
        ).await?;
        loop {
            let msg = stream.try_next().await?.unwrap();
            let signal = PropertiesChanged::from_message(msg).unwrap();
            let args = signal.args()?;
            let changes = self.collect_changes(&args.changed_properties);
            if !changes.is_empty() {
                writer.write(&self.path, &changes).await?;
            }
        }
    }
}

/// Listen for relevant changes to properties for all specified devices, and write any detected
/// changes.
pub async fn listen_all(conn: &Connection, paths: &[DeviceConfig], writer: &impl Writer) {
    let mut futures = vec!();
    for p in paths {
        futures.push(p.listen(conn, writer));
    }
    join_all(futures).await;
}

#[cfg(test)]
pub(crate) mod tests {
    use zbus::zvariant::Value::{Bool, F64, I64, U32, U64};
    use crate::upower::{DeviceConfig, Property};
    use crate::upower::Property::{IsPresent, Online, Percentage, State, TimeToEmpty, TimeToFull,
                                  UpdateTime};

    /// Test creation of [`Property`] structs.
    #[test]
    fn create_property() {
        let to_test = vec!(
            (Property::from_key_value("UpdateTime", &U64(1707671976)), UpdateTime(1707671976)),
            (Property::from_key_value("Online", &Bool(true)), Online(true)),
            (Property::from_key_value("TimeToEmpty", &I64(12345)), TimeToEmpty(12345)),
            (Property::from_key_value("TimeToFull", &I64(54321)), TimeToFull(54321)),
            (Property::from_key_value("Percentage", &F64(54.22)), Percentage(54.22)),
            (Property::from_key_value("IsPresent", &Bool(false)), IsPresent(false)),
            (Property::from_key_value("State", &U32(2)), State(2))
        );
        for (actual, expected) in to_test {
            assert!(actual.is_ok());
            assert_eq!(actual.unwrap(), expected);
        }
        assert!(Property::from_key_value("SomeBadKey", &U32(2)).is_err());
        assert!(Property::from_key_value("UpdateTime", &Bool(true)).is_err());
    }

    /// Test creation of single [`DeviceConfig`] structs.
    #[test]
    fn create_device_config() {
        let dev_path = "/org/freedesktop/UPower/devices/DisplayDevice";

        let single_r = DeviceConfig::new(dev_path, "TimeToFull");
        assert!(single_r.is_ok());
        let single = single_r.unwrap();
        assert_eq!(single.path, dev_path);
        assert_eq!(single.targets, vec!(String::from("TimeToFull")));

        let multi_r = DeviceConfig::new(dev_path, "Online,State,Percentage");
        assert!(multi_r.is_ok());
        let multi = multi_r.unwrap();
        assert_eq!(multi.path, dev_path);
        assert_eq!(
            multi.targets,
            vec!(
                String::from("Online"),
                String::from("State"),
                String::from("Percentage")
            )
        );

        let zero_r = DeviceConfig::new(dev_path, "");
        println!("{zero_r:?}");
        assert!(zero_r.is_err());

        let invalid = DeviceConfig::new(dev_path, "Online,BadTarget");
        assert!(invalid.is_err());
    }

    /// Test creation of multiple [`DeviceConfig`] structures using the
    /// [`DeviceConfig::from_varargs`] function.
    #[test]
    fn multi_device_configs() {
        let good_args: Vec<String> = [
            "/org/freedesktop/UPower/devices/DisplayDevice", "IsPresent,Percentage",
            "/org/freedesktop/UPower/devices/line_power_AC", "Online"
        ].iter().map(|s| String::from(*s)).collect();
        let confs_r = DeviceConfig::from_varargs(&good_args);
        assert!(confs_r.is_ok());
        let confs = confs_r.unwrap();
        assert_eq!(confs.len(), 2);

        let bad_number_args: Vec<String> = [
            "/org/freedesktop/UPower/devices/DisplayDevice", "IsPresent,Percentage",
            "/org/freedesktop/UPower/devices/line_power_AC"
        ].iter().map(|s| String::from(*s)).collect();
        let confs_r = DeviceConfig::from_varargs(&bad_number_args);
        assert!(confs_r.is_err());

        let invalid_args: Vec<String> = [
            "/org/freedesktop/UPower/devices/DisplayDevice", "IsPresent,BadTarget",
            "/org/freedesktop/UPower/devices/line_power_AC", "Online"
        ].iter().map(|s| String::from(*s)).collect();
        let confs_r = DeviceConfig::from_varargs(&invalid_args);
        assert!(confs_r.is_err());
    }

    /// Test creation of [`zbus::MatchRule`] structs.
    #[test]
    fn rules() {
        let dev_conf = DeviceConfig::new(
            "/org/freedesktop/UPower/devices/DisplayDevice",
            "TimeToFull"
        ).unwrap();
        let rule_r = dev_conf.rule();
        assert!(rule_r.is_ok());
        let rule = rule_r.unwrap();
        let rule_str = "type='signal',interface='org.freedesktop.DBus.Properties',\
                            member='PropertiesChanged',\
                            path='/org/freedesktop/UPower/devices/DisplayDevice'";
        assert_eq!(rule.to_string(), rule_str);
    }
}