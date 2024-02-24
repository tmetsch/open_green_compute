use crate::common;
use md5::{Digest, Md5};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::Read;

pub struct FoxEssOpenAPISensor {
    name: String,
    api_key: String,
    inverter_id: String,
    variables: Vec<String>,
    url: String,
    client: reqwest::blocking::Client,
}

#[derive(Serialize)]
struct DataRequest {
    #[serde(rename = "sn")]
    serial_number: String,
    variables: Vec<String>,
}
#[derive(Deserialize)]
struct DataEntry {
    // omitting unit & name for now.
    variable: String,
    value: f64,
}

#[derive(Deserialize)]
struct ResultSet {
    #[serde(rename = "datas")]
    data: Vec<DataEntry>,
}

#[derive(Deserialize)]
struct DataResponse {
    errno: usize,
    result: Vec<ResultSet>,
}

impl FoxEssOpenAPISensor {
    pub fn new(
        name: String,
        api_key: String,
        inverter_id: String,
        variables: Vec<String>,
        url: String,
    ) -> FoxEssOpenAPISensor {
        let builder: reqwest::blocking::ClientBuilder = reqwest::blocking::ClientBuilder::new();
        let client = builder.danger_accept_invalid_certs(true).build().unwrap();
        FoxEssOpenAPISensor {
            name,
            api_key,
            inverter_id,
            variables,
            url,
            client,
        }
    }

    pub fn do_query(&self, path: &str, token: &str) -> Result<Vec<f64>, Box<dyn Error>> {
        let url = format!("{}{}", self.url, path);

        // create signature
        let timestamp = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis();
        let signature_string = format!(r"{}\r\n{}\r\n{}", path, token, timestamp);
        let signature = format!("{:x}", Md5::digest(signature_string.as_bytes()));

        // headers
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers.insert("token", HeaderValue::from_str(token).unwrap());
        headers.insert("signature", HeaderValue::from_str(&signature).unwrap());
        headers.insert(
            "timestamp",
            HeaderValue::from_str(&timestamp.to_string()).unwrap(),
        );
        headers.insert("Lang", HeaderValue::from_static("en"));

        // payload
        let data_req = DataRequest {
            serial_number: self.inverter_id.clone(),
            variables: self.variables.clone(),
        };

        // post request
        let mut response = self
            .client
            .post(url)
            .headers(headers)
            .json(&data_req)
            .send()?;
        if response.status() != 200 {
            return Err(Box::from(format!(
                "Status code was not 200; but: {}.",
                response.status()
            )));
        }

        // parse the result...
        let mut body: String = String::new();
        response.read_to_string(&mut body)?;
        let doc: DataResponse = serde_json::from_str(&body)?;
        if doc.errno != 0 {
            return Err(Box::from(format!(
                "Error code was not 0; but: {}.",
                doc.errno
            )));
        }

        // we ask for 1 inverter atm; expect equal amount of elements to be returned as we request.
        if doc.result.len() != 1 || doc.result[0].data.len() != self.variables.len() {
            return Err(Box::from(
                "Number of data entries does not match number of requested entries.",
            ));
        }

        let mut res = Vec::new();
        for (i, data_entry) in doc.result[0].data.iter().enumerate() {
            if data_entry.variable != self.variables[i] {
                // result is ordered; first one we asked for is the first one we should get...
                return Err(Box::from(format!(
                    "Expected variable {} got {}.",
                    self.variables[i], data_entry.variable,
                )));
            }
            res.push(data_entry.value);
        }
        Ok(res)
    }
}

impl common::Sensor for FoxEssOpenAPISensor {
    fn get_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for metric in self.variables.iter() {
            names.push(format!("{}_{}", self.name, metric));
        }
        names
    }

    fn measure(&self) -> Vec<f64> {
        match self.do_query("/op/v0/device/real/query", &self.api_key) {
            Ok(res) => res,
            Err(err) => {
                println!("Could not retrieve values: {}", err);
                vec![-1.0; self.variables.len()]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Sensor;

    macro_rules! test_post_request {
        ($name:ident, $($status:expr, $body:expr, $expected:expr),+) => {
            #[test]
            fn $name() {
                $(
                    let mut server = mockito::Server::new();
                    let url: String = server.url();
                    server
                        .mock("POST", "/op/v0/device/real/query")
                        .with_status($status)
                        .with_body($body)
                        .create();
                    let sensor = FoxEssOpenAPISensor::new(
                        "fox0".to_string(),
                        "123".to_string(),
                        "abc".to_string(),
                        vec!["foo".to_string(), "bar".to_string()],
                        url,
                    );
                    let data: Vec<f64> = sensor.measure();
                    assert_eq!(data, $expected);
                )*
            }
        }
    }

    // Tests for success.

    // Tests for failure.

    test_post_request!(status_not_ok, 406, "", vec![-1.0, -1.0]);
    test_post_request!(
        errno_not_zero,
        200,
        "{\"errno\": 1, \"result\": []}",
        vec![-1.0, -1.0]
    );
    test_post_request!(wrong_order, 200, "{\"errno\": 0, \"result\": [{\"datas\": [{\"variable\": \"bar\", \"value\": 0.5},{\"variable\": \"foo\", \"value\": 0.5}]}]}", vec![-1.0, -1.0]);
    test_post_request!(
        missing_variable,
        200,
        "{\"errno\": 0, \"result\": [{\"datas\": [{\"variable\": \"foo\", \"value\": 0.5}]}]}",
        vec![-1.0, -1.0]
    );

    // Tests for sanity.

    #[test]
    fn test_get_names_for_sanity() {
        let sensor = FoxEssOpenAPISensor::new(
            "fox0".to_string(),
            "123".to_string(),
            "abc".to_string(),
            vec!["foo".to_string(), "bar".to_string()],
            "".to_string(),
        );
        let data: Vec<String> = sensor.get_names();
        assert_eq!(data, vec!["fox0_foo", "fox0_bar"]);
    }

    test_post_request!(
        sanity_check,
        200,
        "{\"errno\": 0, \"msg\": \"success\", \"result\": [{\"datas\": [\
                {\"unit\": \"kW\", \"name\": \"Blah\", \"variable\": \"foo\", \"value\": 0.5},\
                {\"unit\": \"kW\", \"name\": \"Blub\", \"variable\": \"bar\", \"value\": 0.4}],\
                \"time\": \"2024-02-21 12:34:36 CET+0100\", \"deviceSN\": \"abc\"}]}",
        vec![0.5, 0.4]
    );
}
