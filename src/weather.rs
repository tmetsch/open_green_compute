use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::common;

const NAMES: [&str; 8] = [
    "temperature",
    "humidity",
    "pressure",
    "visibility",
    "wind_speed",
    "wind_direction",
    "cloud_coverage",
    "description",
];

#[derive(Serialize, Deserialize)]
struct WeatherData {
    id: f64,
}

#[derive(Serialize, Deserialize)]
struct MainData {
    temp: f64,
    pressure: f64,
    humidity: f64,
}

#[derive(Serialize, Deserialize)]
struct WindData {
    speed: f64,
    deg: f64,
}

#[derive(Serialize, Deserialize)]
struct CloudData {
    all: f64,
}

#[derive(Serialize, Deserialize)]
struct WeatherInfo {
    weather: Vec<WeatherData>,
    main: Option<MainData>,
    visibility: Option<f64>,
    wind: Option<WindData>,
    clouds: Option<CloudData>,
}

pub struct WeatherSensor {
    name: String,
    url: String,
    lat: f64,
    long: f64,
    app_id: String,
}

impl WeatherSensor {
    pub fn new(name: String, url: String, lat: f64, long: f64, app_id: String) -> WeatherSensor {
        WeatherSensor {
            name,
            url,
            lat,
            long,
            app_id,
        }
    }
}

impl common::Sensor for WeatherSensor {
    fn get_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for item in NAMES {
            names.push(format!("{}_{}", self.name, item));
        }
        names
    }

    fn measure(&self) -> Vec<f64> {
        // blocking requests are ok, weather doesn't change that often. async prog hence might be overkill.
        let uri: String = format!(
            "{0}?lat={1}&lon={2}&appid={3}&units=metric",
            self.url, self.lat, self.long, self.app_id
        );
        let mut body: String = String::new();
        let mut res = match reqwest::blocking::get(uri) {
            Ok(res) => res,
            Err(_) => return vec![-1.0; NAMES.len()],
        };
        if res.status() != 200 {
            return vec![-1.0; NAMES.len()];
        }
        if res.read_to_string(&mut body).is_err() {
            return vec![-1.0; NAMES.len()];
        }

        // parse the data.
        let weather: WeatherInfo = match serde_json::from_str(&body) {
            Ok(body) => body,
            Err(_error) => return vec![-1.0; NAMES.len()],
        };
        let main: MainData = weather.main.unwrap_or_else(|| MainData {
            temp: -1.0,
            pressure: -1.0,
            humidity: -1.0,
        });
        let wind: WindData = weather.wind.unwrap_or_else(|| WindData {
            speed: -1.0,
            deg: -1.0,
        });
        let clouds: CloudData = weather.clouds.unwrap_or_else(|| CloudData { all: -1.0 });

        vec![
            main.temp,
            main.humidity,
            main.pressure,
            weather.visibility.unwrap_or_else(|| -1.0),
            wind.speed,
            wind.deg,
            clouds.all,
            weather.weather[0].id,
        ]
    }
}

#[cfg(test)]
mod tests {
    // Using mockito is not perfect as is spins up a server & hence is more of an integration tests;
    // but works for now w/o to many add on dependencies, so should be easy to replace.
    use mockito;

    use crate::common::Sensor;

    use super::*;

    const TEST_DATA: &str = "{\"weather\": [{\"id\": 201}], \
    \"main\": {\"temp\": 23, \"pressure\": 900, \"humidity\": 65}, \
    \"visibility\": 100000, \
    \"clouds\": {\"all\": 75}, \
    \"wind\": {\"speed\": 2.4, \"deg\": 270}}";

    const FAULTY_DATA: &str = "{\"weather\": [{\"id\": 201}], \
    \"main\": {}, \
    \"clouds\": {}, \
    \"wind\": {}}";

    // Tests for success.

    #[test]
    fn test_get_names_for_success() {
        let sensor: WeatherSensor = WeatherSensor::new(
            "test".to_string(),
            "localhost".to_string(),
            0.0,
            0.0,
            "foo".to_string(),
        );
        sensor.get_names();
    }

    #[test]
    fn test_measure_for_success() {
        let mut server = mockito::Server::new();
        server
            .mock(
                "GET",
                "/data/2.5/weather?lat=0&lon=0&appid=foo&units=metric",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(TEST_DATA)
            .create();

        //
        let url: String = server.url();
        let sensor = WeatherSensor::new(
            "test".to_string(),
            url.to_owned() + "/data/2.5/weather",
            0.0,
            0.0,
            "foo".to_string(),
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data.len(), NAMES.len());
    }

    // Tests for failure.

    #[test]
    fn test_measure_for_failure() {
        let mut server = mockito::Server::new();
        server
            .mock(
                "GET",
                "/data/2.5/weather?lat=0&lon=0&appid=foo&units=metric",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("ohno")
            .create();

        // totally faulty data.
        let url: String = server.url();
        let sensor = WeatherSensor::new(
            "test".to_string(),
            url.to_owned() + "/data/2.5/weather",
            0.0,
            0.0,
            "foo".to_string(),
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0; NAMES.len()]);

        // partly faulty data.
        server
            .mock(
                "GET",
                "/data/2.5/weather?lat=0&lon=0&appid=foo&units=metric",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(FAULTY_DATA)
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0; NAMES.len()]);

        // server error
        server
            .mock(
                "GET",
                "/data/2.5/weather?lat=0&lon=0&appid=foo&units=metric",
            )
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body("Whoops")
            .create();
        let data: Vec<f64> = sensor.measure();
        assert_eq!(data, vec![-1.0; NAMES.len()]);
    }

    // Tests for sanity.

    #[test]
    fn test_get_names_for_sanity() {
        let sensor = WeatherSensor::new(
            "test".to_string(),
            "localhost:8080/data/2.5/weather".to_string(),
            0.0,
            0.0,
            "foo".to_string(),
        );
        let res: Vec<String> = sensor.get_names();
        assert_eq!(
            res,
            vec![
                "test_temperature",
                "test_humidity",
                "test_pressure",
                "test_visibility",
                "test_wind_speed",
                "test_wind_direction",
                "test_cloud_coverage",
                "test_description"
            ]
        );
    }

    #[test]
    fn test_measure_for_sanity() {
        let mut server = mockito::Server::new();
        server
            .mock(
                "GET",
                "/data/2.5/weather?lat=0&lon=0&appid=foo&units=metric",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(TEST_DATA)
            .create();

        //
        let url: String = server.url();
        let sensor = WeatherSensor::new(
            "test".to_string(),
            url.to_owned() + "/data/2.5/weather",
            0.0,
            0.0,
            "foo".to_string(),
        );
        let data: Vec<f64> = sensor.measure();
        assert_eq!(
            data,
            vec![23.0, 65.0, 900.0, 100000.0, 2.4, 270.0, 75.0, 201.0]
        );
    }
}
