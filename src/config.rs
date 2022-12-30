use std::collections;
use std::fs;

/// Struct holding the config info.
pub(crate) struct Config {
    pub(crate) data: collections::HashMap<String, toml::Value>,
}

/// Load the configuration.
pub(crate) fn load_config(filename: &str) -> Config {
    let contents: String = read_config(filename);
    let data: collections::HashMap<String, toml::Value> = get_config(contents);
    Config { data }
}

/// Reads a string from a given filename.
fn read_config(filename: &str) -> String {
    match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(err) => {
            panic!("Could not read Config file: {}: {}", filename, err);
        }
    }
}

/// Parses the configuration from a string.
fn get_config(contents: String) -> collections::HashMap<String, toml::Value> {
    let map: collections::HashMap<String, toml::Value> = match toml::from_str(&contents) {
        Ok(map) => map,
        Err(err) => {
            panic!("Could not parse the Config file: {}.", err)
        }
    };
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for success.

    #[test]
    fn test_load_config_for_success() {
        load_config("defaults.toml");
    }

    #[test]
    fn test_read_config_for_success() {
        read_config("defaults.toml");
    }

    #[test]
    fn test_get_config_for_success() {
        let contents: String = read_config("defaults.toml");
        get_config(contents);
    }

    // Tests for failure.

    #[test]
    #[should_panic]
    fn test_read_config_for_failure() {
        read_config("foo.bar");
    }

    #[test]
    #[should_panic]
    fn test_get_config_for_failure() {
        get_config("foo".to_string());
    }

    // Tests for sanity.

    #[test]
    fn test_load_config_for_sanity() {
        let cfg: Config = load_config("defaults.toml");
        assert_eq!(cfg.data.contains_key("general"), true);
        assert_eq!(
            cfg.data["general"]
                .as_table()
                .unwrap()
                .contains_key("fast_loop"),
            true
        );
        assert_eq!(
            cfg.data["general"]
                .as_table()
                .unwrap()
                .contains_key("slow_loop"),
            true
        );
    }

    #[test]
    fn test_read_config_for_sanity() {
        let res: String = read_config("defaults.toml");
        assert_ne!(res.len(), 0);
    }

    #[test]
    fn test_get_config_for_sanity() {
        let contents: String = read_config("defaults.toml");
        let res = get_config(contents);
        assert_eq!(
            res["general"]["slow_loop"].as_array().unwrap(),
            &vec![toml::Value::String("owa".parse().unwrap())]
        );
    }
}
