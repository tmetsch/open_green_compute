/// Defines a basic sensor.
pub(crate) trait Sensor {
    fn get_names(&self) -> Vec<String>;
    fn measure(&self) -> Vec<f64>;
}
