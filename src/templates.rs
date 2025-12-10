use crate::processing::ProcessedFit;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

pub fn render_landing_page() -> String {
    include_str!("../templates/landing.html").to_string()
}

pub fn render_processed_records(processed: &ProcessedFit) -> String {
    let mut body = String::new();
    let encoded = BASE64_STANDARD.encode(&processed.processed_bytes);

    body.push_str(&format!(
        "<p>Decoded {} data records. Showing up to the first 25.</p>",
        processed.records.len()
    ));
    body.push_str(&format!(
        "<p><a download=\"processed.fit\" href=\"data:application/octet-stream;base64,{encoded}\">\n            <button type=\"button\">Download processed FIT</button>\n        </a></p>"
    ));
    body.push_str("<table><thead><tr><th>Message</th><th>Fields</th></tr></thead><tbody>");

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

    body.push_str("</tbody></table>");
    body
}
