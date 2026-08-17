use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Color as ChessColor, EnPassantMode, Move, Position, Role, Square};

use crate::engine::Score;
use crate::openings::Opening;
use crate::review::{self, MoveReview};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    PlayerTurn,
    EngineThinking,
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Still in known theory. Deliberately not a centipawn verdict: main lines
    /// routinely "lose" a few centipawns to a depth-14 search, and calling the
    /// Najdorf an inaccuracy teaches the opposite of what a trainer should.
    Book,
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
}

impl Quality {
    fn from_loss(loss_cp: i32) -> Self {
        match loss_cp {
            ..=10 => Quality::Best,
            11..=40 => Quality::Good,
            41..=90 => Quality::Inaccuracy,
            91..=200 => Quality::Mistake,
            _ => Quality::Blunder,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Quality::Book => "Theory",
            Quality::Best => "Best move",
            Quality::Good => "Good move",
            Quality::Inaccuracy => "Inaccuracy",
            Quality::Mistake => "Mistake",
            Quality::Blunder => "Blunder",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Quality::Book => Color::srgb(0.48, 0.70, 0.94),
            Quality::Best => Color::srgb(0.36, 0.80, 0.45),
            Quality::Good => Color::srgb(0.55, 0.78, 0.45),
            Quality::Inaccuracy => Color::srgb(0.95, 0.79, 0.30),
            Quality::Mistake => Color::srgb(0.94, 0.55, 0.24),
            Quality::Blunder => Color::srgb(0.90, 0.30, 0.30),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Grade {
    pub quality: Quality,
    pub loss_cp: i32,
    pub played_san: String,
    pub better_san: Option<String>,
}

#[derive(Resource)]
pub struct Game {
    pub pos: Chess,
    pub player: ChessColor,
    pub ply: u32,
    pub phase: Phase,
    pub selected: Option<Square>,
    pub targets: Vec<(Square, Move)>,
    pub evals: HashMap<u32, Score>,
    pub best_moves: HashMap<u32, String>,
    pub best_move: Option<String>,
    pub free_play: bool,
    pub last_grade: Option<Grade>,
    pub show_hint: bool,
    pub history: Vec<String>,
    pub skill: u32,
    pub result_text: Option<String>,
    /// Every graded player move, for the per-stage accuracy breakdown.
    pub reviews: Vec<MoveReview>,
    pub show_summary: bool,
    /// The most specific named opening reached so far.
    pub opening: Option<Opening>,
    /// Plies whose resulting position was still in known theory.
    pub book_plies: HashSet<u32>,
    /// Fullmove number at which the game left theory, once it has.
    pub left_book_at: Option<u32>,
    /// What would have stayed in theory at that point, in SAN.
    pub book_alternatives: Vec<String>,
    snapshots: Vec<Chess>,
}

impl Game {
    pub fn new(player: ChessColor, skill: u32) -> Self {
        let pos = Chess::default();
        let phase = if player == ChessColor::White {
            Phase::PlayerTurn
        } else {
            Phase::EngineThinking
        };

        Self {
            pos,
            player,
            ply: 0,
            phase,
            selected: None,
            targets: Vec::new(),
            evals: HashMap::new(),
            best_moves: HashMap::new(),
            best_move: None,
            free_play: false,
            last_grade: None,
            show_hint: false,
            history: Vec::new(),
            skill,
            result_text: None,
            reviews: Vec::new(),
            show_summary: false,
            opening: None,
            book_plies: HashSet::new(),
            left_book_at: None,
            book_alternatives: Vec::new(),
            snapshots: Vec::new(),
        }
    }

    pub fn undo(&mut self) {
        if self.snapshots.is_empty() {
            return;
        }

        while let Some(prev) = self.snapshots.pop() {
            self.pos = prev;
            self.history.pop();
            self.ply = self.ply.saturating_sub(1);
            if self.pos.turn() == self.player {
                break;
            }
        }

        self.clear_selection();
        self.last_grade = None;
        self.best_move = None;
        self.show_hint = false;
        self.result_text = None;
        // Moves that no longer happened should not still be counted against you.
        self.reviews.retain(|r| r.ply <= self.ply);
        self.book_plies.retain(|&p| p <= self.ply);
        // Rewinding past the point theory ended puts you back in it.
        self.left_book_at = None;
        self.book_alternatives.clear();
        self.phase = if self.pos.turn() == self.player {
            Phase::PlayerTurn
        } else {
            Phase::EngineThinking
        };
    }

    /// The position before the most recent move.
    pub fn previous_position(&self) -> Option<&Chess> {
        self.snapshots.last()
    }

    pub fn fen(&self) -> String {
        Fen::from_position(&self.pos, EnPassantMode::Legal).to_string()
    }

    pub fn movable_color(&self) -> ChessColor {
        if self.free_play {
            self.pos.turn()
        } else {
            self.player
        }
    }

    pub fn is_player_turn(&self) -> bool {
        if self.phase == Phase::GameOver {
            return false;
        }
        self.free_play || (self.pos.turn() == self.player && self.phase == Phase::PlayerTurn)
    }

    pub fn select(&mut self, sq: Square) {
        let Some(piece) = self.pos.board().piece_at(sq) else {
            self.clear_selection();
            return;
        };
        if piece.color != self.movable_color() {
            self.clear_selection();
            return;
        }

        self.targets = self
            .pos
            .legal_moves()
            .into_iter()
            .filter(|m| m.from() == Some(sq))
            .map(|m| (m.to(), m))
            .collect();
        self.selected = Some(sq);
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.targets.clear();
    }

    pub fn move_to(&self, to: Square) -> Option<Move> {
        let mut candidates = self.targets.iter().filter(|(sq, _)| *sq == to);
        let first = candidates.next()?;
        if first.1.promotion().is_some() {
            return self
                .targets
                .iter()
                .find(|(sq, m)| *sq == to && m.promotion() == Some(Role::Queen))
                .map(|(_, m)| *m)
                .or(Some(first.1));
        }
        Some(first.1)
    }

    pub fn apply(&mut self, m: Move) {
        let san = SanPlus::from_move(self.pos.clone(), m).to_string();
        let number = if self.pos.turn() == ChessColor::White {
            format!("{}. ", self.pos.fullmoves())
        } else {
            String::new()
        };
        self.history.push(format!("{number}{san}"));
        self.snapshots.push(self.pos.clone());

        match self.pos.clone().play(m) {
            Ok(next) => self.pos = next,
            Err(e) => {
                error!("rejected a move that was reported legal: {e}");
                return;
            }
        }

        self.ply += 1;
        self.clear_selection();
        self.show_hint = false;
        self.best_move = None;

        if self.pos.is_game_over() {
            self.phase = Phase::GameOver;
            self.result_text = Some(self.describe_outcome());
            self.show_summary = true;
        } else if self.free_play || self.pos.turn() == self.player {
            self.phase = Phase::PlayerTurn;
        } else {
            self.phase = Phase::EngineThinking;
        }
    }

    fn describe_outcome(&self) -> String {
        if self.pos.is_checkmate() {
            let winner = !self.pos.turn();
            let who = if winner == self.player {
                "You win"
            } else {
                "Stockfish wins"
            };
            format!("Checkmate - {who}")
        } else if self.pos.is_stalemate() {
            "Draw by stalemate".to_string()
        } else {
            "Draw".to_string()
        }
    }

    pub fn parse_uci(&self, uci: &str) -> Option<Move> {
        let parsed: UciMove = uci.parse().ok()?;
        parsed.to_move(&self.pos).ok()
    }

    pub fn grade(&mut self, ply: u32) {
        if ply == 0 || self.history.is_empty() {
            return;
        }

        let (Some(&before), Some(&after)) = (self.evals.get(&(ply - 1)), self.evals.get(&ply))
        else {
            return;
        };

        let delta = after.to_cp() - before.to_cp();
        let mover = !self.pos.turn();
        let loss = if mover == ChessColor::White {
            -delta
        } else {
            delta
        };
        let loss = loss.max(0);

        let played_san = self.history.last().cloned().unwrap_or_default();

        let best_uci = self.best_moves.get(&(ply - 1)).cloned();
        let better_san = best_uci
            .and_then(|uci| {
                let prev = self.snapshots.last()?.clone();
                let parsed: UciMove = uci.parse().ok()?;
                let m = parsed.to_move(&prev).ok()?;
                Some(SanPlus::from_move(prev, m).to_string())
            })
            .filter(|best| !played_san.ends_with(best.as_str()));

        // A theory move is reported as theory, whatever the search thinks of it.
        // `book_plies` is filled by `track_opening`, which is ordered ahead of
        // the system that calls this, so the entry is present by now.
        let in_book = self.book_plies.contains(&ply);
        let quality = if in_book {
            Quality::Book
        } else {
            Quality::from_loss(loss)
        };

        // Theory is not scored, so it neither flatters nor dents your accuracy.
        let accuracy = (!in_book).then(|| {
            review::accuracy_for_mover(mover == ChessColor::White, before.to_cp(), after.to_cp())
        });
        self.record_review(ply, quality, loss, accuracy, &played_san);

        // Naming "the engine preferred Nc3" against a main line is the noise
        // this whole feature exists to remove.
        let better_san = if in_book { None } else { better_san };

        self.last_grade = Some(Grade {
            quality,
            loss_cp: loss,
            played_san,
            better_san,
        });
    }

    /// File the graded move under the stage it was *chosen in*, so that a
    /// blunder on the move the queens come off is charged to the middlegame
    /// rather than to the endgame it created.
    fn record_review(
        &mut self,
        ply: u32,
        quality: Quality,
        loss_cp: i32,
        accuracy: Option<f32>,
        san: &str,
    ) {
        let Some(pre_move) = self.snapshots.last() else {
            return;
        };
        let stage = review::classify(pre_move);

        // A takeback replays plies that were already graded; keep one entry each.
        self.reviews.retain(|r| r.ply != ply);
        self.reviews.push(MoveReview {
            ply,
            stage,
            quality,
            loss_cp,
            accuracy,
            san: san.to_string(),
        });
        self.reviews.sort_by_key(|r| r.ply);
    }

    pub fn stage_summary(&self) -> Vec<review::StageStats> {
        review::summarise(&self.reviews)
    }

    pub fn overall_accuracy(&self) -> Option<f32> {
        review::overall_accuracy(&self.reviews)
    }
}

pub fn square_translation(sq: Square) -> Vec3 {
    Vec3::new(
        sq.file().to_u32() as f32 - 3.5,
        0.0,
        3.5 - sq.rank().to_u32() as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Score;
    use shakmaty::Square;

    fn white_game() -> Game {
        Game::new(ChessColor::White, 5)
    }

    #[test]
    fn opening_position_is_white_to_move_with_twenty_moves() {
        let game = white_game();
        assert_eq!(game.pos.turn(), ChessColor::White);
        assert_eq!(game.pos.legal_moves().len(), 20);
        assert!(game.is_player_turn());
    }

    #[test]
    fn selecting_a_pawn_offers_its_two_advances() {
        let mut game = white_game();
        game.select(Square::E2);
        assert_eq!(game.selected, Some(Square::E2));
        let mut targets: Vec<Square> = game.targets.iter().map(|(s, _)| *s).collect();
        targets.sort_by_key(|s| s.to_u32());
        assert_eq!(targets, vec![Square::E3, Square::E4]);
    }

    #[test]
    fn cannot_select_an_opponent_piece() {
        let mut game = white_game();
        game.select(Square::E7);
        assert_eq!(game.selected, None);
        assert!(game.targets.is_empty());
    }

    #[test]
    fn playing_a_move_records_san_and_hands_over_to_the_engine() {
        let mut game = white_game();
        game.select(Square::E2);
        let m = game.move_to(Square::E4).expect("e4 should be legal");
        game.apply(m);

        assert_eq!(game.ply, 1);
        assert_eq!(game.history, vec!["1. e4"]);
        assert_eq!(game.pos.turn(), ChessColor::Black);
        assert_eq!(game.phase, Phase::EngineThinking);
        assert_eq!(game.selected, None);
    }

    #[test]
    fn takeback_rewinds_to_the_players_previous_turn() {
        let mut game = white_game();
        let start_fen = game.fen();

        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());
        // Engine replies.
        game.apply(game.parse_uci("e7e5").unwrap());
        assert_eq!(game.ply, 2);

        game.undo();

        assert_eq!(game.ply, 0);
        assert_eq!(game.fen(), start_fen);
        assert!(game.history.is_empty());
        assert_eq!(game.phase, Phase::PlayerTurn);
    }

    #[test]
    fn engine_uci_moves_are_understood() {
        let game = white_game();
        assert!(game.parse_uci("g1f3").is_some());
        // Not legal in the starting position.
        assert!(game.parse_uci("e2e5").is_none());
        assert!(game.parse_uci("garbage").is_none());
    }

    #[test]
    fn promotion_defaults_to_a_queen() {
        // White pawn on a7, both kings placed legally.
        let fen: Fen = "4k3/P7/8/8/8/8/8/4K3 w - - 0 1".parse().unwrap();
        let pos: Chess = fen.into_position(shakmaty::CastlingMode::Standard).unwrap();

        let mut game = Game::new(ChessColor::White, 5);
        game.pos = pos;
        game.select(Square::A7);

        let m = game.move_to(Square::A8).expect("promotion should be legal");
        assert_eq!(m.promotion(), Some(Role::Queen));
    }

    #[test]
    fn a_move_that_loses_two_pawns_is_graded_a_mistake() {
        let mut game = white_game();
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());

        // White stood +0.20 before the move and -1.80 after it.
        game.evals.insert(0, Score::Centipawns(20));
        game.evals.insert(1, Score::Centipawns(-180));
        game.grade(1);

        let grade = game.last_grade.expect("should have graded the move");
        assert_eq!(grade.quality, Quality::Mistake);
        assert_eq!(grade.loss_cp, 200);
        assert_eq!(grade.played_san, "1. e4");
    }

    #[test]
    fn grading_is_signed_correctly_when_the_player_is_black() {
        let mut game = Game::new(ChessColor::Black, 5);
        // White opens, then Black replies.
        game.apply(game.parse_uci("e2e4").unwrap());
        game.apply(game.parse_uci("e7e5").unwrap());

        // From Black's side, White's score going *up* is Black losing ground.
        game.evals.insert(1, Score::Centipawns(30));
        game.evals.insert(2, Score::Centipawns(330));
        game.grade(2);

        let grade = game.last_grade.expect("should have graded Black's move");
        assert_eq!(grade.loss_cp, 300);
        assert_eq!(grade.quality, Quality::Blunder);
    }

    #[test]
    fn an_engine_best_move_is_reported_when_it_differs() {
        let mut game = white_game();
        // The analyst preferred d4 in the starting position.
        game.best_moves.insert(0, "d2d4".to_string());

        game.select(Square::A2);
        game.apply(game.move_to(Square::A3).unwrap());

        game.evals.insert(0, Score::Centipawns(20));
        game.evals.insert(1, Score::Centipawns(-40));
        game.grade(1);

        let grade = game.last_grade.unwrap();
        assert_eq!(grade.better_san.as_deref(), Some("d4"));
    }

    #[test]
    fn a_graded_move_is_filed_under_the_stage_it_was_played_in() {
        use crate::review::Stage;

        let mut game = white_game();
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());

        game.evals.insert(0, Score::Centipawns(20));
        game.evals.insert(1, Score::Centipawns(10));
        game.grade(1);

        assert_eq!(game.reviews.len(), 1);
        let review = &game.reviews[0];
        assert_eq!(review.ply, 1);
        assert_eq!(review.stage, Stage::Opening);
        assert_eq!(review.quality, Quality::Best);
        // Barely anything given away, so accuracy should be near perfect.
        let accuracy = review.accuracy.expect("a scored move should have one");
        assert!(accuracy > 95.0, "{accuracy}");
    }

    #[test]
    fn a_blunder_by_black_is_scored_from_blacks_side() {
        let mut game = Game::new(ChessColor::Black, 5);
        game.apply(game.parse_uci("e2e4").unwrap());
        game.apply(game.parse_uci("e7e5").unwrap());

        // White's score rising is Black losing ground.
        game.evals.insert(1, Score::Centipawns(0));
        game.evals.insert(2, Score::Centipawns(500));
        game.grade(2);

        let review = &game.reviews[0];
        assert_eq!(review.quality, Quality::Blunder);
        let accuracy = review.accuracy.expect("a scored move should have one");
        assert!(accuracy < 40.0, "{accuracy}");
    }

    #[test]
    fn a_theory_move_is_reported_as_theory_and_left_unscored() {
        let mut game = white_game();
        game.best_moves.insert(0, "d2d4".to_string());
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());

        // The engine mildly disapproves, but the move is known theory.
        game.book_plies.insert(1);
        game.evals.insert(0, Score::Centipawns(30));
        game.evals.insert(1, Score::Centipawns(-20));
        game.grade(1);

        let grade = game.last_grade.as_ref().unwrap();
        assert_eq!(grade.quality, Quality::Book);
        assert_eq!(grade.quality.label(), "Theory");
        // Naming a "better" move against a main line is the noise being removed.
        assert_eq!(grade.better_san, None);

        let review = &game.reviews[0];
        assert_eq!(review.accuracy, None);
        assert_eq!(game.overall_accuracy(), None);
    }

    #[test]
    fn leaving_theory_makes_moves_scored_again() {
        let mut game = white_game();
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());
        game.book_plies.insert(1);
        game.evals.insert(0, Score::Centipawns(30));
        game.evals.insert(1, Score::Centipawns(20));
        game.grade(1);

        game.apply(game.parse_uci("e7e5").unwrap());
        game.select(Square::A2);
        game.apply(game.move_to(Square::A4).unwrap());
        // Ply 3 is deliberately not in `book_plies`.
        game.evals.insert(2, Score::Centipawns(20));
        game.evals.insert(3, Score::Centipawns(-140));
        game.grade(3);

        assert_eq!(game.reviews.len(), 2);
        assert_eq!(game.reviews[0].quality, Quality::Book);
        assert_eq!(game.reviews[1].quality, Quality::Mistake);
        // Only the scored move contributes.
        assert!(game.overall_accuracy().is_some());

        let opening = &game.stage_summary()[0];
        assert_eq!(opening.book_moves, 1);
        assert_eq!(opening.moves, 1);
    }

    #[test]
    fn a_takeback_puts_you_back_inside_theory() {
        let mut game = white_game();
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());
        game.book_plies.insert(1);
        game.apply(game.parse_uci("e7e5").unwrap());

        game.left_book_at = Some(2);
        game.book_alternatives = vec!["Nf3".to_string()];
        game.undo();

        assert_eq!(game.left_book_at, None);
        assert!(game.book_alternatives.is_empty());
        assert!(game.book_plies.is_empty(), "{:?}", game.book_plies);
    }

    #[test]
    fn regrading_a_ply_replaces_rather_than_duplicates_its_review() {
        let mut game = white_game();
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());

        game.evals.insert(0, Score::Centipawns(20));
        game.evals.insert(1, Score::Centipawns(10));
        game.grade(1);
        game.grade(1);

        assert_eq!(game.reviews.len(), 1);
    }

    #[test]
    fn a_takeback_discards_the_review_of_the_move_it_undoes() {
        let mut game = white_game();
        game.select(Square::E2);
        game.apply(game.move_to(Square::E4).unwrap());

        game.evals.insert(0, Score::Centipawns(20));
        game.evals.insert(1, Score::Centipawns(-400));
        game.grade(1);
        assert_eq!(game.reviews.len(), 1);

        game.apply(game.parse_uci("e7e5").unwrap());
        game.undo();

        assert!(game.reviews.is_empty());
        assert!(game.stage_summary().is_empty());
        assert!(game.overall_accuracy().is_none());
    }

    #[test]
    fn the_summary_reaches_a_game_that_ended() {
        let mut game = white_game();
        for uci in ["f2f3", "e7e5", "g2g4", "d8h4"] {
            game.apply(game.parse_uci(uci).unwrap());
        }
        assert!(game.show_summary, "the breakdown should open at game over");
    }

    #[test]
    fn checkmate_ends_the_game() {
        let mut game = white_game();
        for uci in ["f2f3", "e7e5", "g2g4", "d8h4"] {
            game.apply(game.parse_uci(uci).unwrap());
        }
        assert_eq!(game.phase, Phase::GameOver);
        assert!(game.pos.is_checkmate());
        assert!(game.result_text.as_deref().unwrap().contains("Checkmate"));
    }

    #[test]
    fn board_geometry_maps_squares_to_distinct_centres() {
        // a1 is at the near-left from White's view, h8 at the far right.
        assert_eq!(square_translation(Square::A1), Vec3::new(-3.5, 0.0, 3.5));
        assert_eq!(square_translation(Square::H8), Vec3::new(3.5, 0.0, -3.5));
        assert_eq!(square_translation(Square::E1).x, 0.5);
    }
}
