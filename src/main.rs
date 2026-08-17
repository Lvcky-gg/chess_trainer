mod drill;
mod engine;
mod game;
mod openings;
mod review;
mod scene;
mod stl;
mod ui;

use bevy::prelude::*;
use shakmaty::{Color as ChessColor, Position};

use crate::drill::Drill;
use crate::engine::{EngineLink, EngineReply, EngineRequest};
use crate::game::{Game, Phase};
use crate::openings::Book;

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

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Chess Trainer".to_string(),
                ..default()
            }),
            ..default()
        }),
        MeshPickingPlugin,
    ));

    // Everything below reports through `info!`/`warn!`, so it has to come after
    // the plugins install the log subscriber — a mistyped CHESS_DRILL is only
    // useful if the user is told about it.
    let book = openings::Book::load();
    let drill = Drill::from_env(&book);
    let link = engine::start(skill);
    let mut game = Game::new(player, skill);

    // A drill needs no engine at all — the book poses the questions and plays
    // the replies — so it must not fall back to moving both sides by hand.
    if !link.available && drill.is_none() {
        game.free_play = true;
        game.phase = Phase::PlayerTurn;
    }

    app.insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
        .insert_resource(game)
        .insert_resource(link)
        .insert_resource(book)
        .init_resource::<scene::RenderedPly>()
        .init_resource::<EngineSync>()
        .add_systems(Startup, (scene::setup, ui::setup))
        .add_systems(
            Update,
            (
                keyboard,
                drive_engine,
                (
                    drill::step.run_if(resource_exists::<Drill>),
                    track_opening,
                    poll_engine,
                )
                    .chain(),
                scene::sync_pieces,
                scene::update_highlights,
                scene::orbit_camera,
                ui::update,
            ),
        );

    // The resource's presence is what puts the app in drill mode.
    if let Some(drill) = drill {
        app.insert_resource(drill);
    }

    app.run();
}

/// Follow the game through the opening book: name the line while it lasts, and
/// record where it ended. Runs ahead of `poll_engine` so that a move's book
/// status is already known by the time the analyst's grade for it arrives.
fn track_opening(mut game: ResMut<Game>, book: Res<Book>, mut last_seen: Local<u64>) {
    if book.is_empty() {
        return;
    }

    // Touching `game` at all marks it changed and redraws the UI, so do nothing
    // until the position actually moves.
    let key = book.position_key(&game.pos);
    if *last_seen == key {
        return;
    }
    *last_seen = key;

    if book.contains(&game.pos) {
        let ply = game.ply;
        game.book_plies.insert(ply);
        if let Some(opening) = book.name(&game.pos).cloned() {
            game.opening = Some(opening);
        }
        return;
    }

    // First position outside theory: say what would have stayed in it.
    if game.left_book_at.is_none() {
        game.left_book_at = Some(game.pos.fullmoves().get());
        game.book_alternatives = game
            .previous_position()
            .map(|prev| book.continuations(prev))
            .unwrap_or_default();
    }
}

fn drive_engine(
    game: Res<Game>,
    engine: Res<EngineLink>,
    mut sync: ResMut<EngineSync>,
    drill: Option<Res<Drill>>,
) {
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

    // While drilling, the opponent's moves come from the book, not the engine.
    // Analysis stays on, so the eval bar still shows where a line leads.
    if drill.is_some() {
        return;
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
    mut drill: Option<ResMut<Drill>>,
) {
    if keys.just_pressed(KeyCode::KeyH) {
        game.show_hint = !game.show_hint;
    }

    if keys.just_pressed(KeyCode::KeyS) {
        game.show_summary = !game.show_summary;
    }

    // A takeback would rewind the board out from under the pending question,
    // whose answer belongs to the position being left. The drill does its own
    // retries, so it owns the board.
    if keys.just_pressed(KeyCode::KeyU) && drill.is_none() {
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
        if let Some(drill) = drill.as_mut() {
            drill.reset();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn played(ucis: &[&str]) -> Game {
        let mut game = Game::new(ChessColor::White, 5);
        for uci in ucis {
            let m = game.parse_uci(uci).expect("test move should be legal");
            game.apply(m);
        }
        game
    }

    /// Drives the real system against the real book, with no rendering. Moves
    /// are stepped one at a time because that is how the app runs it: the
    /// opening name comes from the named positions passed *through*, not from
    /// the final one, which is usually not itself the end of a database line.
    fn tracked(ucis: &[&str]) -> App {
        let mut app = App::new();
        app.insert_resource(Game::new(ChessColor::White, 5))
            .insert_resource(Book::load())
            .add_systems(Update, track_opening);
        app.update();

        for uci in ucis {
            let mut game = app.world_mut().resource_mut::<Game>();
            let m = game.parse_uci(uci).expect("test move should be legal");
            game.apply(m);
            app.update();
        }
        app
    }

    #[test]
    fn a_takeback_is_refused_while_drilling() {
        let book = Book::load();
        let repertoire = book.repertoire("English Opening").unwrap();

        let mut app = App::new();
        app.insert_resource(played(&["e2e4", "e7e5"]))
            .insert_resource(book)
            .insert_resource(Drill::new(repertoire))
            .init_resource::<EngineSync>()
            .insert_resource(engine::start_unavailable())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(Update, keyboard);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyU);
        app.update();

        assert_eq!(
            app.world().resource::<Game>().ply,
            2,
            "the drill owns the board"
        );
    }

    #[test]
    fn a_main_line_is_named_as_it_is_played() {
        let app = tracked(&["e2e4", "c7c5"]);
        let game = app.world().resource::<Game>();

        let opening = game.opening.as_ref().expect("1.e4 c5 should be named");
        assert!(opening.name.contains("Sicilian"), "{}", opening.name);
        assert!(game.book_plies.contains(&2));
        assert_eq!(game.left_book_at, None);
        assert!(game.book_alternatives.is_empty());
    }

    #[test]
    fn leaving_theory_records_the_move_and_the_alternatives() {
        // A Ruy Lopez, then 3...Nh6, which no database line contains.
        let app = tracked(&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "g8h6"]);
        let game = app.world().resource::<Game>();

        assert_eq!(game.left_book_at, Some(4));
        assert!(!game.book_plies.contains(&6));
        assert!(
            !game.book_alternatives.is_empty(),
            "3...a5 leaves a position with known replies"
        );
        // The last name reached still stands.
        let opening = game
            .opening
            .as_ref()
            .expect("the Ruy Lopez should still be named");
        assert!(opening.name.contains("Ruy Lopez"), "{}", opening.name);
    }

    #[test]
    fn a_transposition_is_still_recognised_as_theory() {
        // The same position by two move orders. Neither order ends on a named
        // line, so what is asserted is that both stay *in* theory — the names
        // differ legitimately, because they describe the route taken.
        let direct_app = tracked(&["d2d4", "g8f6", "c2c4", "e7e6", "b1c3"]);
        let transposed_app = tracked(&["c2c4", "g8f6", "b1c3", "e7e6", "d2d4"]);
        let direct = direct_app.world().resource::<Game>();
        let transposed = transposed_app.world().resource::<Game>();

        assert_eq!(direct.left_book_at, None);
        assert_eq!(transposed.left_book_at, None);
        assert!(direct.book_plies.contains(&5));
        assert!(transposed.book_plies.contains(&5));
        assert!(direct.opening.is_some());
        assert!(transposed.opening.is_some());
    }

    #[test]
    fn an_empty_book_leaves_the_game_untouched() {
        let mut app = App::new();
        app.insert_resource(played(&["e2e4", "c7c5"]))
            .insert_resource(Book::default())
            .add_systems(Update, track_opening);
        app.update();

        let game = app.world().resource::<Game>();
        assert!(game.opening.is_none());
        assert!(game.book_plies.is_empty());
    }
}
