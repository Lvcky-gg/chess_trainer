//! Opening drill: the trainer plays a chosen repertoire and you have to find
//! the book move.
//!
//! Naming theory as it goes by (see `openings`) teaches recognition, which is
//! not the same as knowing a line. A drill inverts the exchange — the position
//! is given and the *move* is the answer — and that is the only part of this
//! app where the player is asked a question rather than graded on an answer
//! they had already chosen.
//!
//! Correctness is judged by comparing the resulting *position* against the one
//! the book move reaches, not by comparing move text. SAN comparison would
//! have to deal with move-number prefixes and with two different notations for
//! the same move; position keys make the whole question go away.

use bevy::prelude::*;
use shakmaty::Position;

use crate::game::{Game, Phase};
use crate::openings::{Book, Repertoire};

/// The move being asked for, and the position it leads to.
#[derive(Debug, Clone)]
struct Question {
    san: String,
    answer: u64,
    /// Cleared by a wrong guess, so a retry cannot earn the point.
    first_try: bool,
}

/// Present only when drilling; its absence is what puts the app in normal play.
#[derive(Resource)]
pub struct Drill {
    repertoire: Repertoire,
    question: Option<Question>,
    pub asked: u32,
    pub correct: u32,
    /// Book moves that were missed on the first attempt.
    pub missed: Vec<String>,
    pub feedback: Option<String>,
    /// Whether the last answer was right, for colouring the feedback.
    pub last_ok: Option<bool>,
    /// The repertoire ran out of moves — the line has been played to its end.
    pub complete: bool,
}

impl Drill {
    pub fn new(repertoire: Repertoire) -> Drill {
        Drill {
            repertoire,
            question: None,
            asked: 0,
            correct: 0,
            missed: Vec::new(),
            feedback: None,
            last_ok: None,
            complete: false,
        }
    }

    /// Read `CHESS_DRILL`, resolve it against the book, and report a filter that
    /// matched nothing rather than quietly drilling the entire database.
    pub fn from_env(book: &Book) -> Option<Drill> {
        let filter = std::env::var("CHESS_DRILL").ok()?;
        if filter.trim().is_empty() {
            return None;
        }

        match book.repertoire(&filter) {
            Some(repertoire) => {
                info!(
                    "drilling {} ({} lines)",
                    repertoire.label,
                    repertoire.lines()
                );
                Some(Drill::new(repertoire))
            }
            None => {
                warn!("no opening matches CHESS_DRILL={filter:?} - playing normally");
                None
            }
        }
    }

    pub fn label(&self) -> String {
        format!("Drill: {}", self.repertoire.label)
    }

    pub fn score(&self) -> String {
        format!("{}/{} first try", self.correct, self.asked)
    }

    /// Restart the drill without losing the chosen repertoire.
    pub fn reset(&mut self) {
        let repertoire = self.repertoire.clone();
        *self = Drill::new(repertoire);
    }
}

/// One step of the drill loop: mark the answer, reply from the book, then pose
/// the next question. All three happen in the same frame, so a correct answer
/// is met with the opponent's reply immediately.
pub fn step(mut game: ResMut<Game>, book: Res<Book>, mut drill: ResMut<Drill>) {
    if book.is_empty() || drill.complete {
        return;
    }

    // A question is outstanding and it is no longer the player's turn, so the
    // move just played is their answer.
    if let Some(question) = drill.question.take() {
        if game.pos.turn() == game.player {
            // Still their turn: the question stands.
            drill.question = Some(question);
        } else if book.position_key(&game.pos) == question.answer {
            if question.first_try {
                drill.correct += 1;
            }
            drill.feedback = Some(format!("Correct - {}", question.san));
            drill.last_ok = Some(true);
        } else {
            let played = game.history.last().cloned().unwrap_or_default();
            drill.feedback = Some(format!("{played}?  the book move is {}", question.san));
            drill.last_ok = Some(false);
            if question.first_try {
                drill.missed.push(question.san.clone());
            }

            game.undo();
            drill.question = Some(Question {
                first_try: false,
                ..question
            });
            return;
        }
    }

    if game.phase == Phase::GameOver {
        drill.complete = true;
        return;
    }

    // The opponent answers from the same repertoire, so the line stays on rails.
    if game.pos.turn() != game.player {
        match book.main_line(&game.pos, Some(&drill.repertoire)) {
            Some((_, reply)) => game.apply(reply),
            None => {
                finish(&mut drill);
                return;
            }
        }
    }

    if drill.question.is_none() {
        let asked = book
            .main_line(&game.pos, Some(&drill.repertoire))
            .and_then(|(san, m)| {
                let next = game.pos.clone().play(m).ok()?;
                Some(Question {
                    san,
                    answer: book.position_key(&next),
                    first_try: true,
                })
            });

        match asked {
            Some(question) => {
                drill.asked += 1;
                drill.question = Some(question);
            }
            None => finish(&mut drill),
        }
    }
}

fn finish(drill: &mut Drill) {
    drill.complete = true;
    drill.feedback = Some(format!("Line complete - {}", drill.score()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::Color as ChessColor;

    fn drill_app(filter: &str, player: ChessColor) -> App {
        let book = Book::load();
        let repertoire = book
            .repertoire(filter)
            .expect("the test filter should match lines");

        let mut app = App::new();
        app.insert_resource(Game::new(player, 5))
            .insert_resource(book)
            .insert_resource(Drill::new(repertoire))
            .add_systems(Update, step);
        app.update();
        app
    }

    fn play(app: &mut App, uci: &str) {
        let mut game = app.world_mut().resource_mut::<Game>();
        let m = game
            .parse_uci(uci)
            .unwrap_or_else(|| panic!("{uci} should be legal"));
        game.apply(m);
        app.update();
    }

    fn drill(app: &App) -> &Drill {
        app.world().resource::<Drill>()
    }

    #[test]
    fn the_first_question_is_posed_before_any_move_is_made() {
        let app = drill_app("English Opening", ChessColor::White);

        assert_eq!(drill(&app).asked, 1);
        assert_eq!(drill(&app).correct, 0);
        assert_eq!(
            drill(&app).question.as_ref().map(|q| q.san.as_str()),
            Some("c4")
        );
    }

    #[test]
    fn a_correct_answer_scores_and_draws_a_reply_from_the_book() {
        let mut app = drill_app("English Opening", ChessColor::White);
        play(&mut app, "c2c4");

        assert_eq!(drill(&app).correct, 1);
        assert!(drill(&app).missed.is_empty());
        assert!(
            drill(&app)
                .feedback
                .as_deref()
                .unwrap()
                .starts_with("Correct")
        );

        // The opponent has already replied and the next question is waiting.
        let game = app.world().resource::<Game>();
        assert_eq!(game.ply, 2, "{:?}", game.history);
        assert_eq!(game.pos.turn(), ChessColor::White);
        assert_eq!(drill(&app).asked, 2);
    }

    #[test]
    fn a_wrong_answer_is_taken_back_and_asked_again() {
        let mut app = drill_app("English Opening", ChessColor::White);
        play(&mut app, "e2e4");

        let game = app.world().resource::<Game>();
        assert_eq!(game.ply, 0, "the wrong move should be taken back");
        assert_eq!(game.pos.turn(), ChessColor::White);

        let drill = drill(&app);
        assert_eq!(drill.asked, 1, "a retry is not a new question");
        assert_eq!(drill.correct, 0);
        assert_eq!(drill.missed, vec!["c4".to_string()]);
        assert!(drill.feedback.as_deref().unwrap().contains("c4"));
    }

    #[test]
    fn a_retry_does_not_earn_the_point() {
        let mut app = drill_app("English Opening", ChessColor::White);
        play(&mut app, "e2e4");
        play(&mut app, "c2c4");

        let drill = drill(&app);
        assert_eq!(drill.correct, 0, "the point was lost on the first guess");
        assert_eq!(drill.missed.len(), 1);
        // The game still moves on once the right move is found.
        assert_eq!(app.world().resource::<Game>().ply, 2);
    }

    #[test]
    fn the_drill_plays_the_players_side_only() {
        // Drilling a defence, the book opens for White and asks Black to reply.
        let app = drill_app("Dutch Defense", ChessColor::Black);

        let game = app.world().resource::<Game>();
        assert_eq!(game.ply, 1, "White should have opened from the book");
        assert_eq!(game.pos.turn(), ChessColor::Black);
        assert_eq!(
            drill(&app).question.as_ref().map(|q| q.san.as_str()),
            Some("f5")
        );
    }

    #[test]
    fn running_out_of_theory_completes_the_drill() {
        // One short line, drilled to its end.
        let mut app = drill_app("Bongcloud Attack", ChessColor::White);
        for _ in 0..12 {
            if drill(&app).complete {
                break;
            }
            let expected = drill(&app)
                .question
                .as_ref()
                .map(|q| q.san.clone())
                .expect("a question while incomplete");
            let uci = {
                let game = app.world().resource::<Game>();
                let parsed: shakmaty::san::SanPlus =
                    expected.parse().expect("book SAN should parse");
                let m = parsed.san.to_move(&game.pos).expect("book move is legal");
                m.to_uci(shakmaty::CastlingMode::Standard).to_string()
            };
            play(&mut app, &uci);
        }

        let drill = drill(&app);
        assert!(drill.complete, "the line should run out");
        assert!(drill.missed.is_empty(), "{:?}", drill.missed);
        assert!(
            drill.feedback.as_deref().unwrap().contains("Line complete"),
            "{:?}",
            drill.feedback
        );
    }

    #[test]
    fn resetting_keeps_the_repertoire_and_clears_the_score() {
        let mut app = drill_app("English Opening", ChessColor::White);
        play(&mut app, "e2e4");

        let mut drill = app.world_mut().resource_mut::<Drill>();
        drill.reset();

        assert_eq!(drill.asked, 0);
        assert!(drill.missed.is_empty());
        assert!(drill.question.is_none());
        assert_eq!(drill.repertoire.label, "English Opening");
    }

    #[test]
    fn an_empty_book_asks_nothing() {
        let mut app = App::new();
        app.insert_resource(Game::new(ChessColor::White, 5))
            .insert_resource(Book::default())
            .insert_resource(Drill::new(Repertoire::empty("nothing")))
            .add_systems(Update, step);
        app.update();

        assert_eq!(app.world().resource::<Drill>().asked, 0);
    }
}
