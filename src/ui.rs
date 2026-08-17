use bevy::prelude::*;
use bevy::text::FontSize;
use shakmaty::{Color as ChessColor, Position};

use crate::drill::Drill;
use crate::engine::{EngineLink, Score};
use crate::game::{Game, Phase, Quality};
use crate::openings::Book;

#[derive(Component)]
pub struct StatusText;
#[derive(Component)]
pub struct FeedbackText;
#[derive(Component)]
pub struct MoveListText;
#[derive(Component)]
pub struct EvalLabel;

#[derive(Component)]
pub struct EvalFill;

#[derive(Component)]
pub struct SummaryPanel;
#[derive(Component)]
pub struct SummaryText;
#[derive(Component)]
pub struct OpeningText;

const BOOK_BLUE: Color = Color::srgb(0.48, 0.70, 0.94);

const PANEL_BG: Color = Color::srgba(0.07, 0.08, 0.10, 0.86);
const TEXT_DIM: Color = Color::srgb(0.68, 0.71, 0.76);

fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

pub fn setup(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            width: px(320),
            padding: UiRect::all(px(14)),
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        children![
            (
                Text::new("Chess Trainer"),
                font(22.0),
                TextColor(Color::srgb(0.95, 0.95, 0.97)),
            ),
            (
                Text::new("Starting..."),
                font(15.0),
                TextColor(TEXT_DIM),
                StatusText,
            ),
            (
                Text::new(""),
                font(14.0),
                TextColor(BOOK_BLUE),
                OpeningText,
            ),
            (
                Text::new(""),
                font(17.0),
                TextColor(Color::WHITE),
                FeedbackText,
            ),
            (
                Text::new(
                    "Click a piece, then its destination\n\
                     H  hint      U  take back\n\
                     N  new game  S  accuracy\n\
                     [ ]  engine strength\n\
                     Right-drag orbit - scroll zoom"
                ),
                font(13.0),
                TextColor(Color::srgb(0.52, 0.55, 0.60)),
            ),
        ],
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            right: px(16),
            width: px(190),
            padding: UiRect::all(px(12)),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        children![
            (
                Text::new("Moves"),
                font(15.0),
                TextColor(Color::srgb(0.95, 0.95, 0.97)),
            ),
            (Text::new(""), font(13.0), TextColor(TEXT_DIM), MoveListText,),
        ],
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            bottom: px(16),
            width: px(34),
            height: px(240),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexEnd,
            border_radius: BorderRadius::all(px(6)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgb(0.10, 0.10, 0.12)),
        children![(
            Node {
                width: percent(100.0),
                height: percent(50.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.92, 0.92, 0.94)),
            EvalFill,
        )],
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(56),
            bottom: px(124),
            ..default()
        },
        Text::new("0.00"),
        font(15.0),
        TextColor(Color::srgb(0.85, 0.87, 0.90)),
        EvalLabel,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(16),
            bottom: px(16),
            width: px(300),
            padding: UiRect::all(px(14)),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            border_radius: BorderRadius::all(px(10)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(PANEL_BG),
        SummaryPanel,
        children![
            (
                Text::new("Accuracy by stage"),
                font(16.0),
                TextColor(Color::srgb(0.95, 0.95, 0.97)),
            ),
            (Text::new(""), font(14.0), TextColor(TEXT_DIM), SummaryText),
        ],
    ));
}

type StatusQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<StatusText>,
        Without<FeedbackText>,
        Without<MoveListText>,
        Without<EvalLabel>,
    ),
>;
type FeedbackQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Text, &'static mut TextColor),
    (
        With<FeedbackText>,
        Without<MoveListText>,
        Without<EvalLabel>,
    ),
>;
type MoveListQuery<'w, 's> =
    Query<'w, 's, &'static mut Text, (With<MoveListText>, Without<EvalLabel>)>;
type SummaryQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<SummaryText>,
        Without<StatusText>,
        Without<FeedbackText>,
        Without<MoveListText>,
        Without<EvalLabel>,
    ),
>;
type OpeningQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<OpeningText>,
        Without<StatusText>,
        Without<FeedbackText>,
        Without<MoveListText>,
        Without<EvalLabel>,
        Without<SummaryText>,
    ),
>;

/// The opening line so far: its name while theory lasts, how mainstream the
/// position is, and where theory ran out.
///
/// `traffic` is the count of database lines running through the current
/// position, and it is the honest qualifier on the name: the database names
/// some very obscure moves, so "Ruy Lopez: Bulgarian Variation" on one line
/// means something quite different from a position sitting on eighty.
fn opening_line(game: &Game, traffic: usize) -> String {
    if let Some(fullmove) = game.left_book_at {
        let mut line = match &game.opening {
            Some(opening) => format!("{}\nleft theory at move {fullmove}", opening.label()),
            None => format!("Left theory at move {fullmove}"),
        };
        if !game.book_alternatives.is_empty() {
            line.push_str(&format!(
                "\nbook: {}",
                game.book_alternatives.join(", ")
            ));
        }
        return line;
    }

    let mainstream = match traffic {
        0 => String::new(),
        1 => "\non 1 book line".to_string(),
        n => format!("\non {n} book lines"),
    };

    match &game.opening {
        Some(opening) => format!("{}{mainstream}", opening.label()),
        // In book, but no line has been named this early.
        None => mainstream.trim_start().to_string(),
    }
}

/// Drill status: what is being drilled, the score, and whose turn it is.
fn drill_status(drill: &Drill, game: &Game) -> String {
    let mut lines = vec![drill.label(), drill.score()];
    if drill.complete {
        if drill.missed.is_empty() {
            lines.push("Line complete - press N to go again".to_string());
        } else {
            lines.push(format!("Missed: {}", drill.missed.join(", ")));
        }
    } else if game.pos.turn() == game.player {
        lines.push("Find the book move".to_string());
    }
    lines.join("\n")
}

fn plural(n: u32, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The breakdown that centipawn grading alone cannot give: not how bad a move
/// was, but which part of the game the bad moves keep landing in.
fn summary_body(game: &Game) -> String {
    let stats = game.stage_summary();
    if stats.is_empty() {
        return "No graded moves yet.\nPlay a few moves and press S again.".to_string();
    }

    let mut lines = Vec::new();
    for stat in &stats {
        let mut headline = match stat.accuracy {
            Some(accuracy) => format!(
                "{}   {accuracy:.0}%   {}",
                stat.stage.label(),
                plural(stat.moves, "move")
            ),
            // Every move was theory, so there is nothing of yours to score.
            None => format!("{}   all theory", stat.stage.label()),
        };
        if stat.book_moves > 0 && stat.accuracy.is_some() {
            headline.push_str(&format!("  (+{} book)", stat.book_moves));
        }
        lines.push(headline);

        let problems = stat.problem_summary();
        if !problems.is_empty() {
            lines.push(format!("   {problems}"));
        }

        // Only worth naming a move that actually cost something.
        if let Some((san, loss)) = &stat.worst
            && *loss > 40
        {
            lines.push(format!("   worst  {san}  -{:.2}", *loss as f32 / 100.0));
        }
    }

    if let Some(overall) = game.overall_accuracy() {
        lines.push(format!("\nOverall   {overall:.0}%"));
    }

    // Only stages that were actually scored can be compared, and with fewer
    // than two of them there is nothing to compare against.
    let scored: Vec<(&str, f32)> = stats
        .iter()
        .filter_map(|s| Some((s.stage.label(), s.accuracy?)))
        .collect();
    if scored.len() > 1
        && let Some((stage, _)) = scored.iter().min_by(|a, b| a.1.total_cmp(&b.1))
    {
        lines.push(format!("Weakest   {stage}"));
    }

    lines.join("\n")
}

// One panel per query, and there are six panels; splitting the system would only
// duplicate the change detection above.
#[allow(clippy::too_many_arguments)]
pub fn update(
    game: Res<Game>,
    engine: Res<EngineLink>,
    book: Res<Book>,
    drill: Option<Res<Drill>>,
    mut status: StatusQuery,
    mut feedback: FeedbackQuery,
    mut move_list: MoveListQuery,
    mut summary: SummaryQuery,
    mut opening: OpeningQuery,
    mut eval_label: Query<&mut Text, With<EvalLabel>>,
    mut eval_fill: Query<&mut Node, With<EvalFill>>,
    mut summary_panel: Query<&mut Node, (With<SummaryPanel>, Without<EvalFill>)>,
) {
    let drill_changed = drill.as_ref().is_some_and(|d| d.is_changed());
    if !game.is_changed() && !engine.is_changed() && !drill_changed {
        return;
    }

    if let Ok(mut text) = status.single_mut() {
        let side = if game.player == ChessColor::White {
            "White"
        } else {
            "Black"
        };
        **text = match game.phase {
            // A drill has no engine opponent and no skill level, so neither
            // belongs in its status.
            _ if drill.is_some() => drill_status(drill.as_ref().unwrap(), &game),
            _ if !engine.available => format!("{}\n(playing without engine)", engine.status),
            Phase::GameOver => game
                .result_text
                .clone()
                .unwrap_or_else(|| "Game over".to_string()),
            Phase::EngineThinking => format!("Stockfish is thinking...  (skill {})", game.skill),
            Phase::PlayerTurn => {
                let check = if game.pos.is_check() {
                    "  - you are in check!"
                } else {
                    ""
                };
                format!("Your move as {side}  (skill {}){check}", game.skill)
            }
        };
    }

    if let Ok((mut text, mut color)) = feedback.single_mut() {
        // While drilling, the answer to the question is the feedback that
        // matters; the analyst's centipawn verdict on a book move is not.
        match (drill.as_ref(), &game.last_grade) {
            (Some(drill), _) => {
                **text = drill.feedback.clone().unwrap_or_default();
                color.0 = match drill.last_ok {
                    Some(true) => Quality::Best.color(),
                    Some(false) => Quality::Mistake.color(),
                    None => BOOK_BLUE,
                };
            }
            (None, Some(grade)) => {
                let mut line = format!("{}  ({})", grade.quality.label(), grade.played_san);
                // A theory move's centipawn cost is an artefact of the search
                // depth, not a fault of yours, so it is not reported.
                if grade.loss_cp > 10 && grade.quality != Quality::Book {
                    line.push_str(&format!("\nlost {:.2} pawns", grade.loss_cp as f32 / 100.0));
                }
                if let Some(better) = &grade.better_san {
                    line.push_str(&format!("\nbetter was {better}"));
                }
                **text = line;
                color.0 = grade.quality.color();
            }
            (None, None) => {
                **text = String::new();
            }
        }
    }

    if let Ok(mut text) = opening.single_mut() {
        **text = opening_line(&game, book.traffic(&game.pos));
    }

    if let Ok(mut text) = move_list.single_mut() {
        let recent: Vec<&str> = game
            .history
            .iter()
            .rev()
            .take(16)
            .rev()
            .map(String::as_str)
            .collect();
        **text = recent.join("  ");
    }

    if let Ok(mut node) = summary_panel.single_mut() {
        node.display = if game.show_summary {
            Display::Flex
        } else {
            Display::None
        };
    }

    if game.show_summary
        && let Ok(mut text) = summary.single_mut()
    {
        **text = summary_body(&game);
    }

    let score = game.evals.get(&game.ply).copied();

    if let Ok(mut text) = eval_label.single_mut() {
        **text = match score {
            Some(s) => s.label(),
            None => "...".to_string(),
        };
    }

    if let Ok(mut node) = eval_fill.single_mut() {
        let fraction = match score {
            Some(Score::Mate(n)) => {
                if n >= 0 {
                    1.0
                } else {
                    0.0
                }
            }
            Some(Score::Centipawns(cp)) => {
                let pawns = cp as f32 / 100.0;
                0.5 + 0.5 * (pawns / 6.0).clamp(-1.0, 1.0).cbrt().clamp(-1.0, 1.0)
            }
            None => 0.5,
        };
        node.height = percent(fraction.clamp(0.0, 1.0) * 100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Quality;
    use crate::openings::Opening;
    use crate::review::{MoveReview, Stage};

    fn review(
        ply: u32,
        stage: Stage,
        quality: Quality,
        loss_cp: i32,
        accuracy: Option<f32>,
    ) -> MoveReview {
        MoveReview {
            ply,
            stage,
            quality,
            loss_cp,
            accuracy,
            san: format!("{}. Nf3", ply.div_ceil(2)),
        }
    }

    fn game_with(reviews: Vec<MoveReview>) -> Game {
        let mut game = Game::new(ChessColor::White, 5);
        game.reviews = reviews;
        game
    }

    #[test]
    fn an_ungraded_game_says_so_instead_of_showing_an_empty_panel() {
        let body = summary_body(&game_with(Vec::new()));
        assert!(body.contains("No graded moves yet"), "{body}");
    }

    #[test]
    fn the_breakdown_names_the_weakest_stage() {
        let body = summary_body(&game_with(vec![
            review(1, Stage::Opening, Quality::Best, 0, Some(100.0)),
            review(3, Stage::Opening, Quality::Good, 20, Some(96.0)),
            review(5, Stage::Middlegame, Quality::Blunder, 310, Some(30.0)),
            review(7, Stage::Middlegame, Quality::Good, 20, Some(96.0)),
        ]));

        assert!(body.contains("Opening   98%   2 moves"), "{body}");
        assert!(body.contains("Middlegame   63%   2 moves"), "{body}");
        assert!(body.contains("1 blunder"), "{body}");
        assert!(body.contains("worst  3. Nf3  -3.10"), "{body}");
        assert!(body.contains("Weakest   Middlegame"), "{body}");
    }

    #[test]
    fn a_single_stage_has_nothing_to_be_weakest_against() {
        let body = summary_body(&game_with(vec![review(
            1,
            Stage::Opening,
            Quality::Best,
            0,
            Some(100.0),
        )]));

        assert!(body.contains("Overall   100%"), "{body}");
        assert!(!body.contains("Weakest"), "{body}");
    }

    #[test]
    fn a_stage_of_pure_theory_is_not_given_a_percentage() {
        let body = summary_body(&game_with(vec![
            review(1, Stage::Opening, Quality::Book, 30, None),
            review(3, Stage::Opening, Quality::Book, 10, None),
            review(5, Stage::Middlegame, Quality::Good, 20, Some(96.0)),
        ]));

        assert!(body.contains("Opening   all theory"), "{body}");
        // Memorised moves must not be scored as if you had found them.
        assert!(!body.contains("Opening   100%"), "{body}");
        assert!(body.contains("Overall   96%"), "{body}");
    }

    #[test]
    fn theory_moves_are_counted_alongside_the_scored_ones() {
        let body = summary_body(&game_with(vec![
            review(1, Stage::Opening, Quality::Book, 0, None),
            review(3, Stage::Opening, Quality::Book, 0, None),
            review(5, Stage::Opening, Quality::Mistake, 150, Some(60.0)),
        ]));

        assert!(body.contains("Opening   60%   1 move  (+2 book)"), "{body}");
    }

    #[test]
    fn an_opening_in_theory_shows_only_its_name() {
        let mut game = game_with(Vec::new());
        game.opening = Some(Opening {
            eco: "B90".to_string(),
            name: "Sicilian Defense: Najdorf Variation".to_string(),
        });

        assert_eq!(
            opening_line(&game, 0),
            "Sicilian Defense: Najdorf Variation (B90)"
        );
    }

    #[test]
    fn leaving_theory_says_where_and_what_would_have_stayed() {
        let mut game = game_with(Vec::new());
        game.opening = Some(Opening {
            eco: "C60".to_string(),
            name: "Ruy Lopez".to_string(),
        });
        game.left_book_at = Some(7);
        game.book_alternatives = vec!["Nf3".to_string(), "d4".to_string()];

        let line = opening_line(&game, 0);
        assert!(line.contains("Ruy Lopez (C60)"), "{line}");
        assert!(line.contains("left theory at move 7"), "{line}");
        assert!(line.contains("book: Nf3, d4"), "{line}");
    }

    #[test]
    fn an_unnamed_position_still_in_book_says_nothing() {
        assert_eq!(opening_line(&game_with(Vec::new()), 0), "");
    }

    #[test]
    fn a_named_line_is_qualified_by_how_mainstream_it_is() {
        let mut game = game_with(Vec::new());
        game.opening = Some(Opening {
            eco: "C60".to_string(),
            name: "Ruy Lopez".to_string(),
        });

        // The same name means something different on 84 lines than on 1.
        assert!(opening_line(&game, 84).contains("on 84 book lines"));
        assert!(opening_line(&game, 1).contains("on 1 book line"));
        // Out of book there is no traffic to report.
        assert!(!opening_line(&game, 0).contains("book line"));
    }

    #[test]
    fn an_unnamed_book_position_still_reports_its_traffic() {
        let line = opening_line(&game_with(Vec::new()), 12);
        assert_eq!(line, "on 12 book lines");
    }

    #[test]
    fn a_drill_in_progress_shows_the_score_and_the_task() {
        let book = Book::load();
        let drill = Drill::new(book.repertoire("Ruy Lopez").unwrap());
        let game = Game::new(ChessColor::White, 5);

        let status = drill_status(&drill, &game);
        assert!(status.contains("Drill: Ruy Lopez"), "{status}");
        assert!(status.contains("0/0 first try"), "{status}");
        assert!(status.contains("Find the book move"), "{status}");
    }

    #[test]
    fn a_finished_drill_lists_what_was_missed() {
        let book = Book::load();
        let mut drill = Drill::new(book.repertoire("Ruy Lopez").unwrap());
        drill.complete = true;
        drill.missed = vec!["Nf3".to_string(), "Bb5".to_string()];

        let status = drill_status(&drill, &Game::new(ChessColor::White, 5));
        assert!(status.contains("Missed: Nf3, Bb5"), "{status}");
        assert!(!status.contains("Find the book move"), "{status}");
    }

    #[test]
    fn a_clean_finish_invites_another_run_instead_of_a_miss_list() {
        let book = Book::load();
        let mut drill = Drill::new(book.repertoire("Ruy Lopez").unwrap());
        drill.complete = true;

        let status = drill_status(&drill, &Game::new(ChessColor::White, 5));
        assert!(status.contains("press N to go again"), "{status}");
    }

    #[test]
    fn a_single_move_is_not_reported_as_moves() {
        let body = summary_body(&game_with(vec![review(
            1,
            Stage::Opening,
            Quality::Best,
            0,
            Some(100.0),
        )]));

        assert!(body.contains("1 move\n") || body.ends_with("1 move"), "{body}");
        assert!(!body.contains("1 moves"), "{body}");
    }
}
