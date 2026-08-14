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
                     N  new game  [ ]  engine strength\n\
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

pub fn update(
    game: Res<Game>,
    engine: Res<EngineLink>,
    mut status: StatusQuery,
    mut feedback: FeedbackQuery,
    mut move_list: MoveListQuery,
    mut eval_label: Query<&mut Text, With<EvalLabel>>,
    mut eval_fill: Query<&mut Node, With<EvalFill>>,
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
