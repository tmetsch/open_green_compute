#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use std::env;
use std::fs;
use std::path;
use std::thread;
use std::time;

use std::io::Write;

mod common;
mod config;
mod foxess;
mod fritz;
mod power;
mod weather;

/// struct to hold the fast & slow loop.
struct Loops {
    fast_loop: Vec<Box<dyn common::Sensor>>,
    slow_loop: Vec<Box<dyn common::Sensor>>,
}

/// Instantiates the rist sensor type based on the config.
fn create_sensor(name: &str, sensor_cfg: &toml::value::Table) -> Option<Box<dyn common::Sensor>> {
    match sensor_cfg["type"]
        .as_str()
        .expect("missing type information for a sensor.")
    {
        "weather" => {
            if !sensor_cfg.contains_key("url")
                || !sensor_cfg.contains_key("lat")
                || !sensor_cfg.contains_key("long")
                || !sensor_cfg.contains_key("app_id")
            {
                panic!("a weather sensor requires the following fields to be set: lat, long, app_id, and url.");
            }
            let tmp = weather::WeatherSensor::new(
                name.to_string(),
                sensor_cfg["url"]
                    .as_str()
                    .unwrap_or("https://api.openweathermap.org/data/2.5/weather")
                    .to_string(),
                sensor_cfg["lat"].as_float().unwrap_or(0.0),
                sensor_cfg["long"].as_float().unwrap_or(0.0),
                sensor_cfg["app_id"].as_str().unwrap_or("").to_string(),
            );
            Some(Box::new(tmp))
        }
        "power" => {
            if !sensor_cfg.contains_key("bus")
                || !sensor_cfg.contains_key("address")
                || !sensor_cfg.contains_key("expected_amps")
            {
                panic!("a power sensor requires the following fields to be set: bus, address, and expected_amps.");
            }
            let tmp = power::PowerSensor::new(
                name.to_string(),
                sensor_cfg["bus"]
                    .as_str()
                    .unwrap_or("/dev/i2c-0")
                    .to_string(),
                sensor_cfg["address"].as_integer().unwrap_or(64) as u8,
                sensor_cfg["expected_amps"].as_float().unwrap_or(1.0),
            );
            Some(Box::new(tmp))
        }
        "fritz" => {
            if !sensor_cfg.contains_key("url")
                || !sensor_cfg.contains_key("user")
                || !sensor_cfg.contains_key("password")
                || !sensor_cfg.contains_key("ain")
            {
                panic!("a fritz-box sensor requires the following fields to be set: url, user, password, and ain.");
            }
            let tmp = fritz::FritzSensor::new(
                name.to_string(),
                sensor_cfg["url"]
                    .as_str()
                    .unwrap_or("https://192.168.178.1")
                    .to_string(),
                sensor_cfg["user"].as_str().unwrap_or("admin").to_string(),
                sensor_cfg["password"]
                    .as_str()
                    .unwrap_or("admin")
                    .to_string(),
                sensor_cfg["ain"]
                    .as_str()
                    .unwrap_or("1122334455")
                    .to_string(),
            );
            Some(Box::new(tmp))
        }
        "foxess" => {
            if !sensor_cfg.contains_key("api_key")
                || !sensor_cfg.contains_key("inverter_id")
                || !sensor_cfg.contains_key("variables")
            {
                panic!("a FoxESS sensor requires the following fields to be set: api_key, inverter_id, variables.");
            }
            let variables: Vec<String> = sensor_cfg["variables"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|c| c.as_str().to_owned().unwrap().to_string())
                .collect();

            let tmp = foxess::FoxEssOpenAPISensor::new(
                name.to_string(),
                sensor_cfg["api_key"].as_str().unwrap_or("bar").to_string(),
                sensor_cfg["inverter_id"]
                    .as_str()
                    .unwrap_or("123")
                    .to_string(),
                variables,
                sensor_cfg["url"]
                    .as_str()
                    .unwrap_or("https://www.foxesscloud.com")
                    .to_string(),
            );
            Some(Box::new(tmp))
        }
        &_ => None,
    }
}

/// Given the configuration determine slow and fast loop sensors.
fn get_sensors(cfg: &config::Config) -> Loops {
    let mut slow_sensors: Vec<Box<dyn common::Sensor>> = Vec::new();
    let mut fast_sensors: Vec<Box<dyn common::Sensor>> = Vec::new();
    if let Some(tmp) = cfg.data["general"]["slow_loop"].as_array() {
        for item in tmp {
            let name = item.as_str().expect("no name provided.");
            let sensor_cfg = cfg.data[name].as_table().expect("no config provided.");
            if let Some(sensor) = create_sensor(name, sensor_cfg) {
                slow_sensors.push(sensor);
            }
        }
    }
    if let Some(tmp) = cfg.data["general"]["fast_loop"].as_array() {
        for item in tmp {
            let name = item.as_str().expect("no name provided.");
            let sensor_cfg = cfg.data[name].as_table().expect("no config provided.");
            if let Some(sensor) = create_sensor(name, sensor_cfg) {
                fast_sensors.push(sensor);
            }
        }
    }
    Loops {
        slow_loop: slow_sensors,
        fast_loop: fast_sensors,
    }
}

fn main() {
    // Load the configuration.
    let cfg_file: String = env::var("OGC_CONFIG").unwrap_or_else(|_| String::from("defaults.toml"));
    let cfg = config::load_config(&cfg_file);

    // figure out the sensors.
    let mut sensors = get_sensors(&cfg);

    // create CSV file if it does not exists...
    let path = cfg.data["general"]["filename"]
        .as_str()
        .unwrap_or("data.csv");
    if !path::Path::new(path).exists() {
        let mut headers = Vec::new();
        headers.push("timestamp".to_string());
        for sensor in &sensors.fast_loop {
            let heads = sensor.get_names();
            headers.extend_from_slice(&heads);
        }
        for sensor in &sensors.slow_loop {
            let heads = sensor.get_names();
            headers.extend_from_slice(&heads);
        }
        let mut output = fs::File::create(path).expect("could not create file.");
        let line = headers.join(",");
        writeln!(output, "{}", line).expect("could not write the header to CSV file.");
    }

    // the actual instrumentation loop...
    let mut j = 0;
    let mut cache: Vec<f64> = Vec::new();
    loop {
        let mut val: Vec<f64> = Vec::new();
        val.push(
            time::SystemTime::now()
                .duration_since(time::UNIX_EPOCH)
                .expect("should be a duration.")
                .as_secs_f64(),
        );
        for sensor in &mut sensors.fast_loop {
            let tmp = sensor.measure();
            val.extend(tmp);
        }
        if j == 0 {
            let mut new_cache: Vec<f64> = Vec::new();
            for sensor in &mut sensors.slow_loop {
                let tmp = sensor.measure();
                new_cache.extend(tmp);
            }
            cache.clear();
            cache.extend(new_cache.to_owned());
        }
        val.extend(cache.to_owned());
        j += 1;
        if j == cfg.data["general"]["slow_loop_delay"]
            .as_integer()
            .unwrap_or(20)
        {
            j = 0;
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("could not open file for appending data.");

        let cols_str: Vec<_> = val.iter().map(ToString::to_string).collect();
        let line = cols_str.join(",");
        if let Err(e) = writeln!(file, "{}", line) {
            eprintln!("Couldn't write to file: {}", e);
        }
        thread::sleep(time::Duration::from_secs(
            cfg.data["general"]["timeout"].as_integer().unwrap_or(30) as u64,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    const TEST_DATA: &str = "[general]\nfast_loop=[\"foo\",\"dummy\"]\nslow_loop=[\"bar\"]\nfilename=\"test.csv\"\n\n[foo]\ntype=\"power\"\nbus=\"\"\naddress=0x40\nexpected_amps=1.0\n\n[bar]\ntype=\"weather\"\nlat=0.0\nlong=0.0\napp_id=123\nurl=\"localhost\"\n\n[dummy]\ntype=\"na\"\n";
    const FAULTY_DATA: &str = "[general]\nfast_loop=[\"foo\"]\nslow_loop=[\"bar\"]\n\n";
    const SENSOR_DATA: &str = "[foo]\ntype=\"power\"\nbus=\"\"\naddress=0x40\nexpected_amps=1.0\n\n[bar]\ntype=\"weather\"\nlat=0.0\nlong=0.0\napp_id=123\nurl=\"localhost\"\n";
    const FAULTY_SENSOR: &str = "[foo]\ntype=\"power\"\n\n[bar]\ntype=\"weather\"\n";

    fn setup(filename: &str, data: &str) {
        let mut file =
            fs::File::create(filename).expect("failed to create config file for testing.");
        file.write_all(data.as_bytes())
            .expect("failed to write sample config file.");
    }

    fn tear_down(filename: &str) {
        fs::remove_file(filename).expect("failed to delete config file for testing.");
    }

    // Tests for success.

    #[test]
    fn test_get_sensors_for_success() {
        setup("for_testing0.toml", TEST_DATA);
        let cfg = config::load_config("for_testing0.toml");
        get_sensors(&cfg);
        tear_down("for_testing0.toml");
    }

    #[test]
    fn test_create_sensors_for_success() {
        setup("for_testing_0.toml", SENSOR_DATA);
        let cfg = config::load_config("for_testing_0.toml");
        create_sensor("foo", cfg.data["foo"].as_table().unwrap());
        tear_down("for_testing_0.toml");
    }

    // Tests for failure.

    #[test]
    #[should_panic]
    fn test_get_sensors_for_failure() {
        setup("for_testing1.toml", FAULTY_DATA);
        let cfg = config::load_config("for_testing1.toml");
        get_sensors(&cfg);
        tear_down("for_testing1.toml");
    }

    #[test]
    #[should_panic]
    fn test_create_sensors_foo_for_failure() {
        setup("for_testing_1.toml", FAULTY_SENSOR);
        let cfg = config::load_config("for_testing_1.toml");
        create_sensor("foo", cfg.data["foo"].as_table().unwrap());
        tear_down("for_testing_1.toml");
    }

    #[test]
    #[should_panic]
    fn test_create_sensors_bar_for_failure() {
        setup("for_testing_1.toml", FAULTY_SENSOR);
        let cfg = config::load_config("for_testing_1.toml");
        create_sensor("bar", cfg.data["bar"].as_table().unwrap());
        tear_down("for_testing_1.toml");
    }

    // Tests for sanity.

    #[test]
    fn test_get_sensors_for_sanity() {
        setup("for_testing2.toml", TEST_DATA);
        let cfg = config::load_config("for_testing2.toml");
        let res = get_sensors(&cfg);
        assert_eq!(res.slow_loop.len(), 1);
        assert_eq!(res.fast_loop.len(), 1);
        tear_down("for_testing2.toml");
    }
}
