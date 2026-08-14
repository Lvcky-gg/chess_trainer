use std::f32::consts::{FRAC_PI_2, PI};

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use shakmaty::{Color as ChessColor, Position, Role, Square};

use crate::game::{Game, Phase, square_translation};
use crate::stl::{DEFAULT_CLUSTER_RESOLUTION, load_piece_mesh};

const PIECE_MODELS: [(Role, &str, f32); 6] = [
    (Role::Pawn, "01-pawn.stl", 0.70),
    (Role::Knight, "02-knight_v2.stl", 0.88),
    (Role::Bishop, "03-bishop.stl", 0.95),
    (Role::Rook, "04-rook.stl", 0.75),
    (Role::Queen, "05-queen.stl", 1.15),
    (Role::King, "06-king.stl", 1.30),
];

const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/assets");

#[derive(Component)]
pub struct Tile(pub Square);

#[derive(Component)]
pub struct PieceView(pub Square);

#[derive(Component)]
pub struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    radius: f32,
}

#[derive(Resource)]
pub struct BoardAssets {
    pieces: Vec<(Role, Handle<Mesh>)>,
    white_piece: Handle<StandardMaterial>,
    black_piece: Handle<StandardMaterial>,
    light_tile: Handle<StandardMaterial>,
    dark_tile: Handle<StandardMaterial>,
    selected_tile: Handle<StandardMaterial>,
    move_target: Handle<StandardMaterial>,
    capture_target: Handle<StandardMaterial>,
    hint_tile: Handle<StandardMaterial>,
    check_tile: Handle<StandardMaterial>,
}

impl BoardAssets {
    fn mesh_for(&self, role: Role) -> Option<Handle<Mesh>> {
        self.pieces
            .iter()
            .find(|(r, _)| *r == role)
            .map(|(_, h)| h.clone())
    }
}

#[derive(Resource, Default)]
pub struct RenderedPly(pub Option<u32>);

fn tile_is_light(sq: Square) -> bool {
    (sq.file().to_u32() + sq.rank().to_u32()) % 2 == 1
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game: Res<Game>,
) {
    let mut pieces = Vec::new();
    for (role, file, height) in PIECE_MODELS {
        let path = format!("{ASSET_DIR}/{file}");
        let mesh = match load_piece_mesh(&path, height, DEFAULT_CLUSTER_RESOLUTION) {
            Ok(m) => m,
            Err(e) => {
                error!("failed to load {file}: {e} - using a placeholder");
                Cylinder::new(0.28, height).mesh().resolution(24).into()
            }
        };
        pieces.push((role, meshes.add(mesh)));
    }

    let matte = |c: Color, rough: f32, metal: f32| StandardMaterial {
        base_color: c,
        perceptual_roughness: rough,
        metallic: metal,
        ..default()
    };

    let assets = BoardAssets {
        pieces,
        white_piece: materials.add(matte(Color::srgb(0.92, 0.90, 0.85), 0.45, 0.05)),
        black_piece: materials.add(matte(Color::srgb(0.10, 0.10, 0.12), 0.40, 0.10)),
        light_tile: materials.add(matte(Color::srgb(0.87, 0.84, 0.76), 0.75, 0.0)),
        dark_tile: materials.add(matte(Color::srgb(0.36, 0.45, 0.34), 0.75, 0.0)),
        selected_tile: materials.add(matte(Color::srgb(0.95, 0.83, 0.35), 0.7, 0.0)),
        move_target: materials.add(matte(Color::srgb(0.40, 0.70, 0.85), 0.7, 0.0)),
        capture_target: materials.add(matte(Color::srgb(0.85, 0.38, 0.32), 0.7, 0.0)),
        hint_tile: materials.add(matte(Color::srgb(0.55, 0.85, 0.50), 0.7, 0.0)),
        check_tile: materials.add(matte(Color::srgb(0.90, 0.25, 0.25), 0.7, 0.0)),
    };

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(9.4, 0.3, 9.4))),
        MeshMaterial3d(materials.add(matte(Color::srgb(0.24, 0.18, 0.13), 0.6, 0.0))),
        Transform::from_xyz(0.0, -0.28, 0.0),
        Pickable::IGNORE,
    ));

    let tile_mesh = meshes.add(Cuboid::new(1.0, 0.24, 1.0));
    for i in 0..64u32 {
        let sq = Square::new(i);
        let base = if tile_is_light(sq) {
            assets.light_tile.clone()
        } else {
            assets.dark_tile.clone()
        };
        commands
            .spawn((
                Tile(sq),
                Mesh3d(tile_mesh.clone()),
                MeshMaterial3d(base),
                Transform::from_translation(square_translation(sq) - Vec3::Y * 0.12),
            ))
            .observe(on_click);
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.03,
            ..default()
        },
        Transform::from_xyz(6.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        PointLight {
            intensity: 1_800_000.0,
            range: 40.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-6.0, 9.0, -6.0),
    ));

    let yaw = if game.player == ChessColor::White {
        0.0
    } else {
        PI
    };
    commands.spawn((
        Camera3d::default(),
        OrbitCamera {
            yaw,
            pitch: 0.70,
            radius: 14.0,
        },
        AmbientLight {
            color: Color::srgb(0.85, 0.88, 1.0),
            brightness: 220.0,
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.insert_resource(assets);
}

pub fn sync_pieces(
    mut commands: Commands,
    game: Res<Game>,
    assets: Res<BoardAssets>,
    mut rendered: ResMut<RenderedPly>,
    existing: Query<Entity, With<PieceView>>,
) {
    if rendered.0 == Some(game.ply) {
        return;
    }
    rendered.0 = Some(game.ply);

    for e in &existing {
        commands.entity(e).despawn();
    }

    for i in 0..64u32 {
        let sq = Square::new(i);
        let Some(piece) = game.pos.board().piece_at(sq) else {
            continue;
        };
        let Some(mesh) = assets.mesh_for(piece.role) else {
            continue;
        };

        let material = if piece.color == ChessColor::White {
            assets.white_piece.clone()
        } else {
            assets.black_piece.clone()
        };

        let facing = if piece.color == ChessColor::White {
            0.0
        } else {
            PI
        };

        commands
            .spawn((
                PieceView(sq),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(square_translation(sq))
                    .with_rotation(Quat::from_rotation_y(facing)),
            ))
            .observe(on_click);
    }
}

pub fn update_highlights(
    game: Res<Game>,
    assets: Res<BoardAssets>,
    mut tiles: Query<(&Tile, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if !game.is_changed() {
        return;
    }

    let hint_squares = game
        .show_hint
        .then(|| game.best_move.as_ref().and_then(|u| game.parse_uci(u)))
        .flatten()
        .map(|m| (m.from(), m.to()));

    let checked_king = game
        .pos
        .is_check()
        .then(|| game.pos.board().king_of(game.pos.turn()))
        .flatten();

    for (tile, mut material) in &mut tiles {
        let sq = tile.0;

        let handle = if Some(sq) == checked_king {
            assets.check_tile.clone()
        } else if Some(sq) == game.selected {
            assets.selected_tile.clone()
        } else if hint_squares.is_some_and(|(from, to)| from == Some(sq) || to == sq) {
            assets.hint_tile.clone()
        } else if game.targets.iter().any(|(t, _)| *t == sq) {
            if game.pos.board().piece_at(sq).is_some() {
                assets.capture_target.clone()
            } else {
                assets.move_target.clone()
            }
        } else if tile_is_light(sq) {
            assets.light_tile.clone()
        } else {
            assets.dark_tile.clone()
        };

        if material.0 != handle {
            material.0 = handle;
        }
    }
}

fn on_click(
    click: On<Pointer<Click>>,
    tiles: Query<&Tile>,
    pieces: Query<&PieceView>,
    mut game: ResMut<Game>,
) {
    let entity = click.entity;
    let Some(sq) = tiles
        .get(entity)
        .map(|t| t.0)
        .ok()
        .or_else(|| pieces.get(entity).map(|p| p.0).ok())
    else {
        return;
    };

    if game.phase != Phase::PlayerTurn || !game.is_player_turn() {
        return;
    }

    if game.selected.is_some()
        && let Some(m) = game.move_to(sq)
    {
        game.apply(m);
        return;
    }

    if game.selected == Some(sq) {
        game.clear_selection();
    } else {
        game.select(sq);
    }
}

pub fn orbit_camera(
    mut camera: Query<(&mut Transform, &mut OrbitCamera)>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
) {
    let Ok((mut transform, mut orbit)) = camera.single_mut() else {
        return;
    };

    if buttons.pressed(MouseButton::Right) {
        orbit.yaw -= motion.delta.x * 0.006;
        orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(0.15, FRAC_PI_2 - 0.02);
    }

    let dt = time.delta_secs();
    if keys.pressed(KeyCode::ArrowLeft) {
        orbit.yaw -= 1.2 * dt;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        orbit.yaw += 1.2 * dt;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        orbit.pitch = (orbit.pitch + 0.9 * dt).min(FRAC_PI_2 - 0.02);
    }
    if keys.pressed(KeyCode::ArrowDown) {
        orbit.pitch = (orbit.pitch - 0.9 * dt).max(0.15);
    }

    if scroll.delta.y != 0.0 {
        orbit.radius = (orbit.radius - scroll.delta.y * 0.8).clamp(6.0, 28.0);
    }

    let (sy, cy) = orbit.yaw.sin_cos();
    let (sp, cp) = orbit.pitch.sin_cos();
    let offset = Vec3::new(sy * cp, sp, cy * cp) * orbit.radius;
    transform.translation = offset;
    transform.look_at(Vec3::ZERO, Vec3::Y);
}
