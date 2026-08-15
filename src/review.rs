//! Which stage of the game a move belonged to, and how accurately it was played.
//!
//! Centipawn loss on its own says a move cost 0.6 pawns. It never says whether
//! that happened while you were still developing or twenty moves later in a rook
//! ending, so a game full of grades still cannot answer the only question worth
//! asking: *which part of my game is the weak one?* Tagging each move with a
//! stage and averaging accuracy within the stage is what answers it.

use shakmaty::{Chess, Color as ChessColor, Position, Role, Square};

use crate::game::Quality;

/// Non-pawn material (both sides, Q=9 R=5 B=N=3) at or below which the position
/// counts as an endgame. 20 is about a rook and a minor each: queens are off and
/// king activity has started to matter.
const ENDGAME_MATERIAL: u32 = 20;

/// A position is only still an opening while *both* bounds hold. Either one
/// alone misfires: a gambit can leave a piece on its home square at move 30, and
/// a quiet exchange line can be fully developed by move 8.
const OPENING_LAST_FULLMOVE: u32 = 15;
const OPENING_MIN_UNDEVELOPED: u32 = 4;

/// The eight squares a knight or bishop starts on, with the colour that owns
/// them. A black knight parked on b1 is not White failing to develop.
const MINOR_HOME: [(Square, ChessColor); 8] = [
    (Square::B1, ChessColor::White),
    (Square::C1, ChessColor::White),
    (Square::F1, ChessColor::White),
    (Square::G1, ChessColor::White),
    (Square::B8, ChessColor::Black),
    (Square::C8, ChessColor::Black),
    (Square::F8, ChessColor::Black),
    (Square::G8, ChessColor::Black),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Opening,
    Middlegame,
    Endgame,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Stage::Opening, Stage::Middlegame, Stage::Endgame];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Opening => "Opening",
            Stage::Middlegame => "Middlegame",
            Stage::Endgame => "Endgame",
        }
    }
}

/// Classify the position a move was chosen *in* — not the one it led to.
pub fn classify(pos: &Chess) -> Stage {
    let board = pos.board();

    let material = 9 * board.queens().count() as u32
        + 5 * board.rooks().count() as u32
        + 3 * (board.bishops().count() + board.knights().count()) as u32;

    if material <= ENDGAME_MATERIAL {
        return Stage::Endgame;
    }

    let undeveloped = MINOR_HOME
        .iter()
        .filter(|(sq, color)| {
            board.piece_at(*sq).is_some_and(|piece| {
                piece.color == *color && matches!(piece.role, Role::Knight | Role::Bishop)
            })
        })
        .count() as u32;

    if pos.fullmoves().get() <= OPENING_LAST_FULLMOVE && undeveloped >= OPENING_MIN_UNDEVELOPED {
        Stage::Opening
    } else {
        Stage::Middlegame
    }
}

/// Lichess' win-percentage curve. A 100cp edge is worth far more at level
/// material than it is when you are already three pawns up, so accuracy is
/// measured in winning chances rather than in raw centipawns.
pub fn win_percent(cp: i32) -> f32 {
    50.0 + 50.0 * (2.0 / (1.0 + (-0.00368208 * cp as f32).exp()) - 1.0)
}

/// Winning chances given up by one move, mapped onto 0-100. Both constants are
/// Lichess'; the curve is deliberately forgiving near zero, so a move that
/// concedes nothing scores 100 rather than something like 96.
pub fn accuracy(win_before: f32, win_after: f32) -> f32 {
    let dropped = (win_before - win_after).max(0.0);
    (103.1668 * (-0.04354 * dropped).exp() - 3.1669).clamp(0.0, 100.0)
}

/// Accuracy of a move, from evaluations normalised to White. Accuracy is about
/// what the *mover* gave away, so Black's winning chances are the complement.
pub fn accuracy_for_mover(mover_is_white: bool, before_cp: i32, after_cp: i32) -> f32 {
    let (before, after) = (win_percent(before_cp), win_percent(after_cp));
    if mover_is_white {
        accuracy(before, after)
    } else {
        accuracy(100.0 - before, 100.0 - after)
    }
}

/// One graded player move, kept for the end-of-game breakdown.
#[derive(Debug, Clone)]
pub struct MoveReview {
    pub ply: u32,
    pub stage: Stage,
    pub quality: Quality,
    pub loss_cp: i32,
    pub accuracy: f32,
    pub san: String,
}

#[derive(Debug, Clone)]
pub struct StageStats {
    pub stage: Stage,
    pub moves: u32,
    pub accuracy: f32,
    pub inaccuracies: u32,
    pub mistakes: u32,
    pub blunders: u32,
    /// The single move that cost the most, as (SAN, centipawns lost).
    pub worst: Option<(String, i32)>,
}

impl StageStats {
    /// "2 mistakes, 1 blunder", or empty when the stage was played cleanly.
    pub fn problem_summary(&self) -> String {
        let counts = [
            (self.inaccuracies, "inaccuracy", "inaccuracies"),
            (self.mistakes, "mistake", "mistakes"),
            (self.blunders, "blunder", "blunders"),
        ];

        counts
            .iter()
            .filter(|(n, _, _)| *n > 0)
            .map(|(n, one, many)| format!("{n} {}", if *n == 1 { one } else { many }))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Group the reviews by stage, in board order, skipping stages never reached.
pub fn summarise(reviews: &[MoveReview]) -> Vec<StageStats> {
    Stage::ALL
        .iter()
        .filter_map(|&stage| {
            let moves: Vec<&MoveReview> = reviews.iter().filter(|r| r.stage == stage).collect();
            if moves.is_empty() {
                return None;
            }

            let count = |q: Quality| moves.iter().filter(|r| r.quality == q).count() as u32;
            let worst = moves
                .iter()
                .max_by_key(|r| r.loss_cp)
                .filter(|r| r.loss_cp > 0)
                .map(|r| (r.san.clone(), r.loss_cp));

            Some(StageStats {
                stage,
                moves: moves.len() as u32,
                accuracy: moves.iter().map(|r| r.accuracy).sum::<f32>() / moves.len() as f32,
                inaccuracies: count(Quality::Inaccuracy),
                mistakes: count(Quality::Mistake),
                blunders: count(Quality::Blunder),
                worst,
            })
        })
        .collect()
}

pub fn overall_accuracy(reviews: &[MoveReview]) -> Option<f32> {
    if reviews.is_empty() {
        return None;
    }
    Some(reviews.iter().map(|r| r.accuracy).sum::<f32>() / reviews.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::CastlingMode;
    use shakmaty::fen::Fen;

    fn position(fen: &str) -> Chess {
        fen.parse::<Fen>()
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap()
    }

    #[test]
    fn the_starting_position_is_an_opening() {
        assert_eq!(classify(&Chess::default()), Stage::Opening);
    }

    #[test]
    fn a_developed_position_is_a_middlegame_even_with_full_material() {
        // Both sides castled, all minors off their home squares, move 9.
        let pos = position("r2q1rk1/ppp1bppp/2np1n2/4p1B1/2B1P1b1/2NP1N2/PPP2PPP/R2Q1RK1 w - - 0 9");
        assert_eq!(classify(&pos), Stage::Middlegame);
    }

    #[test]
    fn an_undeveloped_position_past_move_fifteen_is_a_middlegame() {
        // Same pieces at home as the start, but 20 moves of pawn shuffling in.
        let pos = position("rnbqkbnr/8/pppppppp/8/8/PPPPPPPP/8/RNBQKBNR w KQkq - 0 20");
        assert_eq!(classify(&pos), Stage::Middlegame);
    }

    #[test]
    fn a_rook_ending_is_an_endgame() {
        let pos = position("8/5pk1/6p1/7p/7P/6P1/5PK1/3R4 w - - 0 40");
        assert_eq!(classify(&pos), Stage::Endgame);
    }

    #[test]
    fn queens_and_rooks_alone_are_not_yet_an_endgame() {
        // 2Q + 2R = 28 points of non-pawn material, above the threshold.
        let pos = position("3qr1k1/5ppp/8/8/8/8/5PPP/3QR1K1 w - - 0 25");
        assert_eq!(classify(&pos), Stage::Middlegame);
    }

    #[test]
    fn the_ruy_lopez_is_still_an_opening() {
        let pos = position("r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3");
        assert_eq!(classify(&pos), Stage::Opening);
    }

    #[test]
    fn a_piece_of_the_wrong_colour_at_home_is_not_undeveloped_material() {
        // Black knights have raided b1/g1; White's own minors are developed.
        let pos = position("r1bqk2r/pppp1ppp/8/2b1p3/2B1P3/5N2/PPPP1PPP/RnBQK1nR w KQkq - 0 8");
        assert_eq!(classify(&pos), Stage::Middlegame);
    }

    #[test]
    fn giving_up_nothing_scores_full_accuracy() {
        let wp = win_percent(30);
        assert!((accuracy(wp, wp) - 100.0).abs() < 0.01);
    }

    #[test]
    fn accuracy_falls_as_winning_chances_are_given_away() {
        let clean = accuracy(win_percent(20), win_percent(10));
        let sloppy = accuracy(win_percent(20), win_percent(-180));
        let awful = accuracy(win_percent(20), win_percent(-900));

        assert!(clean > 95.0, "{clean}");
        assert!((20.0..70.0).contains(&sloppy), "{sloppy}");
        assert!(awful < 20.0, "{awful}");
    }

    #[test]
    fn the_same_centipawn_loss_hurts_less_when_already_winning() {
        // 150cp given away from level, versus from +8. Identical in centipawns,
        // nothing like identical in winning chances.
        let from_level = accuracy(win_percent(0), win_percent(-150));
        let from_won = accuracy(win_percent(800), win_percent(650));
        assert!(from_won > from_level + 20.0, "{from_won} vs {from_level}");
    }

    #[test]
    fn a_mate_score_saturates_rather_than_overflowing() {
        assert!(win_percent(10_000) > 99.9);
        assert!(win_percent(-10_000) < 0.1);
    }

    fn review(ply: u32, stage: Stage, quality: Quality, loss_cp: i32) -> MoveReview {
        MoveReview {
            ply,
            stage,
            quality,
            loss_cp,
            accuracy: 100.0 - loss_cp as f32 / 10.0,
            san: format!("move{ply}"),
        }
    }

    #[test]
    fn stages_that_were_never_reached_are_left_out() {
        let reviews = [review(1, Stage::Opening, Quality::Best, 0)];
        let stats = summarise(&reviews);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].stage, Stage::Opening);
    }

    #[test]
    fn each_stage_averages_only_its_own_moves() {
        let reviews = [
            review(1, Stage::Opening, Quality::Best, 0),
            review(3, Stage::Opening, Quality::Good, 20),
            review(5, Stage::Middlegame, Quality::Blunder, 400),
            review(7, Stage::Middlegame, Quality::Mistake, 120),
            review(9, Stage::Middlegame, Quality::Inaccuracy, 60),
        ];

        let stats = summarise(&reviews);
        assert_eq!(stats.len(), 2);

        assert_eq!(stats[0].stage, Stage::Opening);
        assert_eq!(stats[0].moves, 2);
        assert!((stats[0].accuracy - 99.0).abs() < 0.01);
        assert_eq!(stats[0].problem_summary(), "");

        assert_eq!(stats[1].stage, Stage::Middlegame);
        assert_eq!(stats[1].moves, 3);
        assert_eq!(stats[1].blunders, 1);
        assert_eq!(stats[1].mistakes, 1);
        assert_eq!(stats[1].inaccuracies, 1);
        assert_eq!(
            stats[1].problem_summary(),
            "1 inaccuracy, 1 mistake, 1 blunder"
        );
        assert_eq!(stats[1].worst, Some(("move5".to_string(), 400)));
    }

    #[test]
    fn a_flawless_stage_reports_no_worst_move() {
        let reviews = [review(1, Stage::Opening, Quality::Best, 0)];
        assert_eq!(summarise(&reviews)[0].worst, None);
    }

    #[test]
    fn overall_accuracy_spans_every_stage() {
        let reviews = [
            review(1, Stage::Opening, Quality::Best, 0),
            review(3, Stage::Endgame, Quality::Mistake, 200),
        ];
        let overall = overall_accuracy(&reviews).unwrap();
        assert!((overall - 90.0).abs() < 0.01);
        assert!(overall_accuracy(&[]).is_none());
    }
}
