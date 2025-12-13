use crate::processing::ProcessedFit;

fn format_duration(seconds: Option<f64>) -> String {
    match seconds {
        Some(total) => {
            let rounded = total.round().max(0.0) as u64;
            let hours = rounded / 3600;
            let minutes = (rounded % 3600) / 60;
            let seconds = rounded % 60;

            if hours > 0 {
                format!("{}h {:02}m {:02}s", hours, minutes, seconds)
            } else {
                format!("{}m {:02}s", minutes, seconds)
            }
        }
        None => "—".to_string(),
    }
}

fn format_distance(meters: Option<f64>) -> String {
    match meters {
        Some(distance) if distance >= 1000.0 => format!("{:.2} km", distance / 1000.0),
        Some(distance) => format!("{:.0} m", distance),
        None => "—".to_string(),
    }
}

fn format_speed(speed: Option<f64>) -> String {
    match speed {
        Some(value) if value > 0.0 => {
            let total_minutes = 1000.0 / (value * 60.0);
            let whole_minutes = total_minutes.floor();
            let mut seconds = ((total_minutes - whole_minutes) * 60.0).round();

            // Account for rounding up to the next minute when seconds hit 60.
            let mut minutes = whole_minutes as u64;
            if seconds >= 60.0 {
                minutes += 1;
                seconds = 0.0;
            }

            format!("{}:{:02} min/km", minutes, seconds as u64)
        }
        _ => "—".to_string(),
    }
}

fn format_heart_rate(value: Option<f64>) -> String {
    match value {
        Some(hr) if hr.is_finite() && hr > 0.0 => format!("{:.0} bpm", hr.round()),
        _ => "—".to_string(),
    }
}

pub fn render_landing_page() -> String {
    include_str!("../templates/landing.html").to_string()
}

pub fn render_processed_records(processed: &ProcessedFit, download_url: &str) -> String {
    let mut body = String::new();

    let summary = &processed.summary;
    let (min_speed, mean_speed, max_speed) = (
        format_speed(summary.speed_min),
        format_speed(summary.speed_mean),
        format_speed(summary.speed_max),
    );
    let (min_hr, mean_hr, max_hr) = (
        format_heart_rate(summary.heart_rate_min),
        format_heart_rate(summary.heart_rate_mean),
        format_heart_rate(summary.heart_rate_max),
    );

    body.push_str("<section class=\"results-card\">");
    body.push_str(
        "<div class=\"results-header\"><div><p class=\"eyebrow\">Workout Overview</p><h2>Freshly parsed FIT file</h2></div>",
    );
    body.push_str(&format!(
        "<a class=\"cta\" download=processed.fit href={download_url}>Download processed FIT</a>"
    ));
    body.push_str("</div>");

    body.push_str("<div class=\"summary-grid\">");
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Workout Duration</p><p class=\"value\">{}</p></div>",
        format_duration(summary.duration_seconds)
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Workout Type</p><p class=\"value\">{}</p></div>",
        summary
            .workout_type
            .as_ref()
            .map(|val| val.clone())
            .unwrap_or_else(|| "Unknown".into())
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Workout Distance</p><p class=\"value\">{}</p></div>",
        format_distance(summary.distance_meters)
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Speed (min)</p><p class=\"value\">{}</p></div>",
        min_speed
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Speed (mean)</p><p class=\"value\">{}</p></div>",
        mean_speed
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Speed (max)</p><p class=\"value\">{}</p></div>",
        max_speed
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Heart Rate (min)</p><p class=\"value\">{}</p></div>",
        min_hr
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Heart Rate (mean)</p><p class=\"value\">{}</p></div>",
        mean_hr
    ));
    body.push_str(&format!(
        "<div class=\"summary-card\"><p class=\"label\">Heart Rate (max)</p><p class=\"value\">{}</p></div>",
        max_hr
    ));
    body.push_str("</div>");
    body.push_str("</section>");

    body.push_str("<section class=\"results-card\">");
    body.push_str(&format!(
        "<div class=\"results-header\"><div><p class=\"eyebrow\">Data records</p><h2>Showing the first 25 of {} records</h2></div></div>",
        processed.records.len()
    ));
    body.push_str("<div class=\"table-wrapper\"><table><thead><tr><th>Message</th><th>Fields</th></tr></thead><tbody>");

    for record in processed.records.iter().take(25) {
        body.push_str(&format!("<tr><td>{}</td><td>", record.message_type));
        body.push_str("<ul>");
        for field in &record.fields {
            body.push_str(&format!(
                "<li><strong>{}</strong>: {}</li>",
                field.name, field.value
            ));
        }
        body.push_str("</ul></td></tr>");
    }

    body.push_str("</tbody></table></div>");
    body.push_str("</section>");
    body
}
