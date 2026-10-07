//! The original RSSP baseline contract: engine statistics and selected theme output.
//! This projection never reads RSSP results or recalculates chart metrics.
use crate::oracle::{Chart, Document, NativeText, RawF32};
use serde_json::{json, Value};

const DIFFICULTIES: [&str; 7] = [
    "beginner",
    "easy",
    "medium",
    "hard",
    "challenge",
    "edit",
    "invalid",
];
const SEGMENTS: [&str; 11] = [
    "bpms",
    "stops",
    "delays",
    "time_signatures",
    "warps",
    "labels",
    "tickcounts",
    "combos",
    "speeds",
    "scrolls",
    "fakes",
];

// The previous C++ CLI printed ordinary numbers with ostream's six significant
// digits. Preserve that serialization contract; raw values stay in Document.
fn sig6(value: f64) -> Result<f64, String> {
    if !value.is_finite() {
        return Err("non-finite report value".into());
    }
    format!("{value:.5e}")
        .parse()
        .map_err(|e| format!("format report number: {e}"))
}
fn native(value: RawF32) -> Result<f64, String> {
    sig6(f64::from(value.get()))
}
fn numeric(value: &Value) -> Result<f64, String> {
    sig6(
        value
            .as_f64()
            .ok_or_else(|| "theme omitted a numeric metric".to_string())?,
    )
}
fn text(value: &NativeText) -> Result<String, String> {
    match value {
        NativeText::Utf8(value) => Ok(value
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .to_owned()),
        NativeText::Bytes { raw_bytes_hex } => {
            let bytes = (0..raw_bytes_hex.len())
                .step_by(2)
                .map(|i| {
                    u8::from_str_radix(&raw_bytes_hex[i..i + 2], 16).map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(decode_text(&bytes))
        }
    }
}
// Explicit legacy CLI policy for invalid UTF-8, not a native decoding claim.
fn decode_text(bytes: &[u8]) -> String {
    const CP1252: [char; 32] = [
        '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
        '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
        '\u{fffd}', 'ž', 'Ÿ',
    ];
    let value = std::str::from_utf8(bytes)
        .map(str::to_owned)
        .unwrap_or_else(|_| {
            bytes
                .iter()
                .map(|&b| {
                    if (0x80..=0x9f).contains(&b) {
                        CP1252[(b - 0x80) as usize]
                    } else {
                        char::from(b)
                    }
                })
                .collect()
        });
    value
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_owned()
}
// The old harness recovered literal metadata when MsdFile treated an unescaped
// '#' in a title as a new tag. Keep that report-only policy and the raw capture.
fn raw_tag(bytes: &[u8], tag: &[u8]) -> Option<String> {
    let mut result = None;
    let mut pos = 0;
    while pos + tag.len() <= bytes.len() {
        if !bytes[pos..pos + tag.len()].eq_ignore_ascii_case(tag) {
            pos += 1;
            continue;
        }
        pos += tag.len();
        let mut value = Vec::new();
        while pos < bytes.len() && bytes[pos] != b';' {
            if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                pos += 1;
            }
            value.push(bytes[pos]);
            pos += 1;
        }
        result = Some(decode_text(&value));
        pos += 1;
    }
    result
}

pub fn project(document: &Document, source: &[u8]) -> Result<Vec<Value>, String> {
    if document.theme.is_none() {
        return Err("RSSP baseline output requires --theme".into());
    }
    let charts: Vec<_> = document
        .charts
        .iter()
        .filter(|c| {
            matches!(
                c.steps_type.source.as_str(),
                "dance-single" | "dance-double"
            )
        })
        .collect();
    if charts.is_empty() {
        return Err("simfile contains no supported RSSP dance charts".into());
    }
    charts
        .into_iter()
        .map(|chart| project_chart(chart, source))
        .collect()
}

fn project_chart(chart: &Chart, source: &[u8]) -> Result<Value, String> {
    let theme = chart.theme.as_ref().ok_or("missing theme capture")?;
    if theme["status"] != "complete" {
        return Err(format!(
            "chart {} has incomplete theme results: {}",
            chart.source_index, theme["errors"]
        ));
    }
    let player = &theme["players"][0];
    let streams = &player["streams"];
    let difficulty = *DIFFICULTIES
        .get(chart.difficulty_code as usize)
        .ok_or("invalid difficulty")?;
    let stats = chart
        .player_stats
        .first()
        .ok_or("missing native statistics")?;
    if stats.radar.len() != 14 || stats.tech_counts.len() != 10 {
        return Err("incomplete native statistics".into());
    }
    let mut title = text(&chart.title)?;
    let mut subtitle = text(&chart.subtitle)?;
    let mut artist = text(&chart.artist)?;
    if title.is_empty() {
        if let Some(raw) = raw_tag(source, b"#TITLE:") {
            title = raw;
            subtitle = raw_tag(source, b"#SUBTITLE:").unwrap_or_default();
            if let Some(raw) = raw_tag(source, b"#ARTIST:") {
                artist = raw;
            }
        }
    }
    if artist.is_empty() {
        artist = "Unknown artist".into();
    }
    let translated = |raw: &NativeText, display: &str| -> Result<String, String> {
        let value = text(raw)?;
        Ok(if value.is_empty() {
            display.to_owned()
        } else {
            value
        })
    };
    let title_translated = translated(&chart.title_translit, &title)?;
    let subtitle_translated = translated(&chart.subtitle_translit, &subtitle)?;
    let artist_translated = translated(&chart.artist_translit, &artist)?;
    let nps = streams["NPSperMeasure"]
        .as_array()
        .ok_or("missing theme NPS array")?
        .iter()
        .map(numeric)
        .collect::<Result<Vec<_>, _>>()?;
    let notes = streams["NotesPerMeasure"]
        .as_array()
        .ok_or("missing theme density array")?;
    let sequences = player["stream_sequences"]
        .as_array()
        .ok_or("missing theme stream sequences")?
        .iter()
        .map(|s| {
            json!({"stream_start":s["streamStart"].as_f64().map(|n| n as u32),
            "stream_end":s["streamEnd"].as_f64().map(|n| n as u32), "is_break":s["isBreak"]})
        })
        .collect::<Vec<_>>();
    let mut timing = json!({"beat0_offset_seconds":f64::from(chart.timing.beat0_offset_seconds.get()),
        "beat0_group_offset_seconds":f64::from(chart.timing.beat0_group_offset_seconds.get())});
    for key in SEGMENTS {
        timing[key] = json!([]);
    }
    let mut bpms = Vec::new();
    for segment in &chart.timing.segments {
        let code = segment.segment_type_code as usize;
        let key = *SEGMENTS.get(code).ok_or("invalid timing segment type")?;
        let beat = f64::from(segment.beat.get());
        let mut row = vec![json!(beat)];
        if code == 5 {
            row.push(json!(segment.label));
        } else {
            for (i, value) in segment.values.iter().enumerate() {
                row.push(if matches!(code, 3 | 6 | 7) || code == 8 && i == 2 {
                    json!(value.get() as i32)
                } else {
                    json!(f64::from(value.get()))
                });
            }
        }
        timing[key]
            .as_array_mut()
            .ok_or("invalid timing table")?
            .push(json!(row));
        if code == 0 {
            let value = segment.values.first().ok_or("missing BPM value")?;
            bpms.push(format!("{beat:.6}={:.6}", f64::from(value.get())));
        }
    }
    let mut result = json!({
        "status":"ok", "theme_complete":true, "metadata_utf8":true,
        "steps_type":chart.steps_type.source, "difficulty":difficulty, "meter":chart.meter,
        "title":title, "subtitle":subtitle, "artist":artist,
        "title_translated":title_translated, "subtitle_translated":subtitle_translated, "artist_translated":artist_translated,
        "step_artist":chart.credit, "description":chart.description,
        "hash":streams["Hash"], "hash_bpms":player["hash_bpms"], "bpms":bpms.join(","),
        "bpm_min":native(chart.bpm.actual_min)?, "bpm_max":native(chart.bpm.actual_max)?,
        "display_bpm":theme["display_bpm_text"], "display_bpm_min":numeric(&theme["display_bpms"][0])?,
        "display_bpm_max":numeric(&theme["display_bpms"][1])?,
        "duration_seconds":if chart.note_data.entries.is_empty() { 0.0 } else { native(chart.last_second.ok_or("missing last time")?)? },
        "timing":timing, "notes_per_measure":notes.iter().map(|v| v.as_f64().map(|n| n as u32)).collect::<Vec<_>>(),
        "nps_per_measure":nps, "peak_nps":numeric(&streams["PeakNPS"])?,
        "equally_spaced_per_measure":streams["EquallySpacedPerMeasure"],
        "stream_sequences":sequences, "streams_breakdown":player["breakdowns"][0],
        "streams_breakdown_level1":player["breakdowns"][1], "streams_breakdown_level2":player["breakdowns"][2],
        "streams_breakdown_level3":player["breakdowns"][3],
        "total_stream_measures":player["total_stream_measures"].as_f64().map(|n| n as u32),
        "total_break_measures":player["total_break_measures"].as_f64().map(|n| n as u32),
        "total_steps":notes.iter().filter_map(Value::as_f64).sum::<f64>() as u32
    });
    result["tech_counts"] = json!({"crossovers":stats.tech_counts[0].get() as u32, "footswitches":stats.tech_counts[3].get() as u32,
        "sideswitches":stats.tech_counts[6].get() as u32, "jacks":stats.tech_counts[7].get() as u32,
        "brackets":stats.tech_counts[8].get() as u32, "doublesteps":stats.tech_counts[9].get() as u32});
    result["reference_sources"] = json!({"statistics":"ITGmania", "timing":"ITGmania", "hash":"selected Simply Love",
        "density_nps_streams":"selected Simply Love", "metadata":"legacy CLI trimming and UTF-8/Windows-1252 policy"});
    for (name, index) in [
        ("notes", 5),
        ("taps_and_holds", 6),
        ("jumps", 7),
        ("holds", 8),
        ("mines", 9),
        ("hands", 10),
        ("rolls", 11),
        ("lifts", 12),
        ("fakes", 13),
    ] {
        result[name] = json!(stats.radar[index].get() as u32);
    }
    Ok(result)
}
