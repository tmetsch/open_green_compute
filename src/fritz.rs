use std::error::Error;
use std::io::Read;

use md5::Digest;
use serde::Deserialize;

use crate::common;

const METRICS: [&str; 3] = ["power", "energy", "temperature"];

pub struct FritzSensor {
    name: String,
    url: String,
    user: String,
    password: String,
    ain: String,
    client: reqwest::blocking::Client,
}

#[derive(Deserialize)]
struct LoginResponse {
    #[serde(rename = "SID")]
    sid: String,
    #[serde(rename = "Challenge")]
    challenge: String,
}

impl FritzSensor {
    pub fn new(
        name: String,
        url: String,
        user: String,
        password: String,
        ain: String,
    ) -> FritzSensor {
        let builder: reqwest::blocking::ClientBuilder = reqwest::blocking::ClientBuilder::new();
        let client = builder.danger_accept_invalid_certs(true).build().unwrap();
        FritzSensor {
            name,
            url,
            user,
            password,
            ain,
            client,
        }
    }

    fn get_token(&self) -> Result<String, Box<dyn Error>> {
        // retrieve a token.
        let url = format!("{}/login_sid.lua", self.url);
        let mut res = self.client.get(url).send()?;
        if res.status() != 200 {
            return Err(
                Box::try_from("Status code was not 200 when retrieving the challenge.").unwrap(),
            );
        }
        let mut body: String = String::new();
        res.read_to_string(&mut body)?;
        let doc: LoginResponse = serde_xml_rs::from_str(&body)?;

        // get challenge - and create response.
        let s = format!("{}-{}", doc.challenge, self.password);
        let bytes: Vec<u8> = s
            .encode_utf16()
            .flat_map(|utf16| utf16.to_le_bytes().to_vec())
            .collect();
        let mut hasher = md5::Md5::new();
        hasher.update(bytes);
        let tmp = hasher.finalize();

        // get sid with the response
        let query = format!(
            "{}/login_sid.lua?username={}&response={}-{:x}",
            self.url, self.user, doc.challenge, tmp
        );
        let mut res = self.client.get(query).send()?;
        if res.status() != 200 {
            return Err(Box::try_from("Status code was not 200 when retrieving the SID.").unwrap());
        }
        let mut body: String = String::new();
        res.read_to_string(&mut body)?;
        let doc: LoginResponse = serde_xml_rs::from_str(&body)?;

        Ok(doc.sid)
    }

    fn get_value(&self, command: &str, sid: &str) -> Result<f64, Box<dyn Error>> {
        let query = format!(
            "{}/webservices/homeautoswitch.lua?switchcmd={}&ain={}&sid={}",
            self.url, command, self.ain, sid
        );
        let mut res = self.client.get(query).send()?;
        if res.status() != 200 {
            return Err(Box::try_from(format!(
                "Status code was not 200 when retrieving data for: {}",
                command
            ))
            .unwrap());
        }
        let mut body: String = String::new();
        res.read_to_string(&mut body)?;
        let val: f64 = body.trim().parse()?;
        Ok(val)
    }
}

impl common::Sensor for FritzSensor {
    fn get_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for metric in METRICS {
            names.push(format!("{}_{}", self.name, metric));
        }
        names
    }

    fn measure(&mut self) -> Vec<f64> {
        match self.get_token() {
            Ok(sid) => {
                let mut res = Vec::new();
                for op in &["getswitchpower", "getswitchenergy", "gettemperature"] {
                    let tmp: f64 = match self.get_value(op, &sid) {
                        Ok(res) => res,
                        Err(err) => {
                            println!("Could not retrieve val: {}.", err);
                            -1.0
                        }
                    };
                    res.push(tmp)
                }
                res
            }
            Err(err) => {
                println!("Could not retrieve SID: {:?}.", err);
                vec![-1.0, -1.0, -1.0]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mockito;

    use crate::common::Sensor;

    use super::*;

    // Tests for success.

    #[test]
    fn test_get_names_for_success() {
        let sensor: FritzSensor = FritzSensor::new(
            "fritz".to_string(),
            "".to_string(),
            "foo".to_string(),
            "bar".to_string(),
            "aabbccddeeff".to_string(),
        );
        sensor.get_names();
    }

    // Tests for failure.

    #[test]
    fn test_measure_for_failure() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/login_sid.lua")
            .with_status(406)
            .with_body(
                "<SessionInfo><Challenge>abcdefgh</Challenge><SID>000000000000</SID></SessionInfo>",
            )
            .create();

        let url: String = server.url();
        let mut sensor = FritzSensor::new(
            "test".to_string(),
            url,
            "foo".to_string(),
            "bar".to_string(),
            "abc".to_string(),
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0, -1.0]);

        server
            .mock("GET", "/login_sid.lua")
            .with_status(200)
            .with_body(
                "<SessionInfo><Challenge>abcdefgh</Challenge><SID>000000000001</SID></SessionInfo>",
            )
            .create();
        server
            .mock("GET", "/login_sid.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "username".into(),
                "foo".into(),
            ))
            .with_body(
                "<SessionInfo><Challenge>abcdefgh</Challenge><SID>000000000002</SID></SessionInfo>",
            )
            .with_status(406)
            .create();
        let url: String = server.url();
        sensor.url = url;
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0, -1.0]);

        server
            .mock("GET", "/login_sid.lua")
            .with_status(200)
            .with_body(
                "<SessionInfo><Challenge>abcdefgh</Challenge><SID>000000000001</SID></SessionInfo>",
            )
            .create();
        server
            .mock("GET", "/login_sid.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "username".into(),
                "foo".into(),
            ))
            .with_body(
                "<SessionInfo><Challenge>abcdefgh</Challenge><SID>000000000002</SID></SessionInfo>",
            )
            .create();
        server
            .mock("GET", "/webservices/homeautoswitch.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "switchcmd".into(),
                "getswitchpower".into(),
            ))
            .with_status(406)
            .with_body("goo")
            .create();
        let url: String = server.url();
        sensor.url = url;
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0, -1.0]);
    }

    // Tests for sanity.

    #[test]
    fn test_get_names_for_sanity() {
        let sensor: FritzSensor = FritzSensor::new(
            "fritz".to_string(),
            "".to_string(),
            "foo".to_string(),
            "bar".to_string(),
            "abc".to_string(),
        );
        assert_eq!(
            sensor.get_names(),
            vec!["fritz_power", "fritz_energy", "fritz_temperature"]
        );
    }

    #[test]
    fn test_measure_for_sanity() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/login_sid.lua")
            .with_body(
                "<SessionInfo><Challenge>1234abcd</Challenge><SID>000000000000</SID></SessionInfo>",
            )
            .create();
        server
            .mock("GET", "/login_sid.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "username".into(),
                "foo".into(),
            ))
            .with_body(
                "<SessionInfo><Challenge>abcdefgh</Challenge><SID>000000000000</SID></SessionInfo>",
            )
            .create();
        server
            .mock("GET", "/webservices/homeautoswitch.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "switchcmd".into(),
                "getswitchpower".into(),
            ))
            .with_body("10000")
            .create();
        server
            .mock("GET", "/webservices/homeautoswitch.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "switchcmd".into(),
                "getswitchenergy".into(),
            ))
            .with_body("1200")
            .create();
        server
            .mock("GET", "/webservices/homeautoswitch.lua")
            .match_query(mockito::Matcher::UrlEncoded(
                "switchcmd".into(),
                "gettemperature".into(),
            ))
            .with_body("100")
            .create();

        let url: String = server.url();
        let mut sensor = FritzSensor::new(
            "test".to_string(),
            url,
            "foo".to_string(),
            "bar".to_string(),
            "abc".to_string(),
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![10000.0, 1200.0, 100.0]);
    }
}
