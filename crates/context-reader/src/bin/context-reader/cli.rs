// SPDX-License-Identifier: MIT

use context_reader::SnapshotProjection;

pub(crate) fn print_projection(projection: &SnapshotProjection) {
    println!("snapshot_id={}", projection.snapshot_id);
    println!("boundary={}", projection.boundary);
    println!("capture_mode={}", projection.capture_mode.as_str());
    println!(
        "application_capture_complete={}",
        projection.application_capture_complete
    );
    println!("component_count={}", projection.component_count);
    println!("provider_model={}", projection.provider_model);
    println!(
        "input_measurement_source={}",
        projection.input_measurement.source
    );
}
