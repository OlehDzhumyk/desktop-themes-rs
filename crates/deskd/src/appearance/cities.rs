// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline city coordinates and IANA timezone-to-coordinate fallback table.
//!
//! Used locally to compute sunrise and sunset with zero network requests.

/// A resolved geographic coordinate for solar calculations.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedLocation {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
    pub approximate: bool,
}

struct City {
    name: &'static str,
    lat: f64,
    lon: f64,
}

const CITIES: &[City] = &[
    // Ukraine
    City {
        name: "Kyiv",
        lat: 50.45,
        lon: 30.52,
    },
    City {
        name: "Lviv",
        lat: 49.84,
        lon: 24.03,
    },
    City {
        name: "Kharkiv",
        lat: 49.99,
        lon: 36.23,
    },
    City {
        name: "Odesa",
        lat: 46.48,
        lon: 30.73,
    },
    City {
        name: "Dnipro",
        lat: 48.46,
        lon: 35.05,
    },
    City {
        name: "Zaporizhzhia",
        lat: 47.84,
        lon: 35.14,
    },
    City {
        name: "Vinnytsia",
        lat: 49.23,
        lon: 28.47,
    },
    City {
        name: "Ivano-Frankivsk",
        lat: 48.92,
        lon: 24.71,
    },
    City {
        name: "Ternopil",
        lat: 49.55,
        lon: 25.59,
    },
    City {
        name: "Chernivtsi",
        lat: 48.29,
        lon: 25.94,
    },
    City {
        name: "Uzhhorod",
        lat: 48.62,
        lon: 22.29,
    },
    City {
        name: "Poltava",
        lat: 49.59,
        lon: 34.55,
    },
    City {
        name: "Chernihiv",
        lat: 51.49,
        lon: 31.29,
    },
    City {
        name: "Sumy",
        lat: 50.91,
        lon: 34.80,
    },
    City {
        name: "Rivne",
        lat: 50.62,
        lon: 26.25,
    },
    City {
        name: "Lutsk",
        lat: 50.75,
        lon: 25.33,
    },
    City {
        name: "Zhytomyr",
        lat: 50.25,
        lon: 28.66,
    },
    City {
        name: "Khmelnytskyi",
        lat: 49.42,
        lon: 26.99,
    },
    City {
        name: "Cherkasy",
        lat: 49.44,
        lon: 32.06,
    },
    City {
        name: "Kropyvnytskyi",
        lat: 48.51,
        lon: 32.26,
    },
    City {
        name: "Mykolaiv",
        lat: 46.97,
        lon: 31.99,
    },
    City {
        name: "Kherson",
        lat: 46.64,
        lon: 32.62,
    },
    City {
        name: "Simferopol",
        lat: 44.95,
        lon: 34.10,
    },
    City {
        name: "Donetsk",
        lat: 48.02,
        lon: 37.80,
    },
    City {
        name: "Luhansk",
        lat: 48.57,
        lon: 39.31,
    },
    // Europe
    City {
        name: "London",
        lat: 51.51,
        lon: -0.13,
    },
    City {
        name: "Dublin",
        lat: 53.35,
        lon: -6.26,
    },
    City {
        name: "Lisbon",
        lat: 38.72,
        lon: -9.14,
    },
    City {
        name: "Madrid",
        lat: 40.42,
        lon: -3.70,
    },
    City {
        name: "Barcelona",
        lat: 41.39,
        lon: 2.17,
    },
    City {
        name: "Paris",
        lat: 48.86,
        lon: 2.35,
    },
    City {
        name: "Brussels",
        lat: 50.85,
        lon: 4.35,
    },
    City {
        name: "Amsterdam",
        lat: 52.37,
        lon: 4.90,
    },
    City {
        name: "Berlin",
        lat: 52.52,
        lon: 13.41,
    },
    City {
        name: "Munich",
        lat: 48.14,
        lon: 11.58,
    },
    City {
        name: "Frankfurt",
        lat: 50.11,
        lon: 8.68,
    },
    City {
        name: "Zurich",
        lat: 47.37,
        lon: 8.54,
    },
    City {
        name: "Vienna",
        lat: 48.21,
        lon: 16.37,
    },
    City {
        name: "Prague",
        lat: 50.08,
        lon: 14.44,
    },
    City {
        name: "Warsaw",
        lat: 52.23,
        lon: 21.01,
    },
    City {
        name: "Krakow",
        lat: 50.06,
        lon: 19.94,
    },
    City {
        name: "Budapest",
        lat: 47.50,
        lon: 19.04,
    },
    City {
        name: "Bratislava",
        lat: 48.15,
        lon: 17.11,
    },
    City {
        name: "Bucharest",
        lat: 44.43,
        lon: 26.10,
    },
    City {
        name: "Chisinau",
        lat: 47.01,
        lon: 28.86,
    },
    City {
        name: "Sofia",
        lat: 42.70,
        lon: 23.32,
    },
    City {
        name: "Athens",
        lat: 37.98,
        lon: 23.73,
    },
    City {
        name: "Rome",
        lat: 41.90,
        lon: 12.50,
    },
    City {
        name: "Milan",
        lat: 45.46,
        lon: 9.19,
    },
    City {
        name: "Zagreb",
        lat: 45.81,
        lon: 15.98,
    },
    City {
        name: "Belgrade",
        lat: 44.79,
        lon: 20.45,
    },
    City {
        name: "Vilnius",
        lat: 54.69,
        lon: 25.28,
    },
    City {
        name: "Riga",
        lat: 56.95,
        lon: 24.11,
    },
    City {
        name: "Tallinn",
        lat: 59.44,
        lon: 24.75,
    },
    City {
        name: "Helsinki",
        lat: 60.17,
        lon: 24.94,
    },
    City {
        name: "Stockholm",
        lat: 59.33,
        lon: 18.07,
    },
    City {
        name: "Oslo",
        lat: 59.91,
        lon: 10.75,
    },
    City {
        name: "Copenhagen",
        lat: 55.68,
        lon: 12.57,
    },
    City {
        name: "Reykjavik",
        lat: 64.15,
        lon: -21.94,
    },
    City {
        name: "Tromso",
        lat: 69.65,
        lon: 18.96,
    },
    City {
        name: "Longyearbyen",
        lat: 78.22,
        lon: 15.63,
    },
    City {
        name: "Istanbul",
        lat: 41.01,
        lon: 28.98,
    },
    // Americas
    City {
        name: "New York",
        lat: 40.71,
        lon: -74.01,
    },
    City {
        name: "Boston",
        lat: 42.36,
        lon: -71.06,
    },
    City {
        name: "Washington",
        lat: 38.91,
        lon: -77.04,
    },
    City {
        name: "Miami",
        lat: 25.76,
        lon: -80.19,
    },
    City {
        name: "Chicago",
        lat: 41.88,
        lon: -87.63,
    },
    City {
        name: "Austin",
        lat: 30.27,
        lon: -97.74,
    },
    City {
        name: "Denver",
        lat: 39.74,
        lon: -104.99,
    },
    City {
        name: "Los Angeles",
        lat: 34.05,
        lon: -118.24,
    },
    City {
        name: "San Francisco",
        lat: 37.77,
        lon: -122.42,
    },
    City {
        name: "Seattle",
        lat: 47.61,
        lon: -122.33,
    },
    City {
        name: "Anchorage",
        lat: 61.22,
        lon: -149.90,
    },
    City {
        name: "Honolulu",
        lat: 21.31,
        lon: -157.86,
    },
    City {
        name: "Toronto",
        lat: 43.65,
        lon: -79.38,
    },
    City {
        name: "Montreal",
        lat: 45.50,
        lon: -73.57,
    },
    City {
        name: "Vancouver",
        lat: 49.28,
        lon: -123.12,
    },
    City {
        name: "Mexico City",
        lat: 19.43,
        lon: -99.13,
    },
    City {
        name: "Bogota",
        lat: 4.71,
        lon: -74.07,
    },
    City {
        name: "Lima",
        lat: -12.05,
        lon: -77.04,
    },
    City {
        name: "Santiago",
        lat: -33.45,
        lon: -70.67,
    },
    City {
        name: "Buenos Aires",
        lat: -34.60,
        lon: -58.38,
    },
    City {
        name: "Sao Paulo",
        lat: -23.55,
        lon: -46.63,
    },
    // Asia-Pacific & Middle East
    City {
        name: "Tokyo",
        lat: 35.68,
        lon: 139.65,
    },
    City {
        name: "Seoul",
        lat: 37.57,
        lon: 126.98,
    },
    City {
        name: "Beijing",
        lat: 39.90,
        lon: 116.41,
    },
    City {
        name: "Shanghai",
        lat: 31.23,
        lon: 121.47,
    },
    City {
        name: "Hong Kong",
        lat: 22.32,
        lon: 114.17,
    },
    City {
        name: "Taipei",
        lat: 25.03,
        lon: 121.57,
    },
    City {
        name: "Singapore",
        lat: 1.35,
        lon: 103.82,
    },
    City {
        name: "Bangkok",
        lat: 13.76,
        lon: 100.50,
    },
    City {
        name: "Jakarta",
        lat: -6.21,
        lon: 106.85,
    },
    City {
        name: "Manila",
        lat: 14.60,
        lon: 120.98,
    },
    City {
        name: "Mumbai",
        lat: 19.08,
        lon: 72.88,
    },
    City {
        name: "Delhi",
        lat: 28.61,
        lon: 77.21,
    },
    City {
        name: "Bengaluru",
        lat: 12.97,
        lon: 77.59,
    },
    City {
        name: "Dubai",
        lat: 25.20,
        lon: 55.27,
    },
    City {
        name: "Tel Aviv",
        lat: 32.09,
        lon: 34.78,
    },
    City {
        name: "Riyadh",
        lat: 24.71,
        lon: 46.68,
    },
    City {
        name: "Sydney",
        lat: -33.87,
        lon: 151.21,
    },
    City {
        name: "Melbourne",
        lat: -37.81,
        lon: 144.96,
    },
    City {
        name: "Auckland",
        lat: -36.85,
        lon: 174.76,
    },
    // Africa
    City {
        name: "Cairo",
        lat: 30.04,
        lon: 31.24,
    },
    City {
        name: "Casablanca",
        lat: 33.57,
        lon: -7.59,
    },
    City {
        name: "Lagos",
        lat: 6.52,
        lon: 3.38,
    },
    City {
        name: "Nairobi",
        lat: -1.29,
        lon: 36.82,
    },
    City {
        name: "Johannesburg",
        lat: -26.20,
        lon: 28.05,
    },
];

/// IANA timezone aliases whose city segment does not match `CITIES` directly.
const TZ_ALIASES: &[(&str, &str)] = &[
    ("Europe/Kiev", "Kyiv"),
    ("Europe/Uzhgorod", "Uzhhorod"),
    ("Europe/Zaporozhye", "Zaporizhzhia"),
    ("Asia/Kolkata", "Delhi"),
    ("Asia/Calcutta", "Delhi"),
    ("Asia/Jerusalem", "Tel Aviv"),
    ("America/Argentina/Buenos_Aires", "Buenos Aires"),
    ("America/Sao_Paulo", "Sao Paulo"),
    ("America/Mexico_City", "Mexico City"),
    ("America/Los_Angeles", "Los Angeles"),
    ("America/New_York", "New York"),
    ("Pacific/Honolulu", "Honolulu"),
    ("Pacific/Auckland", "Auckland"),
];

fn find_city(query: &str) -> Option<&'static City> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if let Some(c) = CITIES.iter().find(|c| c.name.eq_ignore_ascii_case(q)) {
        return Some(c);
    }
    let ql = q.to_ascii_lowercase();
    CITIES
        .iter()
        .find(|c| c.name.to_ascii_lowercase().starts_with(&ql))
}

fn parse_coords(input: &str) -> Option<(f64, f64)> {
    let (a, b) = input.split_once(',')?;
    let lat: f64 = a.trim().parse().ok()?;
    let lon: f64 = b.trim().parse().ok()?;
    if !lat.is_finite()
        || !lon.is_finite()
        || !(-90.0..=90.0).contains(&lat)
        || !(-180.0..=180.0).contains(&lon)
    {
        return None;
    }
    Some((lat, lon))
}

fn from_timezone(tz: &str) -> Option<ResolvedLocation> {
    let tz = tz.trim();
    if tz.is_empty() {
        return None;
    }
    if let Some((_, city_name)) = TZ_ALIASES.iter().find(|(id, _)| *id == tz)
        && let Some(c) = find_city(city_name)
    {
        return Some(ResolvedLocation {
            lat: c.lat,
            lon: c.lon,
            label: format!("{} (timezone)", c.name),
            approximate: true,
        });
    }
    let segment = tz.rsplit('/').next()?.replace('_', " ");
    let c = find_city(&segment)?;
    Some(ResolvedLocation {
        lat: c.lat,
        lon: c.lon,
        label: format!("{} (timezone)", c.name),
        approximate: true,
    })
}

/// Resolve a user-configured location (`"lat,lon"`, city name, or empty) with system timezone
/// fallback. Never performs network I/O.
pub fn parse_location(input: &str, system_tz: Option<&str>) -> ResolvedLocation {
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        if let Some((lat, lon)) = parse_coords(trimmed) {
            return ResolvedLocation {
                lat,
                lon,
                label: format!("{lat:.2}, {lon:.2}"),
                approximate: false,
            };
        }
        if let Some(c) = find_city(trimmed) {
            return ResolvedLocation {
                lat: c.lat,
                lon: c.lon,
                label: c.name.to_string(),
                approximate: false,
            };
        }
    }
    if let Some(loc) = system_tz.and_then(from_timezone) {
        return loc;
    }
    ResolvedLocation {
        lat: 0.0,
        lon: 0.0,
        label: "UTC (approximate)".to_string(),
        approximate: true,
    }
}

/// Best-effort local IANA timezone name from `TZ` or `/etc/localtime`.
pub fn detect_system_timezone() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let trimmed = tz.trim().trim_start_matches(':');
        if !trimmed.is_empty() && trimmed.contains('/') {
            return Some(trimmed.to_string());
        }
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let s = target.to_string_lossy();
    let (_, tz) = s.split_once("zoneinfo/")?;
    (!tz.is_empty()).then(|| tz.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_coordinates() {
        let loc = parse_location("50.45, 30.52", None);
        assert!((loc.lat - 50.45).abs() < 1e-6);
        assert!((loc.lon - 30.52).abs() < 1e-6);
        assert_eq!(loc.label, "50.45, 30.52");
        assert!(!loc.approximate);
    }

    #[test]
    fn resolves_cities_case_insensitively() {
        let kyiv = parse_location("kyiv", None);
        assert_eq!(kyiv.label, "Kyiv");
        assert!(!kyiv.approximate);
        assert!((kyiv.lat - 50.45).abs() < 0.1);

        let lviv = parse_location("LVIV", None);
        assert_eq!(lviv.label, "Lviv");
        assert!(!lviv.approximate);
    }

    #[test]
    fn falls_back_to_system_timezone_when_empty_or_invalid() {
        let from_tz = parse_location("", Some("Europe/Kyiv"));
        assert!(from_tz.approximate);
        assert!(from_tz.label.contains("Kyiv"));
        assert!((from_tz.lat - 50.45).abs() < 0.1);

        let legacy_tz = parse_location("999,999", Some("Europe/Kiev"));
        assert!(legacy_tz.approximate);
        assert!(legacy_tz.label.contains("Kyiv"));

        let ny = parse_location("", Some("America/New_York"));
        assert!(ny.approximate);
        assert!(ny.label.contains("New York"));

        let unknown = parse_location("", Some("Etc/UTC"));
        assert!(unknown.approximate);
        assert_eq!(unknown.label, "UTC (approximate)");
    }
}
