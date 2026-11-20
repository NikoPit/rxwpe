use super::*;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SoundLayer {
    pub(crate) id: i64,
    pub(crate) paths: Vec<String>,
    pub(crate) volume: f32,
    pub(crate) looped: bool,
    pub(crate) startsilent: bool,
}

pub(super) fn parse(
    project: &Project,
    object: &Value,
    warnings: &mut Vec<String>,
) -> Result<SoundLayer> {
    let id = object.get("id").and_then(Value::as_i64).unwrap_or(-1);
    let values = object
        .get("sound")
        .and_then(Value::as_array)
        .context("sound object has no asset list")?;
    ensure!(!values.is_empty(), "sound object has no assets");
    let paths = values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .context("sound asset path must be a string")?;
            resolve_texture(project, value)
        })
        .collect::<Result<Vec<_>>>()?;
    let mode = object
        .get("playbackmode")
        .and_then(Value::as_str)
        .unwrap_or("loop");
    ensure!(
        ["loop", "once"].contains(&mode),
        "sound object {id} has unsupported playback mode {mode}"
    );
    let volume = scalar(object.get("volume"), 1., project, warnings, "sound volume")?.clamp(0., 1.);
    let startsilent = boolean(
        object.get("startsilent"),
        false,
        project,
        warnings,
        "sound startsilent",
    )?;
    Ok(SoundLayer {
        id,
        paths,
        volume,
        looped: mode == "loop",
        startsilent,
    })
}
