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
