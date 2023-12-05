/// Defines a basic sensor.
pub(crate) trait Sensor {
    fn get_names(&self) -> Vec<String>;
    fn measure(&mut self) -> Vec<f64>;
}
