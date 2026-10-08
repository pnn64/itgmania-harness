use std::fmt;
use std::path::Path;

pub fn evaluate(path: &Path) -> Result<serde_json::Value, Error> {
    let request = std::fs::read(path).map_err(Error::Read)?;
    let response = native_eval(&request)?;
    let mut document: serde_json::Value = serde_json::from_slice(&response).map_err(Error::Json)?;
    if let Some(message) = document.get("error").and_then(serde_json::Value::as_str) {
        return Err(Error::Native(message.to_owned()));
    }
    document["provenance"] = serde_json::json!({
        "source_root": env!("ITGMANIA_BUILD_ROOT"),
        "git_revision": env!("ITGMANIA_GIT_REVISION"),
        "git_dirty": env!("ITGMANIA_GIT_DIRTY"),
        "execution": "embedded_native_sources"
    });
    Ok(document)
}

#[cfg(itgmania_oracle)]
fn native_eval(request: &[u8]) -> Result<Vec<u8>, Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_eval_actor_fixture(request: *const u8, request_len: usize) -> NativeBuffer;
        fn itg_oracle_free(data: *mut u8);
    }

    // SAFETY: the request slice remains alive during the call and its explicit
    // length prevents the native parser from reading beyond it.
    let native = unsafe { itg_oracle_eval_actor_fixture(request.as_ptr(), request.len()) };
    if native.data.is_null() {
        return Err(Error::Native("native actor oracle returned no data".into()));
    }
    // SAFETY: the bridge owns `len` initialized bytes at `data` until the
    // matching bridge free below.
    let bytes = unsafe { std::slice::from_raw_parts(native.data, native.len) }.to_vec();
    // SAFETY: this pointer was returned by the bridge and has not been freed.
    unsafe { itg_oracle_free(native.data) };
    Ok(bytes)
}

#[cfg(not(itgmania_oracle))]
fn native_eval(_request: &[u8]) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
}

#[derive(Debug)]
pub enum Error {
    Read(std::io::Error),
    Native(String),
    Json(serde_json::Error),
    #[cfg(not(itgmania_oracle))]
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "could not read actor fixture: {error}"),
            Self::Native(message) => write!(formatter, "ITGmania actor fixture failed: {message}"),
            Self::Json(error) => write!(
                formatter,
                "native actor oracle returned invalid JSON: {error}"
            ),
            #[cfg(not(itgmania_oracle))]
            Self::Unavailable => write!(
                formatter,
                "the embedded ITGmania actor oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(all(test, itgmania_oracle))]
mod tests {
    use super::*;

    #[test]
    fn sprite_methods_use_native() {
        for name in ["sprite-load", "texture-path"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("fixtures/actors/{name}.json"));
            let result = evaluate(&path).expect("native Sprite assertions");
            for key in ["script_errors", "diagnostics", "allocations"] {
                assert_eq!(result[key], serde_json::json!([]), "{name}: {key}");
            }
        }
    }

    #[test]
    fn bitmap_methods_use_native_userdata() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/bitmap-methods.json");
        let result = evaluate(&path).expect("native BitmapText assertions");
        assert_eq!(result, evaluate(&path).expect("repeat native BitmapText"));
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
        assert_eq!(result["allocations"], serde_json::json!([]));
    }

    #[test]
    fn definitions_use_native_fallback_concatenation() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/actors/definition-concat.json");
        let result = evaluate(&path).expect("native definition concatenation");
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn proxy_methods_use_native_userdata() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/proxy-methods.json");
        let result = evaluate(&path).expect("native ActorProxy assertions");
        assert_eq!(result, evaluate(&path).expect("repeat native ActorProxy"));
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
        assert_eq!(result["allocations"], serde_json::json!([]));
    }

    #[test]
    fn aft_creation_uses_native_allocation_and_lua_methods() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/aft-creation.json");
        let result = evaluate(&path).expect("native AFT allocation assertions");
        assert_eq!(result, evaluate(&path).expect("repeat native AFT allocation"));
        assert_eq!(
            result["allocations"],
            serde_json::json!([
                {"width":65, "height":33, "alpha":true, "depth":true, "float":true},
                {"width":32, "height":16, "alpha":false, "depth":false, "float":false},
                {"width":1, "height":1, "alpha":false, "depth":false, "float":false}
            ])
        );
        assert_eq!(
            result["diagnostics"],
            serde_json::json!([
                "ActorFrameTexture: Cannot have width or height less than 1",
                "Can't Create an already created ActorFrameTexture",
                "ActorFrameTexture: Texture Name already in use."
            ])
        );
        assert_eq!(result["script_errors"], result["diagnostics"]);
    }

    #[test]
    fn queued_size_and_zero_tween_use_native_actors() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/runtime-size-zoom.json");
        let native = evaluate(&path).expect("native queued size");
        assert_eq!(native, evaluate(&path).expect("repeat queued size"));
        for sample in native["samples"].as_array().unwrap() {
            let second = sample["time"].as_f64().unwrap();
            let resized = &sample["actors"][1];
            let expected = if second < 1.0 {
                [1280.0, 720.0]
            } else {
                [854.0, 480.0]
            };
            assert_eq!(resized["size"], serde_json::json!(expected));
            assert_eq!(
                resized["played_commands"].as_array().unwrap().len(),
                usize::from(second >= 1.0)
            );
            if second >= 1.1 {
                assert_eq!(
                    sample["actors"][2]["current"]["zoom"],
                    serde_json::json!([0.0, 0.0, 0.0])
                );
            }
        }
    }

    #[test]
    fn queued_visibility_waits_for_positive_updates() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/queued-visibility.json");
        let native = evaluate(&path).expect("native queued commands");
        assert_eq!(native, evaluate(&path).expect("repeat queued commands"));
        for (index, sample) in native["samples"].as_array().unwrap().iter().enumerate() {
            for actor in sample["actors"].as_array().unwrap().iter().skip(1) {
                let name = actor["name"].as_str().unwrap();
                let expected = match name {
                    "QueuedHide" | "QueuedBuilder" => index < 2,
                    "QueuedShow" => index >= 2,
                    "DelayedRestore" => index < 2 || index >= 5,
                    "ImmediateHide" => false,
                    _ => panic!("unexpected actor {name}"),
                };
                assert_eq!(actor["visible"], expected, "{name} at {}", sample["time"]);
                if name == "ImmediateHide" {
                    continue;
                }
                let count = if index < 2 {
                    0
                } else if name == "DelayedRestore" && index >= 5 {
                    2
                } else {
                    1
                };
                assert_eq!(actor["played_commands"].as_array().unwrap().len(), count);
            }
        }
    }

    #[test]
    fn position_spline_native_samples_are_deterministic() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/position-spline.json");
        let first = evaluate(&path).expect("native spline sample");
        assert_eq!(first, evaluate(&path).expect("repeat native sample"));
        let cases = first["splines"].as_array().expect("spline cases");
        assert_eq!(cases.len(), 42);
        for case in cases {
            for sample in case["samples"].as_array().expect("samples") {
                for key in ["position", "derivative", "receptor"] {
                    assert!(
                        sample[key]
                            .as_array()
                            .expect("vector")
                            .iter()
                            .all(|value| value.as_f64().is_some_and(f64::is_finite))
                    );
                }
            }
        }
    }
}
