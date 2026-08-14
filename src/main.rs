mod engine;
mod game;
mod scene;
mod stl;
mod ui;

use bevy::prelude::*;
use shakmaty::{Color as ChessColor, Position};

use crate::engine::{EngineLink, EngineReply, EngineRequest};
use crate::game::{Game, Phase};

const DEFAULT_SKILL: u32 = 5;
const MAX_SKILL: u32 = 20;

#[derive(Resource, Default)]
struct EngineSync {
    analyse_requested: Option<u32>,
    move_requested: Option<u32>,
}

fn main() {
    let skill = std::env::var("CHESS_SKILL")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SKILL)
        .min(MAX_SKILL);

    let player = match std::env::var("CHESS_SIDE").as_deref() {
        Ok("black") | Ok("b") => ChessColor::Black,
        _ => ChessColor::White,
    };

    let link = engine::start(skill);
    let mut game = Game::new(player, skill);

    if !link.available {
        game.free_play = true;
        game.phase = Phase::PlayerTurn;
    }

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Chess Trainer".to_string(),
                    ..default()
                }),
                ..default()
            }),
            MeshPickingPlugin,
        ))
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
        .insert_resource(game)
        .insert_resource(link)
        .init_resource::<scene::RenderedPly>()
        .init_resource::<EngineSync>()
        .add_systems(Startup, (scene::setup, ui::setup))
        .add_systems(
            Update,
            (
                keyboard,
                drive_engine,
                poll_engine,
                scene::sync_pieces,
                scene::update_highlights,
                scene::orbit_camera,
                ui::update,
            ),
        )
        .run();
}

fn drive_engine(game: Res<Game>, engine: Res<EngineLink>, mut sync: ResMut<EngineSync>) {
    if !engine.available || game.phase == Phase::GameOver {
        return;
    }

    let ply = game.ply;

    if sync.analyse_requested != Some(ply) {
        sync.analyse_requested = Some(ply);
        engine.send(EngineRequest::Analyse {
            fen: game.fen(),
            ply,
        });
    }

    if game.phase == Phase::EngineThinking && sync.move_requested != Some(ply) {
        sync.move_requested = Some(ply);
        engine.send(EngineRequest::PlayMove {
            fen: game.fen(),
            ply,
        });
    }
}

fn poll_engine(mut game: ResMut<Game>, mut engine: ResMut<EngineLink>) {
    while let Some(reply) = engine.try_recv() {
        match reply {
            EngineReply::Analysis {
                ply,
                score,
                best_move,
            } => {
                game.evals.insert(ply, score);
                game.best_moves.insert(ply, best_move.clone());

                if ply == game.ply {
                    game.best_move = Some(best_move);

                    let mover = !game.pos.turn();
                    if ply > 0 && (game.free_play || mover == game.player) {
                        game.grade(ply);
                    }
                }
            }

            EngineReply::OpponentMove { ply, uci } => {
                if ply != game.ply || game.phase != Phase::EngineThinking {
                    continue;
                }
                match game.parse_uci(&uci) {
                    Some(m) => game.apply(m),
                    None => {
                        error!("engine returned unplayable move {uci}");
                        game.phase = Phase::PlayerTurn;
                    }
                }
            }

            EngineReply::Unavailable(msg) => {
                engine.available = false;
                engine.status = msg;
                game.free_play = true;
                game.phase = Phase::PlayerTurn;
            }
        }
    }
}

fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    engine: Res<EngineLink>,
    mut sync: ResMut<EngineSync>,
) {
    if keys.just_pressed(KeyCode::KeyH) {
        game.show_hint = !game.show_hint;
    }

    if keys.just_pressed(KeyCode::KeyU) {
        game.undo();
        *sync = EngineSync::default();
    }

    if keys.just_pressed(KeyCode::KeyN) {
        let (player, skill, free_play) = (game.player, game.skill, game.free_play);
        let mut fresh = Game::new(player, skill);
        fresh.free_play = free_play;
        if free_play {
            fresh.phase = Phase::PlayerTurn;
        }
        *game = fresh;
        *sync = EngineSync::default();
    }

    let mut skill = game.skill;
    if keys.just_pressed(KeyCode::BracketRight) {
        skill = (skill + 1).min(MAX_SKILL);
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        skill = skill.saturating_sub(1);
    }
    if skill != game.skill {
        game.skill = skill;
        engine.send(EngineRequest::SetSkill(skill));
    }
}
