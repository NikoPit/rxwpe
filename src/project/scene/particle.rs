use super::*;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ParticleLayer {
    pub(crate) id: i64,
    pub(crate) order: usize,
    pub(crate) origin: [f32; 3],
    pub(crate) scale: [f32; 3],
    pub(crate) angles: [f32; 3],
    pub(crate) visible: bool,
    pub(crate) alpha: f32,
    pub(crate) color: [f32; 3],
    pub(crate) size: f32,
    pub(crate) lifetime: f32,
    pub(crate) rate: f32,
    pub(crate) max_count: usize,
    pub(crate) start_time: f32,
    pub(crate) texture: String,
    pub(crate) blending: BlendMode,
    pub(crate) animation_mode: String,
    pub(crate) sequence_multiplier: f32,
    pub(crate) emitters: Vec<Emitter>,
    pub(crate) initializers: Vec<Initializer>,
    pub(crate) operators: Vec<Operator>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Emitter {
    pub(crate) sphere: bool,
    pub(crate) directions: [f32; 3],
    pub(crate) min: [f32; 3],
    pub(crate) max: [f32; 3],
    pub(crate) origin: [f32; 3],
    pub(crate) rate: f32,
    pub(crate) instantaneous: usize,
    pub(crate) speed: [f32; 2],
    pub(crate) sign: [f32; 3],
}

#[derive(Debug, Clone, Serialize)]
pub(crate) enum Initializer {
    Lifetime([f32; 2]),
    Size([f32; 2], f32),
    Alpha([f32; 2]),
    Color([[f32; 3]; 2]),
    Rotation([[f32; 3]; 2]),
    Velocity([[f32; 3]; 2]),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) enum Operator {
    Movement { gravity: [f32; 3], drag: f32 },
    AlphaFade { fade_in: f32, fade_out: f32 },
}

pub(super) fn parse(
    project: &Project,
    object: &Value,
    path: &str,
    order: usize,
    warnings: &mut Vec<String>,
) -> Result<ParticleLayer> {
    let source = read_json(&project.assets, path)?;
    let id = object.get("id").and_then(Value::as_i64).unwrap_or(-1);
    let warn = |warnings: &mut Vec<String>, message: String| {
        warnings.push(format!("particle {id}: {message}"))
    };
    let number = |o: &Value, key: &str, default, warnings: &mut Vec<String>| {
        scalar(o.get(key), default, project, warnings, key)
    };
    let vec = |o: &Value, key: &str, default, warnings: &mut Vec<String>| {
        vector(o.get(key), default, project, warnings, key)
    };
    let range = |o: &Value, a, b, warnings: &mut Vec<String>| -> Result<[f32; 2]> {
        let v = [
            number(o, "min", a, warnings)?,
            number(o, "max", b, warnings)?,
        ];
        ensure!(v[0] <= v[1], "particle {id}: minimum exceeds maximum");
        Ok(v)
    };
    if source.get("flags").and_then(Value::as_u64).unwrap_or(0) != 0 {
        warn(
            warnings,
            "nonzero particle flags are not implemented".into(),
        );
    }
    for key in ["children", "child", "constraint"] {
        if source.get(key).is_some_and(has_values) {
            warn(warnings, format!("{key} are not implemented"));
        }
    }
    if object.get("parent").is_some_and(|v| !v.is_null()) {
        warn(warnings, "parent transforms are not implemented".into());
    }
    let material = read_json(
        &project.assets,
        source
            .get("material")
            .and_then(Value::as_str)
            .context("particle has no material")?,
    )?;
    let passes = material
        .get("passes")
        .and_then(Value::as_array)
        .context("particle material has no passes")?;
    let pass = passes.first().context("particle material is empty")?;
    if passes.len() != 1 {
        warn(
            warnings,
            "multiple material passes are not implemented".into(),
        );
    }
    if pass.get("shader").and_then(Value::as_str) != Some("genericparticle") {
        warn(
            warnings,
            "only genericparticle materials are implemented".into(),
        );
    }
    if pass.get("combos").is_some_and(has_values)
        || pass.get("constantshadervalues").is_some_and(has_values)
    {
        warn(
            warnings,
            "custom particle material combos/constants are not implemented".into(),
        );
    }
    let texture = resolve_texture(
        project,
        pass.pointer("/textures/0")
            .and_then(Value::as_str)
            .context("particle material has no texture")?,
    )?;
    let blending = match pass
        .get("blending")
        .and_then(Value::as_str)
        .unwrap_or("translucent")
    {
        "additive" => BlendMode::Additive,
        "translucent" => BlendMode::Translucent,
        "normal" => BlendMode::Normal,
        "opaque" => BlendMode::Opaque,
        other => {
            warn(warnings, format!("blending {other} is not implemented"));
            BlendMode::Translucent
        }
    };
    let mut emitters = Vec::new();
    for emitter in source
        .get("emitter")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = emitter.get("name").and_then(Value::as_str).unwrap_or("");
        if !["sphererandom", "boxrandom"].contains(&kind) {
            warn(warnings, format!("emitter {kind} is not implemented"));
            continue;
        }
        for field in ["flags", "delay", "duration", "cone", "audioprocessingmode"] {
            if number(emitter, field, 0., warnings)? != 0. {
                warn(warnings, format!("emitter {field} is not implemented"));
            }
        }
        let mut origin = vec(emitter, "origin", [0.; 3], warnings)?;
        let cp = emitter
            .get("controlpoint")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if let Some(point) = source
            .get("controlpoint")
            .and_then(Value::as_array)
            .and_then(|v| {
                v.iter()
                    .find(|p| p.get("id").and_then(Value::as_i64) == Some(cp))
            })
        {
            if number(point, "flags", 0., warnings)? != 0. {
                warn(warnings, "linked control points are not implemented".into());
            }
            let offset = vec(point, "offset", [0.; 3], warnings)?;
            for i in 0..3 {
                origin[i] += offset[i];
            }
        }
        let distance = |key, default, warnings: &mut Vec<String>| -> Result<[f32; 3]> {
            let value = effective(emitter.get(key), project, warnings, key);
            if value.is_some_and(Value::is_number) {
                Ok([number(emitter, key, default, warnings)?; 3])
            } else {
                vec(emitter, key, [default; 3], warnings)
            }
        };
        let min = distance("distancemin", 0., warnings)?;
        let max = distance("distancemax", 256., warnings)?;
        ensure!(
            (0..3).all(|i| min[i] >= 0. && min[i] <= max[i]),
            "invalid particle emission bounds"
        );
        let rate = number(emitter, "rate", 10., warnings)?;
        ensure!(
            (0. ..=100000.).contains(&rate),
            "particle emission rate exceeds supported limits"
        );
        let instantaneous = emitter
            .get("instantaneous")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        ensure!(
            instantaneous <= 10000,
            "particle burst exceeds supported limits"
        );
        emitters.push(Emitter {
            sphere: kind == "sphererandom",
            directions: vec(emitter, "directions", [1., 1., 0.], warnings)?,
            min,
            max,
            origin,
            rate,
            instantaneous: instantaneous as usize,
            speed: [
                number(emitter, "speedmin", 0., warnings)?,
                number(emitter, "speedmax", 0., warnings)?,
            ],
            sign: vec(emitter, "sign", [0.; 3], warnings)?,
        });
    }
    let mut initializers = Vec::new();
    for item in source
        .get("initializer")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        let init = match name {
            "lifetimerandom" => Initializer::Lifetime(range(item, 0., 1., warnings)?),
            "sizerandom" => Initializer::Size(
                range(item, 0., 20., warnings)?,
                number(item, "exponent", 1., warnings)?.max(0.0001),
            ),
            "alpharandom" => Initializer::Alpha(range(item, 0.05, 1., warnings)?),
            "colorrandom" => Initializer::Color([
                color(item.get("min"), project, warnings)?,
                color(item.get("max"), project, warnings)?,
            ]),
            "rotationrandom" => Initializer::Rotation([
                vec(item, "min", [0.; 3], warnings)?,
                vec(item, "max", [0., 0., std::f32::consts::TAU], warnings)?,
            ]),
            "velocityrandom" => Initializer::Velocity([
                vec(item, "min", [-32.; 3], warnings)?,
                vec(item, "max", [32.; 3], warnings)?,
            ]),
            other => {
                warn(warnings, format!("initializer {other} is not implemented"));
                continue;
            }
        };
        initializers.push(init);
    }
    let mut operators = Vec::new();
    for item in source
        .get("operator")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("name").and_then(Value::as_str).unwrap_or("") {
            "movement" => operators.push(Operator::Movement {
                gravity: vec(item, "gravity", [0.; 3], warnings)?,
                drag: number(item, "drag", 0., warnings)?.max(0.),
            }),
            "alphafade" => operators.push(Operator::AlphaFade {
                fade_in: number(item, "fadeintime", 0.5, warnings)?.clamp(0., 1.),
                fade_out: number(item, "fadeouttime", 0.5, warnings)?.clamp(0., 1.),
            }),
            other => warn(warnings, format!("operator {other} is not implemented")),
        }
    }
    for item in source
        .get("renderer")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("name").and_then(Value::as_str) != Some("sprite") {
            warn(
                warnings,
                "only sprite particle renderers are implemented".into(),
            );
        }
    }
    let overrides = object.get("instanceoverride").unwrap_or(&Value::Null);
    let max_count = source
        .get("maxcount")
        .and_then(Value::as_u64)
        .unwrap_or(100);
    ensure!(
        (1..=10000).contains(&max_count),
        "particle count exceeds supported limits"
    );
    let start_time = number(&source, "starttime", 0., warnings)?;
    ensure!(
        (0. ..=120.).contains(&start_time),
        "particle starttime exceeds supported limits"
    );
    let animation_mode = source
        .get("animationmode")
        .and_then(Value::as_str)
        .unwrap_or("sequence")
        .to_owned();
    if !["sequence", "lifetime"].contains(&animation_mode.as_str()) {
        warn(
            warnings,
            format!("animation mode {animation_mode} is not implemented"),
        );
    }
    Ok(ParticleLayer {
        id,
        order,
        origin: vec(object, "origin", [0.; 3], warnings)?,
        scale: vec(object, "scale", [1.; 3], warnings)?,
        angles: vec(object, "angles", [0.; 3], warnings)?,
        visible: boolean(object.get("visible"), true, project, warnings, "visible")?,
        alpha: number(overrides, "alpha", 1., warnings)?,
        color: vector(
            overrides.get("colorn"),
            [1.; 3],
            project,
            warnings,
            "colorn",
        )?,
        size: number(overrides, "size", 1., warnings)?,
        lifetime: number(overrides, "lifetime", 1., warnings)?,
        rate: number(overrides, "rate", 1., warnings)?.max(0.),
        max_count: max_count as usize,
        start_time,
        texture,
        blending,
        animation_mode,
        sequence_multiplier: number(&source, "sequencemultiplier", 1., warnings)?,
        emitters,
        initializers,
        operators,
    })
}
