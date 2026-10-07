use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Event {
    seq: u64,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    operation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    beat: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct EventContext {
    actor: Option<String>,
    definition_id: Option<String>,
    command: Option<String>,
    callback: Option<String>,
    source: Option<String>,
    line: Option<u64>,
}

#[derive(Debug, Serialize)]
struct CommandTrack {
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_id: Option<String>,
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    runs: Vec<(u64, Option<Value>, Option<Value>)>,
}

#[derive(Debug, Serialize)]
struct OperationTrack {
    actor: String,
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    samples: Vec<OperationSample>,
}

type OperationSample = (u64, Option<Value>, Option<Value>, Vec<Value>);
type TimelineSample = (
    u64,
    Option<Value>,
    Option<Value>,
    Vec<Value>,
    Option<Map<String, Value>>,
);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TimelineContext {
    kind: String,
    operation: String,
    context: EventContext,
}

#[derive(Debug, Serialize)]
struct TimelineTrack {
    kind: String,
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    samples: Vec<TimelineSample>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TweenContext {
    actor: String,
    kind: String,
    easing: Option<String>,
    definition_id: Option<String>,
    command: Option<String>,
    callback: Option<String>,
    source: Option<String>,
    line: Option<u64>,
}

#[derive(Debug, Serialize)]
struct TweenTrack {
    actor: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    easing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    segments: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct Definition {
    id: String,
    #[serde(default)]
    children: Vec<DefinitionChild>,
    #[serde(default)]
    runtime_actors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct DefinitionChild {
    layer_index: usize,
    definition_id: String,
}

#[derive(Clone, Debug, Serialize)]
struct TweenOperation {
    seq: u64,
    operation: String,
    args: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_tween_time_left: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
struct TweenSegment {
    actor: String,
    kind: &'static str,
    duration: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    easing: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    easing_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<Value>,
    enqueue_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    beat: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_tween_time_left: Option<Value>,
    #[serde(skip)]
    observed_tween_time_left: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    implicit: bool,
    operations: Vec<TweenOperation>,
}

#[derive(Debug, Serialize)]
struct TweenControl {
    actor: String,
    operation: String,
    seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    beat: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    factor: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_tween_time_left: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
struct DrawChild {
    definition_id: String,
    layer_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    draw_order: f64,
    draw_index: usize,
}

#[derive(Deserialize)]
struct RuntimeActor {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct DrawSnapshot {
    cause: &'static str,
    seq: u64,
    children: Vec<DrawChild>,
}

#[derive(Debug, Serialize)]
struct DrawOrder {
    parent_definition_id: String,
    parent_actor: String,
    instance: usize,
    mode: &'static str,
    snapshots: Vec<DrawSnapshot>,
    final_children: Vec<DrawChild>,
}

pub fn enrich(document: &mut Value) -> Result<(), Error> {
    let events = read_array::<Event>(document, "events")?;
    let definitions = read_array::<Definition>(document, "actor_definitions")?;
    let actors = if document.get("runtime_actors").is_some() {
        read_array::<RuntimeActor>(document, "runtime_actors")?
    } else {
        Vec::new()
    };
    let (segments, controls, validates_time) = build_tweens(&events);
    let draw_orders = build_draw_orders(&definitions, &actors, &events);
    let projected_layout = document
        .get("projected_vertex_tracks")
        .and_then(Value::as_array)
        .and_then(|tracks| tracks.first())
        .and_then(|track| track.get("sample_layout"))
        .cloned()
        .unwrap_or_else(|| {
            json!([
                "beat",
                "seconds",
                "visible",
                "alpha",
                "world_vertices",
                "clip_vertices",
                "screen_vertices",
                "camera"
            ])
        });
    let object = document
        .as_object_mut()
        .ok_or_else(|| Error("semantic trace root is not an object".into()))?;
    object.insert("tween_segments".into(), to_value(segments)?);
    object.insert("tween_queue_controls".into(), to_value(controls)?);
    object.insert("draw_orders".into(), to_value(draw_orders)?);
    object.insert(
        "semantic_derivation".into(),
        json!({
            "tween_model": "Actor::BeginTweening and Actor::DestTweenState",
            "draw_model": "ActorFrame::SortByDrawOrder stable sort",
            "render_model": {
                "actor_transform_order": ["translation", "rotation_xyz", "scale", "alignment", "skew"],
                "rotation_xyz": "RageMatrixRotationX * RageMatrixRotationY * RageMatrixRotationZ",
                "vector_transform": "RageVec4TransformCoord",
                "perspective": "ActorFrame::BeginDraw -> RageDisplay::LoadMenuPerspective",
                "projection_width_basis": "SCREEN_WIDTH",
                "projected_sample_layout": projected_layout,
                "projected_vertex_order": ["top_left", "top_right", "bottom_right", "bottom_left"],
                "player_render_rule": "source Player visible or a visible ActorProxy targeting it"
            },
            "source_files": [
                "src/Actor.cpp",
                "src/Actor.h",
                "src/ActorFrame.cpp",
                "src/RageDisplay.cpp",
                "src/RageMath.cpp"
            ],
        }),
    );
    let has_projected_samples = object
        .get("projected_vertex_tracks")
        .and_then(Value::as_array)
        .is_some_and(|tracks| !tracks.is_empty());
    let capabilities = object
        .entry("capabilities")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error("semantic trace capabilities is not an object".into()))?;
    capabilities.insert("tween_segments".into(), Value::Bool(true));
    capabilities.insert("tween_time_validation".into(), Value::Bool(validates_time));
    capabilities.insert("stable_draw_order".into(), Value::Bool(true));
    capabilities.insert("render_state_calls".into(), Value::Bool(true));
    capabilities.insert("source_derived_transform_model".into(), Value::Bool(true));
    capabilities.insert(
        "projected_vertex_samples".into(),
        Value::Bool(has_projected_samples),
    );
    capabilities.insert("raster_output".into(), Value::Bool(false));
    capabilities.insert("native_tween_queue_memory".into(), Value::Bool(false));
    capabilities.insert("native_draw_list_memory".into(), Value::Bool(false));
    Ok(())
}

pub fn compact_fixture(document: &mut Value) -> Result<(), Error> {
    let events = read_array::<Event>(document, "events")?;
    let emitted_event_count = document
        .get("emitted_event_count")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| u64::try_from(events.len()).unwrap_or(u64::MAX));
    let callback_operation_tracks = document
        .get("callback_operation_tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tween_sequences = tween_sequences(document)?;
    let mut command_tracks = Vec::<CommandTrack>::new();
    let mut command_indexes = HashMap::<EventContext, usize>::new();
    let mut operation_tracks = Vec::<OperationTrack>::new();
    let mut operation_indexes = HashMap::<(EventContext, String), usize>::new();
    let mut timeline_tracks = Vec::<TimelineTrack>::new();
    let mut timeline_indexes = HashMap::<TimelineContext, usize>::new();

    for event in &events {
        if event.operation == "command.begin" {
            let Some(command) = event.command.clone() else {
                continue;
            };
            let context = event_context(event);
            let index = *command_indexes.entry(context.clone()).or_insert_with(|| {
                let index = command_tracks.len();
                command_tracks.push(CommandTrack {
                    actor: context.actor,
                    definition_id: context.definition_id,
                    command,
                    source: context.source,
                    line: context.line,
                    runs: Vec::new(),
                });
                index
            });
            command_tracks[index]
                .runs
                .push((event.seq, event.beat.clone(), event.seconds.clone()));
        } else if event.kind == "call" && !tween_sequences.contains(&event.seq) {
            let Some(actor) = event.actor.clone() else {
                push_timeline(event, &mut timeline_tracks, &mut timeline_indexes);
                continue;
            };
            let context = event_context(event);
            let key = (context.clone(), event.operation.clone());
            let index = *operation_indexes.entry(key).or_insert_with(|| {
                let index = operation_tracks.len();
                operation_tracks.push(OperationTrack {
                    actor,
                    operation: event.operation.clone(),
                    definition_id: context.definition_id,
                    command: context.command,
                    callback: context.callback,
                    source: context.source,
                    line: context.line,
                    samples: Vec::new(),
                });
                index
            });
            operation_tracks[index].samples.push((
                event.seq,
                event.beat.clone(),
                event.seconds.clone(),
                event.args.clone(),
            ));
        } else if !matches!(
            event.operation.as_str(),
            "command.end" | "command.error" | "callback.error"
        ) && event.kind != "call"
        {
            push_timeline(event, &mut timeline_tracks, &mut timeline_indexes);
        }
    }

    let tween_tracks = compact_tweens(document)?;
    let mut compact_operations = to_value(operation_tracks)?;
    compact_operations
        .as_array_mut()
        .expect("serialized operation tracks must be an array")
        .extend(callback_operation_tracks);

    let object = document
        .as_object_mut()
        .ok_or_else(|| Error("semantic trace root is not an object".into()))?;
    object.remove("events");
    object.remove("callback_operation_tracks");
    object.remove("emitted_event_count");
    object.remove("tween_segments");
    object.insert("command_tracks".into(), to_value(command_tracks)?);
    object.insert("operation_tracks".into(), compact_operations);
    object.insert("timeline_tracks".into(), to_value(timeline_tracks)?);
    object.insert("tween_tracks".into(), to_value(tween_tracks)?);
    object.insert(
        "compact_event_layouts".into(),
        json!({
            "command_tracks.runs": ["seq", "beat", "seconds"],
            "operation_tracks.samples": ["seq", "beat", "seconds", "args"],
            "timeline_tracks.samples": ["seq", "beat", "seconds", "args", "detail"],
        }),
    );
    object.insert("raw_event_count".into(), Value::from(emitted_event_count));
    let capabilities = object
        .get_mut("capabilities")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| Error("semantic trace capabilities is not an object".into()))?;
    capabilities.insert("raw_event_stream".into(), Value::Bool(false));
    capabilities.insert("compact_event_tracks".into(), Value::Bool(true));
    Ok(())
}

fn push_timeline(
    event: &Event,
    tracks: &mut Vec<TimelineTrack>,
    indexes: &mut HashMap<TimelineContext, usize>,
) {
    let context = event_context(event);
    let key = TimelineContext {
        kind: event.kind.clone(),
        operation: event.operation.clone(),
        context: context.clone(),
    };
    let index = *indexes.entry(key).or_insert_with(|| {
        let index = tracks.len();
        tracks.push(TimelineTrack {
            kind: event.kind.clone(),
            operation: event.operation.clone(),
            actor: context.actor,
            definition_id: context.definition_id,
            command: context.command,
            callback: context.callback,
            source: context.source,
            line: context.line,
            samples: Vec::new(),
        });
        index
    });
    tracks[index].samples.push((
        event.seq,
        event.beat.clone(),
        event.seconds.clone(),
        event.args.clone(),
        event.detail.clone(),
    ));
}

fn compact_tweens(document: &Value) -> Result<Vec<TweenTrack>, Error> {
    let segments = document
        .get("tween_segments")
        .and_then(Value::as_array)
        .ok_or_else(|| Error("semantic trace has no `tween_segments` array".into()))?;
    let mut tracks = Vec::<TweenTrack>::new();
    let mut indexes = HashMap::<TweenContext, usize>::new();
    for segment in segments {
        let object = segment
            .as_object()
            .ok_or_else(|| Error("semantic tween segment is not an object".into()))?;
        let context = TweenContext {
            actor: required_string(object, "actor")?,
            kind: required_string(object, "kind")?,
            easing: optional_string(object, "easing"),
            definition_id: optional_string(object, "definition_id"),
            command: optional_string(object, "command"),
            callback: optional_string(object, "callback"),
            source: optional_string(object, "source"),
            line: object.get("line").and_then(Value::as_u64),
        };
        let index = *indexes.entry(context.clone()).or_insert_with(|| {
            let index = tracks.len();
            tracks.push(TweenTrack {
                actor: context.actor.clone(),
                kind: context.kind.clone(),
                easing: context.easing.clone(),
                definition_id: context.definition_id.clone(),
                command: context.command.clone(),
                callback: context.callback.clone(),
                source: context.source.clone(),
                line: context.line,
                segments: Vec::new(),
            });
            index
        });
        let mut compact = object.clone();
        for key in [
            "actor",
            "kind",
            "easing",
            "definition_id",
            "command",
            "callback",
            "source",
            "line",
        ] {
            compact.remove(key);
        }
        tracks[index].segments.push(Value::Object(compact));
    }
    Ok(tracks)
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, Error> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error(format!("semantic tween segment has no `{key}` string")))
}

fn optional_string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn event_context(event: &Event) -> EventContext {
    EventContext {
        actor: event.actor.clone(),
        definition_id: event.definition_id.clone(),
        command: event.command.clone(),
        callback: event.callback.clone(),
        source: event.source.clone(),
        line: event.line,
    }
}

fn tween_sequences(document: &Value) -> Result<HashSet<u64>, Error> {
    let mut out = HashSet::new();
    let segments = document
        .get("tween_segments")
        .and_then(Value::as_array)
        .ok_or_else(|| Error("semantic trace has no `tween_segments` array".into()))?;
    for segment in segments {
        if let Some(seq) = segment.get("enqueue_seq").and_then(Value::as_u64) {
            out.insert(seq);
        }
        if let Some(operations) = segment.get("operations").and_then(Value::as_array) {
            out.extend(
                operations
                    .iter()
                    .filter_map(|operation| operation.get("seq").and_then(Value::as_u64)),
            );
        }
    }
    Ok(out)
}

const fn is_false(value: &bool) -> bool {
    !*value
}

fn read_array<T: for<'de> Deserialize<'de>>(document: &Value, key: &str) -> Result<Vec<T>, Error> {
    serde_json::from_value(
        document
            .get(key)
            .cloned()
            .ok_or_else(|| Error(format!("semantic trace has no `{key}` array")))?,
    )
    .map_err(|source| Error(format!("invalid semantic trace `{key}`: {source}")))
}

fn to_value(value: impl Serialize) -> Result<Value, Error> {
    serde_json::to_value(value)
        .map_err(|source| Error(format!("could not serialize semantic analysis: {source}")))
}

fn native_time(event: &Event) -> Option<Value> {
    event
        .detail
        .as_ref()
        .and_then(|detail| detail.get("native_tween_time_left"))
        .cloned()
}

fn observed_tween_time(event: &Event) -> Option<f64> {
    event.detail.as_ref().and_then(|detail| {
        detail
            .get("native_tween_time_left")
            .or_else(|| detail.get("headless_tween_time_left"))
            .and_then(Value::as_f64)
    })
}

fn segment(
    event: &Event,
    kind: &'static str,
    duration: f64,
    easing: Option<&'static str>,
) -> TweenSegment {
    TweenSegment {
        actor: event.actor.clone().expect("segments require an actor"),
        kind,
        duration,
        easing,
        easing_value: None,
        name: None,
        enqueue_seq: event.seq,
        beat: event.beat.clone(),
        seconds: event.seconds.clone(),
        definition_id: event.definition_id.clone(),
        command: event.command.clone(),
        callback: event.callback.clone(),
        source: event.source.clone(),
        line: event.line,
        native_tween_time_left: native_time(event),
        observed_tween_time_left: observed_tween_time(event),
        implicit: false,
        operations: Vec::new(),
    }
}

fn tween_start(method: &str) -> Option<&'static str> {
    match method {
        "linear" => Some("linear"),
        "accelerate" => Some("accelerate"),
        "decelerate" => Some("decelerate"),
        "smooth" => Some("smooth"),
        "spring" => Some("spring"),
        "bouncebegin" => Some("bouncebegin"),
        "bounceend" => Some("bounceend"),
        "tween" => Some("custom"),
        _ => None,
    }
}

fn is_tween_control(method: &str) -> bool {
    matches!(method, "stoptweening" | "finishtweening" | "hurrytweening")
}

fn is_actor_state_call(event: &Event, method: &str) -> bool {
    event
        .actor
        .as_deref()
        .is_some_and(|actor| actor.starts_with("def-"))
        && !matches!(
            method,
            "draworder"
                | "name"
                | "playcommand"
                | "addcommand"
                | "removecommand"
                | "setupdatefunction"
                | "setdrawfunction"
                | "sortbydraworder"
                | "setdrawbyzposition"
        )
}

fn build_tweens(events: &[Event]) -> (Vec<TweenSegment>, Vec<TweenControl>, bool) {
    let mut segments = Vec::<TweenSegment>::new();
    let mut controls = Vec::new();
    let mut active = HashMap::<String, usize>::new();
    let mut validates_time = false;
    for event in events {
        if matches!(event.operation.as_str(), "command.begin" | "callback.begin") {
            active.retain(|_, index| segments[*index].kind != "immediate");
            if let Some(actor) = event.actor.as_deref() {
                active.remove(actor);
            }
            continue;
        }
        if matches!(
            event.operation.as_str(),
            "command.end" | "command.error" | "callback.end" | "callback.error"
        ) {
            if let Some(actor) = event.actor.as_deref() {
                active.remove(actor);
            }
            continue;
        }
        let Some(actor) = event.actor.as_ref().filter(|_| event.kind == "call") else {
            continue;
        };
        validates_time |= native_time(event).is_some();
        let method = event
            .operation
            .rsplit('.')
            .next()
            .unwrap_or(&event.operation);
        let method = method.to_ascii_lowercase();
        if let Some(easing) = tween_start(&method) {
            let mut value = segment(event, "tween", arg_f64(event, 0), Some(easing));
            if method == "tween" {
                value.easing_value = event.args.get(1).cloned();
            }
            push_active(&mut segments, &mut active, actor, value);
        } else if method == "sleep" {
            push_active(
                &mut segments,
                &mut active,
                actor,
                segment(event, "sleep", arg_f64(event, 0), Some("linear")),
            );
            let mut boundary = segment(event, "tween", 0.0, Some("linear"));
            boundary.implicit = true;
            push_active(&mut segments, &mut active, actor, boundary);
        } else if matches!(method.as_str(), "queuecommand" | "queuemessage") {
            let kind = if method == "queuecommand" {
                "command"
            } else {
                "message"
            };
            let mut value = segment(event, kind, 0.0, Some("linear"));
            value.name = event.args.first().cloned();
            push_active(&mut segments, &mut active, actor, value);
        } else if is_tween_control(&method) {
            controls.push(TweenControl {
                actor: actor.clone(),
                operation: method.clone(),
                seq: event.seq,
                beat: event.beat.clone(),
                seconds: event.seconds.clone(),
                factor: (method == "hurrytweening")
                    .then(|| event.args.first().cloned())
                    .flatten(),
                native_tween_time_left: native_time(event),
            });
            if method != "hurrytweening" {
                active.remove(actor);
            }
        } else if is_actor_state_call(event, &method) {
            push_state_operation(event, actor, &mut segments, &mut active);
        }
    }
    (segments, controls, validates_time)
}

fn push_active(
    segments: &mut Vec<TweenSegment>,
    active: &mut HashMap<String, usize>,
    actor: &str,
    value: TweenSegment,
) {
    let index = segments.len();
    segments.push(value);
    active.insert(actor.to_owned(), index);
}

fn push_state_operation(
    event: &Event,
    actor: &str,
    segments: &mut Vec<TweenSegment>,
    active: &mut HashMap<String, usize>,
) {
    let expired = active.get(actor).is_some_and(|&index| {
        observed_tween_time(event) == Some(0.0)
            && segments[index]
                .observed_tween_time_left
                .is_some_and(|value| value > 0.0)
    });
    if expired {
        active.remove(actor);
    }
    let index = active.get(actor).copied().unwrap_or_else(|| {
        let index = segments.len();
        segments.push(segment(event, "immediate", 0.0, None));
        active.insert(actor.to_owned(), index);
        index
    });
    segments[index].operations.push(TweenOperation {
        seq: event.seq,
        operation: event.operation.clone(),
        args: event.args.clone(),
        native_tween_time_left: native_time(event),
    });
}

fn arg_f64(event: &Event, index: usize) -> f64 {
    event.args.get(index).and_then(Value::as_f64).unwrap_or(0.0)
}

fn build_draw_orders(
    definitions: &[Definition],
    actors: &[RuntimeActor],
    events: &[Event],
) -> Vec<DrawOrder> {
    let by_id = definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let mut by_actor = HashMap::<&str, Vec<&Event>>::new();
    for event in events {
        if let Some(actor) = event.actor.as_deref() {
            by_actor.entry(actor).or_default().push(event);
        }
    }
    let end_seq = events.last().map_or(1, |event| event.seq.saturating_add(1));
    let mut children = HashMap::<&str, Vec<&str>>::new();
    for actor in actors {
        if let Some(parent) = actor.parent_id.as_deref() {
            children.entry(parent).or_default().push(&actor.id);
        }
    }
    let mut out = Vec::new();
    for parent in definitions
        .iter()
        .filter(|definition| !definition.children.is_empty())
    {
        for (instance, parent_actor) in parent.runtime_actors.iter().enumerate() {
            out.push(draw_order_for(
                parent,
                parent_actor,
                instance,
                &by_id,
                &by_actor,
                children.get(parent_actor.as_str()).map(Vec::as_slice),
                end_seq,
            ));
        }
    }
    out
}

fn draw_order_for(
    parent: &Definition,
    parent_actor: &str,
    instance: usize,
    definitions: &HashMap<&str, &Definition>,
    events: &HashMap<&str, Vec<&Event>>,
    actors: Option<&[&str]>,
    end_seq: u64,
) -> DrawOrder {
    let parent_events = events
        .get(parent_actor)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let load_seq = parent_events.first().map_or(end_seq, |event| event.seq);
    let mut children = parent.children.clone();
    children.sort_by_key(|child| child.layer_index);
    let mut order = stable_draw_sort(&children, load_seq, instance, definitions, events, actors);
    let mut snapshots = vec![DrawSnapshot {
        cause: "native_load",
        seq: load_seq,
        children: order.clone(),
    }];
    let mut draw_by_z = false;
    for event in parent_events {
        if event.operation == "ActorFrame.SortByDrawOrder" {
            order = stable_resort(&order, event.seq, events);
            snapshots.push(DrawSnapshot {
                cause: "SortByDrawOrder",
                seq: event.seq,
                children: order.clone(),
            });
        } else if event.operation == "ActorFrame.SetDrawByZPosition" {
            draw_by_z = event.args.first().and_then(Value::as_bool).unwrap_or(false);
        }
    }
    DrawOrder {
        parent_definition_id: parent.id.clone(),
        parent_actor: parent_actor.to_owned(),
        instance: instance + 1,
        mode: if draw_by_z {
            "z_position_each_draw"
        } else {
            "stable_draw_order"
        },
        snapshots,
        final_children: order,
    }
}

fn stable_draw_sort(
    children: &[DefinitionChild],
    seq: u64,
    instance: usize,
    definitions: &HashMap<&str, &Definition>,
    events: &HashMap<&str, Vec<&Event>>,
    actors: Option<&[&str]>,
) -> Vec<DrawChild> {
    let mut out = children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let actor = actors
                .and_then(|actors| actors.get(index))
                .map(|actor| (*actor).to_owned())
                .or_else(|| {
                    definitions
                        .get(child.definition_id.as_str())
                        .and_then(|definition| {
                            definition
                                .runtime_actors
                                .get(instance)
                                .or_else(|| definition.runtime_actors.first())
                        })
                        .cloned()
                });
            DrawChild {
                definition_id: child.definition_id.clone(),
                layer_index: child.layer_index,
                draw_order: draw_order_at(actor.as_deref(), seq, events),
                actor,
                draw_index: 0,
            }
        })
        .collect::<Vec<_>>();
    stable_sort(&mut out);
    out
}

fn stable_resort(
    children: &[DrawChild],
    seq: u64,
    events: &HashMap<&str, Vec<&Event>>,
) -> Vec<DrawChild> {
    let mut out = children.to_vec();
    for child in &mut out {
        child.draw_order = draw_order_at(child.actor.as_deref(), seq, events);
    }
    stable_sort(&mut out);
    out
}

fn stable_sort(children: &mut [DrawChild]) {
    children.sort_by(|left, right| {
        left.draw_order
            .partial_cmp(&right.draw_order)
            .unwrap_or(Ordering::Equal)
    });
    for (index, child) in children.iter_mut().enumerate() {
        child.draw_index = index + 1;
    }
}

fn draw_order_at(actor: Option<&str>, seq: u64, events: &HashMap<&str, Vec<&Event>>) -> f64 {
    actor
        .and_then(|actor| events.get(actor))
        .into_iter()
        .flatten()
        .take_while(|event| event.seq <= seq)
        .filter(|event| event.operation == "Actor.draworder")
        .filter_map(|event| event.args.first().and_then(Value::as_f64))
        .last()
        .unwrap_or(0.0)
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_tween_segments_and_native_duration_samples() {
        let mut trace = json!({
            "actor_definitions": [],
            "events": [
                event(1, "Actor.decelerate", json!([0.6]), 0.6),
                event(2, "Actor.addrotationz", json!([360]), 0.6),
                event(3, "Actor.linear", json!([0.35]), 0.95),
                event(4, "Actor.zoom", json!([1.15]), 0.95)
            ]
        });
        enrich(&mut trace).unwrap();
        let segments = trace["tween_segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["easing"], "decelerate");
        assert_eq!(
            segments[0]["operations"][0]["operation"],
            "Actor.addrotationz"
        );
        assert_eq!(segments[1]["easing"], "linear");
        assert_eq!(segments[1]["operations"][0]["operation"], "Actor.zoom");
        assert_eq!(trace["capabilities"]["tween_time_validation"], true);
        assert_eq!(trace["capabilities"]["render_state_calls"], true);
        assert_eq!(trace["capabilities"]["projected_vertex_samples"], false);
        assert_eq!(
            trace["semantic_derivation"]["render_model"]["projection_width_basis"],
            "SCREEN_WIDTH"
        );
    }

    #[test]
    fn derives_native_stable_draw_order() {
        let mut trace = json!({
            "actor_definitions": [
                {"id":"root", "runtime_actors":["root"], "children":[
                    {"layer_index":1,"definition_id":"a"},
                    {"layer_index":2,"definition_id":"b"},
                    {"layer_index":3,"definition_id":"c"}
                ]},
                {"id":"a", "runtime_actors":["a"], "children":[]},
                {"id":"b", "runtime_actors":["b"], "children":[]},
                {"id":"c", "runtime_actors":["c"], "children":[]}
            ],
            "events": [
                raw_event(1, "b", "Actor.draworder", json!([-1])),
                raw_event(2, "a", "Actor.draworder", json!([2])),
                raw_event(3, "root", "command.begin", json!([]))
            ]
        });
        enrich(&mut trace).unwrap();
        let children = trace["draw_orders"][0]["final_children"]
            .as_array()
            .unwrap();
        assert_eq!(children[0]["definition_id"], "b");
        assert_eq!(children[1]["definition_id"], "c");
        assert_eq!(children[2]["definition_id"], "a");
        assert_eq!(children[0]["draw_index"], 1);
    }

    #[test]
    fn compacts_fixture_events_into_comparable_tracks() {
        let mut trace = json!({
            "actor_definitions": [],
            "events": [
                {
                    "seq": 1, "kind": "command", "actor": "def-player",
                    "operation": "command.begin", "command": "OnCommand",
                    "beat": 0, "seconds": 0
                },
                raw_event(2, "def-player", "Actor.zoom", json!([1.5])),
                {
                    "seq": 3, "kind": "message", "operation": "MessageManager.Broadcast",
                    "args": ["Pulse"], "beat": 4, "seconds": 2
                },
                {
                    "seq": 4, "kind": "command", "actor": "def-player",
                    "operation": "command.end", "command": "OnCommand",
                    "beat": 0, "seconds": 0
                }
            ]
        });
        enrich(&mut trace).unwrap();
        compact_fixture(&mut trace).unwrap();
        assert!(trace.get("events").is_none());
        assert!(trace.get("tween_segments").is_none());
        assert_eq!(trace["raw_event_count"], 4);
        assert_eq!(trace["command_tracks"][0]["command"], "OnCommand");
        assert_eq!(
            trace["tween_tracks"][0]["segments"][0]["operations"][0]["operation"],
            "Actor.zoom"
        );
        assert_eq!(trace["timeline_tracks"][0]["samples"][0][3][0], "Pulse");
    }

    #[test]
    fn repeated_commands_have_independent_tween_queues() {
        let mut trace = json!({
            "actor_definitions": [],
            "events": [
                command_event(1, "command.begin"),
                command_call(2, "Actor.diffusealpha", json!([0.5])),
                command_call(3, "Actor.linear", json!([0.3])),
                command_call(4, "Actor.diffusealpha", json!([0.0])),
                command_event(5, "command.end"),
                command_event(6, "command.begin"),
                command_call(7, "Actor.diffusealpha", json!([0.5])),
                command_call(8, "Actor.linear", json!([0.3])),
                command_call(9, "Actor.diffusealpha", json!([0.0])),
                command_event(10, "command.end")
            ]
        });
        enrich(&mut trace).unwrap();
        let segments = trace["tween_segments"].as_array().unwrap();
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0]["kind"], "immediate");
        assert_eq!(segments[0]["operations"][0]["args"][0], 0.5);
        assert_eq!(segments[1]["kind"], "tween");
        assert_eq!(segments[1]["operations"][0]["args"][0], 0.0);
        assert_eq!(segments[2]["kind"], "immediate");
        assert_eq!(segments[2]["operations"][0]["args"][0], 0.5);
        assert_eq!(segments[3]["kind"], "tween");
        assert_eq!(segments[3]["operations"][0]["args"][0], 0.0);
    }

    #[test]
    fn repeated_update_commands_split_cross_actor_immediate_state() {
        let mut trace = json!({
            "actor_definitions": [],
            "events": [
                {
                    "seq": 1, "kind": "command", "actor": "external-update",
                    "operation": "command.begin", "command": "UpdateCommand",
                    "beat": 8, "seconds": 3
                },
                {
                    "seq": 2, "kind": "call", "actor": "def-flash",
                    "operation": "Quad.diffusealpha", "args": [1],
                    "command": "UpdateCommand", "beat": 8, "seconds": 3
                },
                {
                    "seq": 3, "kind": "command", "actor": "external-update",
                    "operation": "command.end", "command": "UpdateCommand",
                    "beat": 8, "seconds": 3
                },
                {
                    "seq": 4, "kind": "command", "actor": "external-update",
                    "operation": "command.begin", "command": "UpdateCommand",
                    "beat": 9, "seconds": 3.4
                },
                {
                    "seq": 5, "kind": "call", "actor": "def-flash",
                    "operation": "Quad.diffusealpha", "args": [0.5],
                    "command": "UpdateCommand", "beat": 9, "seconds": 3.4
                },
                {
                    "seq": 6, "kind": "command", "actor": "external-update",
                    "operation": "command.end", "command": "UpdateCommand",
                    "beat": 9, "seconds": 3.4
                }
            ]
        });
        enrich(&mut trace).unwrap();
        let segments = trace["tween_segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["beat"], 8);
        assert_eq!(segments[0]["operations"][0]["args"][0], 1);
        assert_eq!(segments[1]["beat"], 9);
        assert_eq!(segments[1]["operations"][0]["args"][0], 0.5);
    }

    #[test]
    fn completed_commands_do_not_absorb_later_actor_calls() {
        let mut trace = json!({
            "actor_definitions": [],
            "events": [
                command_event(1, "command.begin"),
                command_call(2, "Actor.linear", json!([0.3])),
                command_call(3, "Actor.zoom", json!([0.5])),
                command_event(4, "command.end"),
                raw_event(5, "def-player", "Actor.x", json!([42]))
            ]
        });
        enrich(&mut trace).unwrap();
        let segments = trace["tween_segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["kind"], "tween");
        assert_eq!(segments[0]["operations"][0]["operation"], "Actor.zoom");
        assert_eq!(segments[1]["kind"], "immediate");
        assert_eq!(segments[1]["operations"][0]["operation"], "Actor.x");
    }

    #[test]
    fn completed_tweens_do_not_absorb_later_update_writes() {
        let mut first_write = headless_event(2, "Actor.addx", json!([427]), 0.375);
        first_write["beat"] = json!(111);
        first_write["seconds"] = json!(41.6);
        let mut later_write = headless_event(3, "Actor.zoom", json!([0.94]), 0.0);
        later_write["beat"] = json!(248);
        later_write["seconds"] = json!(93.5);
        let mut trace = json!({
            "actor_definitions": [],
            "events": [
                headless_event(1, "Actor.accelerate", json!([0.375]), 0.375),
                first_write,
                later_write,
            ]
        });

        enrich(&mut trace).unwrap();

        let segments = trace["tween_segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["operations"][0]["operation"], "Actor.addx");
        assert_eq!(segments[1]["kind"], "immediate");
        assert_eq!(segments[1]["beat"], 248);
        assert_eq!(segments[1]["operations"][0]["operation"], "Actor.zoom");
        assert_eq!(trace["capabilities"]["tween_time_validation"], false);
    }

    fn headless_event(seq: u64, operation: &str, args: Value, time_left: f64) -> Value {
        let mut value = raw_event(seq, "def-player", operation, args);
        value["detail"] = json!({"headless_tween_time_left":time_left});
        value
    }

    fn event(seq: u64, operation: &str, args: Value, time_left: f64) -> Value {
        let mut value = raw_event(seq, "def-player", operation, args);
        value["detail"] = json!({"native_tween_time_left":time_left});
        value
    }

    fn raw_event(seq: u64, actor: &str, operation: &str, args: Value) -> Value {
        json!({
            "seq":seq,
            "kind":"call",
            "actor":actor,
            "operation":operation,
            "args":args
        })
    }

    fn command_event(seq: u64, operation: &str) -> Value {
        json!({
            "seq":seq,
            "kind":"command",
            "actor":"def-player",
            "operation":operation,
            "command":"PulseMessageCommand",
            "beat":4,
            "seconds":2
        })
    }

    fn command_call(seq: u64, operation: &str, args: Value) -> Value {
        json!({
            "seq":seq,
            "kind":"call",
            "actor":"def-player",
            "operation":operation,
            "args":args,
            "command":"PulseMessageCommand",
            "beat":4,
            "seconds":2
        })
    }
}
