//! Splash scene shown before the editor: the engine logo slides in under a glitch shader, then a
//! fade-to-black hands over to the `editor` scene. Any action button skips straight to the editor.
//!
//! The logo sprite and the phase-control entity are plain scene entities, so the engine's scene
//! teardown despawns them on exit; only the post-process chain and its uniforms are resources and
//! are reset in [`intro_exit`].

use aberredengine::prelude::{
    Camera2D, Commands, Component, Easing, GameConfig, InputAction, InputState, MainCamera,
    NextScene, On, Phase, PostProcessShader, Query, Res, ResMut, SceneEntered, SceneExited, Sprite,
    Tween, UniformValue, Vec2, With, WorldPosition, ZIndex, bevy_ecs,
};

/// Seconds spent in each of the `start` and `medium` phases before advancing.
const PHASE_DURATION: f32 = 1.5;
/// `uGlitch` units per second while the `medium` phase is active.
const GLITCH_RATE: f32 = 0.1;
/// Fade alpha added per second during the `fadeout` phase.
const FADE_RATE: f32 = 0.7;
/// Fade alpha at which the intro hands over to the editor scene.
const FADE_THRESHOLD: f32 = 1.1;

/// Marks the entity that carries the intro's [`Phase`], so [`intro_shader_system`] can drive the
/// shader uniforms from it.
#[derive(Component)]
pub struct IntroShaderControl;

/// Scene-enter observer for `intro`: spawns the logo off the top edge with a slide-in tween, points
/// the camera at the render-target centre, and enables the glitch + fade post-process chain.
pub fn intro_enter(
    _: On<SceneEntered>,
    mut commands: Commands,
    config: Res<GameConfig>,
    mut post_process: ResMut<PostProcessShader>,
) {
    log::info!("intro: scene entered");
    let start_position = WorldPosition::new(0.0, config.render_height as f32);
    let end_position = WorldPosition::new(0.0, 0.0);
    commands.spawn_empty().insert((
        Sprite::new("aberred_engine_isometric_alpha", 807.0, 970.0)
            .with_origin(Vec2::new(403.0, 242.0)),
        start_position,
        Tween::new(start_position, end_position, 2.0).with_easing(Easing::CubicOut),
        ZIndex(0.0),
    ));
    commands.insert_resource(MainCamera(Camera2D::new(
        Vec2::ZERO,
        Vec2::new(
            config.render_width as f32 / 2.0,
            config.render_height as f32 / 2.0,
        ),
    )));
    post_process.set_shader_chain(Some(vec!["glitch".to_string(), "fade".to_string()]));
    commands.spawn((Phase::new("start"), IntroShaderControl));
}

/// Drives the `start` -> `medium` -> `fadeout` phase machine: raises `uGlitch` during `medium`,
/// ramps `fadeColor` alpha to black during `fadeout`, and requests the `editor` scene once the fade
/// is fully opaque. The engine's phase system applies `phase.next` and advances `time_in_phase`
/// (reset on each transition) before this runs.
pub fn intro_shader_system(
    mut query: Query<&mut Phase, With<IntroShaderControl>>,
    mut post_process: ResMut<PostProcessShader>,
    mut next_scene: ResMut<NextScene>,
) {
    for mut phase in &mut query {
        let t = phase.time_in_phase;
        match phase.current.as_str() {
            "start" | "medium" => {
                let medium = phase.current == "medium";
                let uglitch = if medium { t * GLITCH_RATE } else { 0.0 };
                post_process.set_uniform("uGlitch", UniformValue::Float(uglitch));
                if t > PHASE_DURATION {
                    phase.next = Some(if medium { "fadeout" } else { "medium" }.into());
                }
            }
            "fadeout" => {
                let fade = t * FADE_RATE;
                post_process.set_uniform(
                    "fadeColor",
                    UniformValue::Vec4 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: fade,
                    },
                );
                if fade > FADE_THRESHOLD {
                    next_scene.set("editor");
                }
            }
            _ => {}
        }
    }
}

/// Skips the intro on any action button by requesting the `editor` scene directly.
pub fn intro_update(input: Res<InputState>, mut next_scene: ResMut<NextScene>) {
    if [
        InputAction::Action1,
        InputAction::Action2,
        InputAction::Action3,
        InputAction::Back,
    ]
    .into_iter()
    .any(|a| input.action(a).active)
    {
        log::info!("intro: skipped by action button");
        next_scene.set("editor");
    }
}

/// Scene-exit observer for `intro`: disables the post-process chain and clears its uniforms so the
/// editor starts with a clean default pipeline.
pub fn intro_exit(_: On<SceneExited>, mut post_process: ResMut<PostProcessShader>) {
    log::info!("intro: scene exited");
    post_process.set_shader_chain(None);
    post_process.clear_uniforms();
}
