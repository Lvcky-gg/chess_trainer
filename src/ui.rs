use bevy::prelude::*;
use bevy::text::FontSize;
use shakmaty::{Color as ChessColor, Position};

use crate::engine::{EngineLink, Score};
use crate::game::{Game, Phase};

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
        lines.push(format!(
            "{}   {:.0}%   {}",
            stat.stage.label(),
            stat.accuracy,
            plural(stat.moves, "move")
        ));

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

    // With only one stage played there is nothing to compare it against.
    if stats.len() > 1
        && let Some(weakest) = stats.iter().min_by(|a, b| a.accuracy.total_cmp(&b.accuracy))
    {
        lines.push(format!("Weakest   {}", weakest.stage.label()));
    }

    lines.join("\n")
}

// One panel per query, and there are six panels; splitting the system would only
// duplicate the change detection above.
#[allow(clippy::too_many_arguments)]
pub fn update(
    game: Res<Game>,
    engine: Res<EngineLink>,
    mut status: StatusQuery,
    mut feedback: FeedbackQuery,
    mut move_list: MoveListQuery,
    mut summary: SummaryQuery,
    mut eval_label: Query<&mut Text, With<EvalLabel>>,
    mut eval_fill: Query<&mut Node, With<EvalFill>>,
    mut summary_panel: Query<&mut Node, (With<SummaryPanel>, Without<EvalFill>)>,
) {
    if !game.is_changed() && !engine.is_changed() {
        return;
    }

    if let Ok(mut text) = status.single_mut() {
        let side = if game.player == ChessColor::White {
            "White"
        } else {
            "Black"
        };
        **text = match game.phase {
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
        match &game.last_grade {
            Some(grade) => {
                let mut line = format!("{}  ({})", grade.quality.label(), grade.played_san);
                if grade.loss_cp > 10 {
                    line.push_str(&format!("\nlost {:.2} pawns", grade.loss_cp as f32 / 100.0));
                }
                if let Some(better) = &grade.better_san {
                    line.push_str(&format!("\nbetter was {better}"));
                }
                **text = line;
                color.0 = grade.quality.color();
            }
            None => {
                **text = String::new();
            }
        }
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
    use crate::review::{MoveReview, Stage};

    fn review(ply: u32, stage: Stage, quality: Quality, loss_cp: i32, accuracy: f32) -> MoveReview {
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
            review(1, Stage::Opening, Quality::Best, 0, 100.0),
            review(3, Stage::Opening, Quality::Good, 20, 96.0),
            review(5, Stage::Middlegame, Quality::Blunder, 310, 30.0),
            review(7, Stage::Middlegame, Quality::Good, 20, 96.0),
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
            100.0,
        )]));

        assert!(body.contains("Overall   100%"), "{body}");
        assert!(!body.contains("Weakest"), "{body}");
    }

    #[test]
    fn a_single_move_is_not_reported_as_moves() {
        let body = summary_body(&game_with(vec![review(
            1,
            Stage::Opening,
            Quality::Best,
            0,
            100.0,
        )]));

        assert!(body.contains("1 move\n") || body.ends_with("1 move"), "{body}");
        assert!(!body.contains("1 moves"), "{body}");
    }
}
