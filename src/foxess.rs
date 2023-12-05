use crate::common;
use chrono::{Datelike, Utc};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::Read;

pub struct FoxEssSensor {
    name: String,
    user: String,
    password: String,
    inverter_id: String,
    variables: Vec<String>,
    url: String,
    client: reqwest::blocking::Client,
    token: String,
}

#[derive(Deserialize)]
struct TokenInfo {
    token: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    errno: usize,
    result: TokenInfo,
}

#[derive(Deserialize)]
struct TokenTestResponse {
    errno: usize,
}

#[derive(Serialize)]
struct BeginDate {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

#[derive(Serialize)]
struct RequestParameter {
    #[serde(rename = "deviceId")]
    device_id: String,
    variables: Vec<String>,
    timespan: String,
    #[serde(rename = "BeginDate")]
    begin_date: BeginDate,
}

#[derive(Deserialize)]
struct SeriesEntry {
    // omitting time: String for now.
    value: f64,
}

#[derive(Deserialize)]
struct DataSeries {
    variable: String,
    // omitting for now name: String, FIXME: add this to verify order is always correct!
    data: Vec<SeriesEntry>,
}

#[derive(Deserialize)]
struct DataResponse {
    errno: usize,
    result: Vec<DataSeries>,
}

impl FoxEssSensor {
    pub fn new(
        name: String,
        user: String,
        password: String,
        inverter_id: String,
        variables: Vec<String>,
        url: String,
    ) -> FoxEssSensor {
        let builder: reqwest::blocking::ClientBuilder = reqwest::blocking::ClientBuilder::new();
        let client = builder.danger_accept_invalid_certs(true).build().unwrap();
        FoxEssSensor {
            name,
            user,
            password,
            inverter_id,
            variables,
            url,
            client,
            token: "n/a".to_string(),
        }
    }

    fn get_token(&self) -> Result<String, Box<dyn Error>> {
        let url = format!("{}/c/v0/user/login", self.url);
        // hash the password and try to get a token...
        let tmp = Md5::digest(self.password.as_bytes());
        let body = format!("user={}&password={:x}", self.user, tmp);
        let mut response = self
            .client
            .post(url)
            .header("User-Agent", "")
            .body(body)
            .send()?;
        if response.status() != 200 {
            return Err(Box::try_from(format!(
                "Status code was not 200; but: {}.",
                response.status()
            ))
            .unwrap());
        }

        // parse the result...
        let mut body: String = String::new();
        response.read_to_string(&mut body)?;
        let doc: TokenResponse = serde_json::from_str(&body)?;

        // wrong credentials will lead to a non zero err code from the api.
        if doc.errno != 0 {
            return Err(
                Box::try_from(format!("Error code was not 0; but: {}.", doc.errno)).unwrap(),
            );
        }
        Ok(doc.result.token)
    }

    fn test_token(&self, token: &str) -> Result<bool, Box<dyn Error>> {
        let url = format!("{}/c/v0/device/status/all", self.url);
        let mut response = self
            .client
            .get(url)
            .header("User-Agent", "")
            .header("token", token)
            .send()?;
        if response.status() != 200 {
            return Err(Box::try_from(format!(
                "Status code was not 200; but: {}.",
                response.status()
            ))
            .unwrap());
        }

        // parse the result...
        let mut body: String = String::new();
        response.read_to_string(&mut body)?;
        let doc: TokenTestResponse = serde_json::from_str(&body)?;

        // wrong credentials will lead to a non zero err code from the api.
        if doc.errno != 0 {
            return Err(
                Box::try_from(format!("Error code was not 0; but: {}.", doc.errno)).unwrap(),
            );
        }
        Ok(true)
    }

    fn do_query(&self, token: &str) -> Result<Vec<f64>, Box<dyn Error>> {
        let url = format!("{}/c/v0/device/history/raw", self.url);
        let now = Utc::now();
        let request_parameter = RequestParameter {
            device_id: self.inverter_id.clone(),
            variables: self.variables.clone(),
            timespan: "day".to_string(),
            begin_date: BeginDate {
                year: now.year(),
                month: now.month(),
                day: now.day(),
                hour: 0,
                minute: 0,
                second: 0,
            },
        };
        let mut response = self
            .client
            .post(url)
            .json(&request_parameter)
            .header("token", token)
            .header("User-Agent", "")
            .send()?;

        if response.status() != 200 {
            return Err(Box::try_from(format!(
                "Status code was not 200; but: {}.",
                response.status()
            ))
            .unwrap());
        }

        // parse the result...
        let mut body: String = String::new();
        response.read_to_string(&mut body)?;
        let doc: DataResponse = serde_json::from_str(&body)?;

        if doc.errno != 0 {
            return Err(
                Box::try_from(format!("Error code was not 0; but: {}.", doc.errno)).unwrap(),
            );
        }

        // result is order; first one we asked for is the first one we should get...
        if doc.result.len() != self.variables.len() {
            return Err(Box::try_from(
                "Number of data entries does not match number of requested entries.",
            )
            .unwrap());
        }
        let mut res = Vec::new();
        for series in doc.result {
            // pick the last measurement - as FoxESS raw data is "old" anyhow this seems to be the best we can do.
            if series.data.is_empty() {
                return Err(Box::try_from(format!(
                    "Expected at least one value for the series: {}; length is: {}.",
                    series.variable,
                    series.data.len()
                ))
                .unwrap());
            }
            res.push(series.data.last().unwrap().value);
        }
        Ok(res)
    }
}

impl common::Sensor for FoxEssSensor {
    fn get_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for metric in self.variables.iter() {
            names.push(format!("{}_{}", self.name, metric));
        }
        names
    }

    fn measure(&mut self) -> Vec<f64> {
        if self.token == "n/a" {
            self.token = match self.get_token() {
                Ok(res) => res,
                Err(err) => {
                    println!("Could not retrieve a valid token: {}.", err);
                    return vec![-1.0; self.variables.len()];
                }
            };
        } else {
            match self.test_token(&self.token) {
                Ok(_) => {}
                Err(_) => {
                    self.token = match self.get_token() {
                        Ok(res) => res,
                        Err(err) => {
                            println!("Could not retrieve a valid token: {}.", err);
                            return vec![-1.0; self.variables.len()];
                        }
                    };
                }
            }
        }

        match self.do_query(&self.token) {
            Ok(res) => res,
            Err(err) => {
                println!("Could not retrieve values: {}.", err);
                vec![-1.0; self.variables.len()]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Sensor;
    use mockito;

    // Tests for success.

    // Tests for failure.

    #[test]
    fn test_measure_for_failure() {
        let mut server = mockito::Server::new();
        let url: String = server.url();

        // token - errno not 0.
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 1, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        let mut sensor = FoxEssSensor::new(
            "fox0".to_string(),
            "foo".to_string(),
            "bar".to_string(),
            "".to_string(),
            vec!["foo".to_string(), "bar".to_string()],
            url,
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);

        // token - status code not 200.
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(403)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);

        // token ok - retrieving raw data returns errno no 0.
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(500)
            .with_body("{}")
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);

        // token ok - retrieving raw data returns some non 200 status code.
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(200)
            .with_body("{\"errno\": 40000, \"result\": []}")
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);

        // token ok - retrieving raw data return corrupt data series.
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": [\
                {\"variable\":\"foo\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[{\"time\":\"2023-11-24 18:02:00 CET+0100\",\"value\":0.1},{\"time\":\"2023-11-24 18:06:30 CET+0100\",\"value\":0.5}]}\
                ]}",
            )
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);

        // token ok - retrieving raw data return corrupt data series.
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": [\
                {\"variable\":\"foo\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[]},\
                {\"variable\":\"bar\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[]}\
                ]}",
            )
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);
        assert_eq!(sensor.token, "abc");

        // we'll let test token fail.
        server
            .mock("GET", "/c/v0/device/status/all")
            .with_status(200)
            .with_body("{\"errno\": 40000}")
            .create();
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"xyz\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": [\
                {\"variable\":\"foo\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[]},\
                {\"variable\":\"bar\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[]}\
                ]}",
            )
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);
        assert_eq!(sensor.token, "xyz");

        // we'll let test token fail.
        server
            .mock("GET", "/c/v0/device/status/all")
            .with_status(200)
            .with_body("{\"errno\": 40000}")
            .create();
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body("{\"errno\": 40000, \"result\": {\"token\": \"\"}}")
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0, -1.0]);
    }

    // Tests for sanity.

    #[test]
    fn test_get_names_for_sanity() {
        let sensor = FoxEssSensor::new(
            "fox0".to_string(),
            "foo".to_string(),
            "bar".to_string(),
            "".to_string(),
            vec!["foo".to_string(), "bar".to_string()],
            "".to_string(),
        );
        let data: Vec<String> = sensor.get_names();
        assert_eq!(data, vec!["fox0_foo", "fox0_bar"]);
    }
    #[test]
    fn test_measure_for_sanity() {
        let mut server = mockito::Server::new();
        let url: String = server.url();
        server
            .mock("POST", "/c/v0/user/login")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": {\"token\": \"abc\", \"user\": \"foo\", \"access\": 1}}",
            )
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": [\
                {\"variable\":\"foo\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[{\"time\":\"2023-11-24 18:02:00 CET+0100\",\"value\":0.1},{\"time\":\"2023-11-24 18:06:30 CET+0100\",\"value\":0.5}]},\
                {\"variable\":\"bar\",\"unit\":\"kW\",\"name\":\"Bar Power\",\"data\":[{\"time\":\"2023-11-24 18:02:00 CET+0100\",\"value\":0.4}]}\
                ]}",
            )
            .create();
        let mut sensor = FoxEssSensor::new(
            "fox0".to_string(),
            "foo".to_string(),
            "bar".to_string(),
            "".to_string(),
            vec!["foo".to_string(), "bar".to_string()],
            url,
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![0.5, 0.4]);
        assert_eq!(sensor.token, "abc");

        // this time we reuse a token.
        server
            .mock("GET", "/c/v0/device/status/all")
            .with_status(200)
            .with_body("{\"errno\": 0}")
            .create();
        server
            .mock("POST", "/c/v0/device/history/raw")
            .with_status(200)
            .with_body(
                "{\"errno\": 0, \"result\": [\
                {\"variable\":\"foo\",\"unit\":\"kW\",\"name\":\"Foo Power\",\"data\":[{\"time\":\"2023-11-24 18:02:00 CET+0100\",\"value\":0.1},{\"time\":\"2023-11-24 18:06:30 CET+0100\",\"value\":0.8}]},\
                {\"variable\":\"bar\",\"unit\":\"kW\",\"name\":\"Bar Power\",\"data\":[{\"time\":\"2023-11-24 18:02:00 CET+0100\",\"value\":0.7}]}\
                ]}",
            )
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![0.8, 0.7]);
    }
}
