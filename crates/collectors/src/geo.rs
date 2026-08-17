pub(crate) fn haversine_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
    let latitude_delta = (lat2 - lat1).to_radians();
    let longitude_delta = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let haversine = (latitude_delta / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * (longitude_delta / 2.0).sin().powi(2);
    EARTH_RADIUS_METERS * 2.0 * haversine.clamp(0.0, 1.0).sqrt().asin()
}

pub(crate) fn initial_bearing_degrees(
    latitude: f64,
    longitude: f64,
    target_latitude: f64,
    target_longitude: f64,
) -> f64 {
    let source_latitude = latitude.to_radians();
    let target_latitude = target_latitude.to_radians();
    let longitude_delta = (target_longitude - longitude).to_radians();
    let y = longitude_delta.sin() * target_latitude.cos();
    let x = source_latitude.cos() * target_latitude.sin()
        - source_latitude.sin() * target_latitude.cos() * longitude_delta.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

pub(crate) fn angular_difference_degrees(left: f64, right: f64) -> f64 {
    let difference = (left - right).abs().rem_euclid(360.0);
    difference.min(360.0 - difference)
}
