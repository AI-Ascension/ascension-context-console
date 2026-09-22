// SPDX-License-Identifier: MIT

use super::super::types::Boundary;

pub(crate) fn parse_time(value: &str) -> Option<u64> {
    let date = value.strip_suffix('Z')?;
    let (day, time) = date.split_once('T')?;
    let mut parts = day.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    let mut clock = time.split(':');
    let hour = clock.next()?.parse::<u64>().ok()?;
    let minute = clock.next()?.parse::<u64>().ok()?;
    let second = clock.next()?.split('.').next()?.parse::<u64>().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(
        (days as u64)
            .saturating_mul(86_400)
            .saturating_add(hour.saturating_mul(3600))
            .saturating_add(minute.saturating_mul(60))
            .saturating_add(second),
    )
}

pub(crate) fn format_time(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub(super) fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}
pub(super) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}

/// Compare the parts of a boundary supplied by the harness observation and prepared-input
/// contract. `control_version` is deliberately omitted because accepting a commit increments
/// that local CAS version while the continuation remains valid for the same observed boundary.
pub(super) fn same_external_boundary(left: &Boundary, right: &Boundary) -> bool {
    left.scope == right.scope
        && left.state_id == right.state_id
        && left.generation == right.generation
        && left.observation_sha256 == right.observation_sha256
        && left.catalog_sha256 == right.catalog_sha256
        && left.controller_epoch == right.controller_epoch
        && left.gate_epoch == right.gate_epoch
        && left.lease_epoch == right.lease_epoch
        && left.adapter_revision == right.adapter_revision
        && left.adapter_sha256 == right.adapter_sha256
        && left.model == right.model
        && left.configuration_sha256 == right.configuration_sha256
        && left.output_schema_sha256 == right.output_schema_sha256
        && left.capabilities_sha256 == right.capabilities_sha256
        && left.authorization_policy_version == right.authorization_policy_version
}
